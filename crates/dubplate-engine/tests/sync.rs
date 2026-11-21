//! Sync and beatmatching through the whole engine: tempo and beat sync to the tempo master,
//! tempo ranges, the tempo fader's pickup and pitch bend.

mod common;

use common::{SAMPLE_RATE, assert_close, constant_track, render_frames};
use dubplate_engine::{
    BeatGrid, Controller, DeckId, GridBeat, MasterTempo, Renderer, SyncMode, TempoRange,
    offline_engine,
};

const BLOCK: usize = 256;

/// Deck A plays a 124 BPM track as tempo master; deck B holds a track at `bpm_b`, stopped.
fn two_decks(bpm_b: f64) -> (Controller, Renderer) {
    let (mut controller, renderer) = offline_engine(SAMPLE_RATE);
    for deck in DeckId::ALL {
        controller
            .load_track(deck, constant_track(0.1, 0.1, 120.0, SAMPLE_RATE))
            .expect("load a test track");
    }
    controller
        .set_beat_grid(DeckId::A, Some(BeatGrid::new(124.0, 0.1).expect("grid A")))
        .expect("set grid A");
    controller
        .set_beat_grid(DeckId::B, Some(BeatGrid::new(bpm_b, 0.37).expect("grid B")))
        .expect("set grid B");
    controller
        .set_master_tempo(MasterTempo::Deck(DeckId::A))
        .expect("choose the master");
    controller.play(DeckId::A).expect("play A");
    (controller, renderer)
}

fn beat(controller: &Controller, deck: DeckId) -> f64 {
    let grid = controller.beat_grid(deck).expect("a grid");
    grid.beat_at(controller.deck_state(deck).position_seconds)
}

/// How far deck B's beat is from deck A's, in beats, from -0.5 to 0.5.
fn phase_difference(controller: &Controller, multiple: f64) -> f64 {
    let difference = beat(controller, DeckId::A) * multiple - beat(controller, DeckId::B);
    difference - difference.round()
}

fn seconds(renderer: &mut Renderer, seconds: f64) {
    render_frames(renderer, (seconds * f64::from(SAMPLE_RATE)) as usize, BLOCK);
}

#[test]
fn beat_sync_plays_at_the_masters_tempo_with_the_beats_on_the_masters() {
    let (mut controller, mut renderer) = two_decks(128.0);
    controller.set_sync(DeckId::B, SyncMode::Beat).unwrap();
    controller.play(DeckId::B).unwrap();
    seconds(&mut renderer, 0.2);
    let state = controller.tempo_state(DeckId::B);
    assert!(state.following);
    assert!(!state.is_master);
    assert!(controller.tempo_state(DeckId::A).is_master);
    assert_close(state.bpm.unwrap(), 124.0, 0.01);
    assert!(phase_difference(&controller, 1.0).abs() < 0.002);
    seconds(&mut renderer, 20.0);
    assert!(phase_difference(&controller, 1.0).abs() < 0.002);
}

#[test]
fn sync_follows_every_move_of_the_masters_tempo_and_stays_locked() {
    let (mut controller, mut renderer) = two_decks(128.0);
    controller.set_sync(DeckId::B, SyncMode::Beat).unwrap();
    controller.play(DeckId::B).unwrap();
    seconds(&mut renderer, 1.0);
    for step in 1..=40 {
        controller
            .set_tempo_fader(DeckId::A, f64::from(step) / 40.0 * 0.6)
            .unwrap();
        seconds(&mut renderer, 0.05);
    }
    seconds(&mut renderer, 2.0);
    assert_close(controller.deck_state(DeckId::A).tempo, 1.06, 1e-9);
    assert_close(
        controller.tempo_state(DeckId::B).bpm.unwrap(),
        124.0 * 1.06,
        0.01,
    );
    assert!(phase_difference(&controller, 1.0).abs() < 0.002);
}

#[test]
fn tempo_sync_matches_the_tempo_and_leaves_the_beats_alone() {
    let (mut controller, mut renderer) = two_decks(126.0);
    controller.play(DeckId::B).unwrap();
    seconds(&mut renderer, 0.5);
    let before = phase_difference(&controller, 1.0);
    controller.set_sync(DeckId::B, SyncMode::Tempo).unwrap();
    seconds(&mut renderer, 5.0);
    assert_close(controller.tempo_state(DeckId::B).bpm.unwrap(), 124.0, 0.01);
    let after = phase_difference(&controller, 1.0);
    assert!(
        (after - before).abs() < 0.03,
        "phase moved from {before} to {after}"
    );
}

#[test]
fn a_half_tempo_track_locks_to_every_other_beat() {
    let (mut controller, mut renderer) = two_decks(64.0);
    controller.set_sync(DeckId::B, SyncMode::Beat).unwrap();
    controller.play(DeckId::B).unwrap();
    seconds(&mut renderer, 2.0);
    assert_close(controller.tempo_state(DeckId::B).bpm.unwrap(), 62.0, 0.01);
    assert!(phase_difference(&controller, 0.5).abs() < 0.002);
    controller.set_sync_half_and_double(false).unwrap();
    controller.set_sync(DeckId::B, SyncMode::Beat).unwrap();
    seconds(&mut renderer, 0.5);
    assert_close(controller.tempo_state(DeckId::B).bpm.unwrap(), 124.0, 0.01);
}

#[test]
fn a_grid_with_tempo_changes_stays_on_the_masters_beats() {
    let (mut controller, mut renderer) = two_decks(128.0);
    let mut beats = Vec::new();
    let mut seconds_at_beat = 0.25;
    for index in 0..400 {
        let bpm = if index < 40 {
            120.0
        } else {
            121.5 + f64::from(index % 3) * 0.5
        };
        beats.push(GridBeat {
            seconds: seconds_at_beat,
            bpm,
        });
        seconds_at_beat += 60.0 / bpm;
    }
    let grid = BeatGrid::from_beats(&beats).unwrap();
    assert!(grid.has_tempo_changes());
    controller.set_beat_grid(DeckId::B, Some(grid)).unwrap();
    controller.set_sync(DeckId::B, SyncMode::Beat).unwrap();
    controller.play(DeckId::B).unwrap();
    for _ in 0..60 {
        seconds(&mut renderer, 0.5);
        assert!(phase_difference(&controller, 1.0).abs() < 0.003);
        assert_close(controller.tempo_state(DeckId::B).bpm.unwrap(), 124.0, 0.05);
    }
}

#[test]
fn nudging_a_following_deck_turns_beat_sync_into_tempo_sync() {
    let (mut controller, mut renderer) = two_decks(128.0);
    controller.set_sync(DeckId::B, SyncMode::Beat).unwrap();
    controller.play(DeckId::B).unwrap();
    seconds(&mut renderer, 0.5);
    for _ in 0..10 {
        controller.jog_nudge(DeckId::B, 20).unwrap();
        seconds(&mut renderer, 0.01);
    }
    seconds(&mut renderer, 2.0);
    let state = controller.tempo_state(DeckId::B);
    assert_eq!(state.sync, SyncMode::Tempo);
    assert_close(state.bpm.unwrap(), 124.0, 0.01);
    assert!(
        phase_difference(&controller, 1.0).abs() > 0.02,
        "the nudge stays"
    );
}

#[test]
fn switching_sync_off_holds_the_tempo_until_the_fader_reaches_it() {
    let (mut controller, mut renderer) = two_decks(128.0);
    controller.set_sync(DeckId::B, SyncMode::Beat).unwrap();
    controller.play(DeckId::B).unwrap();
    seconds(&mut renderer, 0.5);
    controller.set_tempo_fader(DeckId::B, 0.0).unwrap();
    controller.set_sync(DeckId::B, SyncMode::Off).unwrap();
    seconds(&mut renderer, 0.1);
    let synced_tempo = 124.0 / 128.0;
    assert_close(controller.deck_state(DeckId::B).tempo, synced_tempo, 1e-3);
    assert!(controller.tempo_state(DeckId::B).fader_waits_for_pickup);
    controller.set_tempo_fader(DeckId::B, 0.1).unwrap();
    seconds(&mut renderer, 0.1);
    assert_close(controller.deck_state(DeckId::B).tempo, synced_tempo, 1e-3);
    controller.set_tempo_fader(DeckId::B, -0.4).unwrap();
    seconds(&mut renderer, 0.1);
    assert_close(controller.deck_state(DeckId::B).tempo, 0.96, 1e-9);
    assert!(!controller.tempo_state(DeckId::B).fader_waits_for_pickup);
}

#[test]
fn the_tempo_range_sets_how_far_the_fader_reaches() {
    let (mut controller, mut renderer) = two_decks(128.0);
    controller.set_tempo_fader(DeckId::A, 0.5).unwrap();
    seconds(&mut renderer, 0.01);
    assert_close(controller.deck_state(DeckId::A).tempo, 1.05, 1e-9);
    controller
        .set_tempo_range(DeckId::A, TempoRange::SixteenPercent)
        .unwrap();
    seconds(&mut renderer, 0.01);
    assert_close(controller.deck_state(DeckId::A).tempo, 1.08, 1e-9);
    controller
        .set_tempo_range(DeckId::A, TempoRange::Wide)
        .unwrap();
    controller.set_tempo_fader(DeckId::A, -1.0).unwrap();
    seconds(&mut renderer, 0.01);
    let state = controller.tempo_state(DeckId::A);
    assert_eq!(state.tempo_range, TempoRange::Wide);
    assert_eq!(state.bpm, Some(0.0));
    assert!(controller.set_tempo_fader(DeckId::A, 1.5).is_err());
}

#[test]
fn a_held_pitch_bend_speeds_the_deck_up_and_glides_back() {
    let (mut controller, mut renderer) = two_decks(128.0);
    let start = controller.deck_state(DeckId::A).position_seconds;
    controller.set_pitch_bend(DeckId::A, 0.04).unwrap();
    seconds(&mut renderer, 1.0);
    assert_close(controller.platter_state(DeckId::A).speed, 1.04, 1e-4);
    controller.set_pitch_bend(DeckId::A, 0.0).unwrap();
    seconds(&mut renderer, 1.0);
    assert_close(controller.platter_state(DeckId::A).speed, 1.0, 1e-4);
    let ahead = controller.deck_state(DeckId::A).position_seconds - start - 2.0;
    assert_close(ahead, 0.04, 2e-3);
    assert_close(controller.deck_state(DeckId::A).tempo, 1.0, 0.0);
}

#[test]
fn the_louder_synced_deck_takes_over_as_master_without_a_tempo_jump() {
    let (mut controller, mut renderer) = two_decks(128.0);
    controller
        .set_master_tempo(MasterTempo::LouderDeck)
        .unwrap();
    controller.set_tempo_fader(DeckId::A, 0.3).unwrap();
    controller.set_sync(DeckId::A, SyncMode::Beat).unwrap();
    controller.set_sync(DeckId::B, SyncMode::Beat).unwrap();
    controller.set_crossfader(0.0).unwrap();
    controller.play(DeckId::B).unwrap();
    seconds(&mut renderer, 1.0);
    assert!(controller.tempo_state(DeckId::A).is_master);
    let bpm = controller.tempo_state(DeckId::B).bpm.unwrap();
    assert_close(bpm, 124.0 * 1.03, 0.01);
    controller.set_crossfader(1.0).unwrap();
    seconds(&mut renderer, 1.0);
    assert!(controller.tempo_state(DeckId::B).is_master);
    assert!(controller.tempo_state(DeckId::A).following);
    assert_close(controller.tempo_state(DeckId::B).bpm.unwrap(), bpm, 0.01);
    assert_close(controller.tempo_state(DeckId::A).bpm.unwrap(), bpm, 0.01);
    assert!(controller.tempo_state(DeckId::B).fader_waits_for_pickup);
}
