//! Regression coverage for deck control handover and slip-loop transitions.

use dubplate_engine::{BeatGrid, BeatLength, DeckId, MasterTempo, SyncMode, Track, offline_engine};
#[test]
fn sync_without_a_grid_keeps_the_manual_fader() {
    let (mut c, mut r) = offline_engine(48000);
    let track = Track::from_interleaved_stereo(&vec![0.1; 48000 * 5 * 2], 48000).unwrap();
    c.load_track(DeckId::A, track.clone()).unwrap();
    c.load_track(DeckId::B, track).unwrap();
    c.set_beat_grid(DeckId::A, Some(BeatGrid::new(120.0, 0.0).unwrap()))
        .unwrap();
    c.set_master_tempo(MasterTempo::Deck(DeckId::A)).unwrap();
    c.play(DeckId::A).unwrap();
    c.set_sync(DeckId::B, SyncMode::Beat).unwrap();
    r.render(&mut [0.0; 960], 2);
    c.set_tempo_fader(DeckId::B, 1.0).unwrap();
    r.render(&mut [0.0; 960], 2);
    assert!(
        (c.deck_state(DeckId::B).tempo - 1.1).abs() < 1e-9,
        "tempo={}",
        c.deck_state(DeckId::B).tempo
    );
}
#[test]
fn reloop_with_slip_restarts_the_shadow() {
    let (mut c, mut r) = offline_engine(48000);
    let track = Track::from_interleaved_stereo(&vec![0.1; 48000 * 5 * 2], 48000).unwrap();
    c.load_track(DeckId::A, track).unwrap();
    c.set_beat_grid(DeckId::A, Some(BeatGrid::new(120.0, 0.0).unwrap()))
        .unwrap();
    c.set_slip(DeckId::A, true).unwrap();
    c.play(DeckId::A).unwrap();
    r.render(&mut [0.0; 960], 2);
    c.beat_loop(DeckId::A, BeatLength::ONE).unwrap();
    r.render(&mut [0.0; 960], 2);
    c.reloop_or_exit(DeckId::A).unwrap();
    r.render(&mut [0.0; 960], 2);
    assert!(c.slip_state(DeckId::A).shadow_seconds.is_none());
    c.reloop_or_exit(DeckId::A).unwrap();
    r.render(&mut [0.0; 960], 2);
    assert!(
        c.slip_state(DeckId::A).shadow_seconds.is_some(),
        "reloop failed to begin slip"
    );
}
