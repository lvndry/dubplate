//! Deck transfer and prepared-load regressions across the control/audio boundary.

use dubplate_engine::{
    BeatGrid, DeckId, DeckRestorePolicy, EngineError, MasterTempo, SyncMode, Track, offline_engine,
};

fn track(rate: u32) -> Track {
    Track::from_interleaved_stereo(&vec![0.1; rate as usize * 8 * 2], rate)
        .expect("valid stereo test track")
}

fn flush(renderer: &mut dubplate_engine::Renderer) {
    renderer.render(&mut [0.0; 960], 2);
}

#[test]
fn copies_effective_tempo_and_fader_pickup_without_copying_mixer_settings() {
    let (mut source, mut renderer) = offline_engine(48_000);
    for deck in DeckId::ALL {
        source.load_track(deck, track(48_000)).unwrap();
        source
            .set_beat_grid(
                deck,
                Some(BeatGrid::new(if deck == DeckId::A { 128.0 } else { 124.0 }, 0.0).unwrap()),
            )
            .unwrap();
    }
    source
        .set_master_tempo(MasterTempo::Deck(DeckId::B))
        .unwrap();
    source.play(DeckId::B).unwrap();
    source.set_sync(DeckId::A, SyncMode::Beat).unwrap();
    flush(&mut renderer);
    source.set_sync(DeckId::A, SyncMode::Off).unwrap();
    flush(&mut renderer);
    let tempo = source.deck_state(DeckId::A).tempo;
    assert!((tempo - 124.0 / 128.0).abs() < 1e-8);
    source.duplicate_deck(DeckId::A, DeckId::B).unwrap();
    flush(&mut renderer);
    assert!((source.deck_state(DeckId::B).tempo - tempo).abs() < 1e-8);
    assert!(source.tempo_state(DeckId::B).fader_waits_for_pickup);
    let snapshot = source.snapshot_deck(DeckId::A).unwrap();
    let (mut target, mut target_renderer) = offline_engine(44_100);
    target
        .restore_deck(DeckId::A, &snapshot, DeckRestorePolicy::DeviceChange)
        .unwrap();
    flush(&mut target_renderer);
    assert!((target.deck_state(DeckId::A).tempo - tempo).abs() < 1e-8);
    assert!(target.tempo_state(DeckId::A).fader_waits_for_pickup);
    assert!(!target.deck_state(DeckId::A).playing);
}

#[test]
fn instant_double_observes_queued_load_and_seek_in_order() {
    let (mut controller, mut renderer) = offline_engine(48_000);
    controller.load_track(DeckId::A, track(48_000)).unwrap();
    controller.seek(DeckId::A, 5.0).unwrap();
    controller.play(DeckId::A).unwrap();
    flush(&mut renderer);
    controller.load_track(DeckId::A, track(44_100)).unwrap();
    controller.seek(DeckId::A, 1.5).unwrap();
    assert!(matches!(
        controller.snapshot_deck(DeckId::A),
        Err(EngineError::DeckStatePending)
    ));
    controller.duplicate_deck(DeckId::A, DeckId::B).unwrap();
    flush(&mut renderer);
    let copy = controller.deck_state(DeckId::B);
    assert!(!copy.playing);
    assert!((copy.position_seconds - 1.5).abs() < 1e-8, "{copy:?}");
    assert_eq!(controller.track(DeckId::B).unwrap().sample_rate(), 44_100);
}

#[test]
fn snapshots_also_reject_unpublished_tempo_commands() {
    let (mut controller, mut renderer) = offline_engine(48_000);
    controller.load_track(DeckId::A, track(48_000)).unwrap();
    flush(&mut renderer);
    controller.set_tempo_fader(DeckId::A, 1.0).unwrap();
    assert!(matches!(
        controller.snapshot_deck(DeckId::A),
        Err(EngineError::DeckStatePending)
    ));
    flush(&mut renderer);
    assert!(controller.snapshot_deck(DeckId::A).is_ok());
}

#[test]
fn full_queue_cannot_partially_replace_a_prepared_track() {
    let (mut controller, mut renderer) = offline_engine(48_000);
    controller.load_track(DeckId::A, track(48_000)).unwrap();
    flush(&mut renderer);
    while controller.set_gain(DeckId::A, 1.0).is_ok() {}
    assert!(matches!(
        controller.load_prepared_track(DeckId::A, track(44_100), None),
        Err(EngineError::CommandQueueFull)
    ));
    assert_eq!(controller.track(DeckId::A).unwrap().sample_rate(), 48_000);
    for _ in 0..5 {
        flush(&mut renderer);
    }
    assert_eq!(controller.track(DeckId::A).unwrap().sample_rate(), 48_000);
}
