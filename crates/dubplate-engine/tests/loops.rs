//! Loops, beat jump, hot cues and quantize through the whole engine.

mod common;

use common::{
    SAMPLE_RATE, assert_close, constant_track, largest_step, left_channel, render_frames,
    sine_track,
};
use dubplate_engine::{
    BeatGrid, BeatLength, Controller, DeckId, GridBeat, Renderer, offline_engine,
};

const BLOCK: usize = 256;

/// A deck A holding a 60 s track on a 120 BPM grid whose first beat is at 0, so beat `n`
/// falls at `n / 2` seconds.
fn deck_on_grid() -> (Controller, Renderer) {
    let (mut controller, renderer) = offline_engine(SAMPLE_RATE);
    controller
        .load_track(DeckId::A, sine_track(440.0, 0.5, 0.5, 60.0, SAMPLE_RATE))
        .expect("load the test track");
    controller
        .set_beat_grid(DeckId::A, Some(BeatGrid::new(120.0, 0.0).expect("grid")))
        .expect("set the grid");
    controller.set_crossfader(0.0).expect("set the crossfader");
    (controller, renderer)
}

fn seconds(renderer: &mut Renderer, seconds: f64) -> Vec<f32> {
    left_channel(&render_frames(
        renderer,
        (seconds * f64::from(SAMPLE_RATE)) as usize,
        BLOCK,
    ))
}

fn position(controller: &Controller) -> f64 {
    controller.deck_state(DeckId::A).position_seconds
}

/// The beat position within the beat, from 0 to 1, at 120 BPM.
fn phase(controller: &Controller) -> f64 {
    (position(controller) * 2.0).rem_euclid(1.0)
}

#[test]
fn a_beat_loop_starts_on_the_beat_just_passed_and_goes_round_without_a_click() {
    let (mut controller, mut renderer) = deck_on_grid();
    controller.seek(DeckId::A, 10.3).unwrap();
    controller.play(DeckId::A).unwrap();
    seconds(&mut renderer, 0.1);
    controller.beat_loop(DeckId::A, BeatLength::FOUR).unwrap();
    let output = seconds(&mut renderer, 6.0);
    let state = controller.loop_state(DeckId::A);
    assert!(state.active);
    assert_eq!(state.length, Some(BeatLength::FOUR));
    assert_close(state.start_seconds.unwrap(), 10.0, 1e-9);
    assert_close(state.end_seconds.unwrap(), 12.0, 1e-9);
    let now = position(&controller);
    assert!((10.0..12.0).contains(&now), "position {now}");
    let natural_step = 0.5 * std::f32::consts::TAU * 440.0 / SAMPLE_RATE as f32;
    assert!(
        largest_step(&output) < natural_step * 1.5,
        "step {}",
        largest_step(&output)
    );
}

#[test]
fn halving_and_doubling_change_the_loop_from_its_start() {
    let (mut controller, mut renderer) = deck_on_grid();
    controller.seek(DeckId::A, 5.1).unwrap();
    controller.play(DeckId::A).unwrap();
    controller.beat_loop(DeckId::A, BeatLength::FOUR).unwrap();
    seconds(&mut renderer, 1.7);
    controller.halve_loop(DeckId::A).unwrap();
    seconds(&mut renderer, 0.01);
    let halved = controller.loop_state(DeckId::A);
    assert_eq!(halved.length, Some(BeatLength::TWO));
    assert_close(halved.end_seconds.unwrap(), 6.0, 1e-9);
    assert!((5.0..6.0).contains(&position(&controller)));
    controller.double_loop(DeckId::A).unwrap();
    controller.double_loop(DeckId::A).unwrap();
    seconds(&mut renderer, 0.01);
    assert_eq!(
        controller.loop_state(DeckId::A).length,
        Some(BeatLength::EIGHT)
    );
    for _ in 0..12 {
        controller.halve_loop(DeckId::A).unwrap();
    }
    seconds(&mut renderer, 0.01);
    assert_eq!(
        controller.loop_state(DeckId::A).length,
        Some(BeatLength::SIXTEENTH)
    );
    assert_close(
        controller.loop_state(DeckId::A).end_seconds.unwrap(),
        5.0 + 0.5 / 16.0,
        1e-9,
    );
}

#[test]
fn loop_in_and_loop_out_land_on_the_nearest_beats_with_quantize() {
    let (mut controller, mut renderer) = deck_on_grid();
    controller.seek(DeckId::A, 1.03).unwrap();
    controller.play(DeckId::A).unwrap();
    seconds(&mut renderer, 0.01);
    controller.loop_in(DeckId::A).unwrap();
    seconds(&mut renderer, 0.01);
    assert_close(
        controller
            .loop_state(DeckId::A)
            .pending_start_seconds
            .unwrap(),
        1.0,
        1e-9,
    );
    seconds(&mut renderer, 1.95);
    controller.loop_out(DeckId::A).unwrap();
    seconds(&mut renderer, 3.0);
    let state = controller.loop_state(DeckId::A);
    assert!(state.active);
    assert_eq!(state.length, None);
    assert_close(state.start_seconds.unwrap(), 1.0, 1e-9);
    assert_close(state.end_seconds.unwrap(), 3.0, 1e-9);
    assert!((1.0..3.0).contains(&position(&controller)));
}

#[test]
fn without_quantize_loop_points_stay_where_they_were_set() {
    let (mut controller, mut renderer) = deck_on_grid();
    controller.set_quantize(false).unwrap();
    assert!(!controller.quantize());
    controller.seek(DeckId::A, 1.03).unwrap();
    seconds(&mut renderer, 0.01);
    controller.loop_in(DeckId::A).unwrap();
    seconds(&mut renderer, 0.01);
    assert_close(
        controller
            .loop_state(DeckId::A)
            .pending_start_seconds
            .unwrap(),
        1.03,
        1e-9,
    );
}

#[test]
fn exit_plays_on_past_the_loop_and_reloop_returns_to_it_in_rhythm() {
    let (mut controller, mut renderer) = deck_on_grid();
    controller.seek(DeckId::A, 20.0).unwrap();
    controller.play(DeckId::A).unwrap();
    controller.beat_loop(DeckId::A, BeatLength::TWO).unwrap();
    seconds(&mut renderer, 1.6);
    controller.reloop_or_exit(DeckId::A).unwrap();
    seconds(&mut renderer, 2.0);
    assert!(!controller.loop_state(DeckId::A).active);
    assert!(position(&controller) > 21.0);
    let phase_before = phase(&controller);
    controller.reloop_or_exit(DeckId::A).unwrap();
    seconds(&mut renderer, 0.01);
    assert!(controller.loop_state(DeckId::A).active);
    assert!((20.0..21.0).contains(&position(&controller)));
    let drift = (phase(&controller) - phase_before - 0.02).rem_euclid(1.0);
    assert!(
        !(1e-6..1.0 - 1e-6).contains(&drift),
        "phase moved by {drift}"
    );
}

#[test]
fn beat_jump_moves_by_whole_beats_and_takes_the_loop_along() {
    let (mut controller, mut renderer) = deck_on_grid();
    controller.seek(DeckId::A, 8.2).unwrap();
    seconds(&mut renderer, 0.01);
    controller.beat_jump(DeckId::A, 8.0).unwrap();
    seconds(&mut renderer, 0.01);
    assert_close(position(&controller), 12.2, 1e-9);
    controller.beat_jump(DeckId::A, -16.0).unwrap();
    seconds(&mut renderer, 0.01);
    assert_close(position(&controller), 4.2, 1e-9);
    controller.play(DeckId::A).unwrap();
    controller.beat_loop(DeckId::A, BeatLength::FOUR).unwrap();
    seconds(&mut renderer, 0.1);
    controller.beat_jump(DeckId::A, 4.0).unwrap();
    seconds(&mut renderer, 0.01);
    let state = controller.loop_state(DeckId::A);
    assert_close(state.start_seconds.unwrap(), 6.0, 1e-9);
    assert_close(state.end_seconds.unwrap(), 8.0, 1e-9);
    assert!((6.0..8.0).contains(&position(&controller)));
}

#[test]
fn a_hot_cue_while_playing_lands_in_rhythm_and_starts_a_stopped_deck() {
    let (mut controller, mut renderer) = deck_on_grid();
    controller.seek(DeckId::A, 30.15).unwrap();
    controller.play(DeckId::A).unwrap();
    seconds(&mut renderer, 0.05);
    let phase_before = phase(&controller);
    controller.hot_cue(DeckId::A, 5.0).unwrap();
    seconds(&mut renderer, 0.01);
    let landed = position(&controller);
    assert!((5.0..5.5).contains(&landed), "landed at {landed}");
    let drift = (phase(&controller) - phase_before - 0.02).rem_euclid(1.0);
    assert!(
        !(1e-6..1.0 - 1e-6).contains(&drift),
        "phase moved by {drift}"
    );

    controller.pause(DeckId::A).unwrap();
    seconds(&mut renderer, 0.01);
    controller.hot_cue(DeckId::A, 7.25).unwrap();
    seconds(&mut renderer, 0.01);
    assert!(controller.deck_state(DeckId::A).playing);
    assert_close(position(&controller), 7.26, 1e-9);
}

#[test]
fn the_cue_button_sets_the_cue_on_the_nearest_beat_with_quantize() {
    let (mut controller, mut renderer) = deck_on_grid();
    controller.seek(DeckId::A, 3.9).unwrap();
    seconds(&mut renderer, 0.01);
    controller.cue(DeckId::A).unwrap();
    seconds(&mut renderer, 0.01);
    let state = controller.deck_state(DeckId::A);
    assert_close(state.cue_point_seconds, 4.0, 1e-9);
    assert_close(state.position_seconds, 4.0, 1e-9);
}

#[test]
fn a_loop_on_a_grid_that_changes_tempo_follows_the_beats() {
    let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
    controller
        .load_track(DeckId::A, constant_track(0.2, 0.2, 30.0, SAMPLE_RATE))
        .unwrap();
    let beats = [
        GridBeat {
            seconds: 0.0,
            bpm: 120.0,
        },
        GridBeat {
            seconds: 0.5,
            bpm: 120.0,
        },
        GridBeat {
            seconds: 1.0,
            bpm: 100.0,
        },
        GridBeat {
            seconds: 1.6,
            bpm: 100.0,
        },
    ];
    controller
        .set_beat_grid(DeckId::A, Some(BeatGrid::from_beats(&beats).unwrap()))
        .unwrap();
    controller.seek(DeckId::A, 0.6).unwrap();
    controller.beat_loop(DeckId::A, BeatLength::FOUR).unwrap();
    seconds(&mut renderer, 0.01);
    let state = controller.loop_state(DeckId::A);
    assert_close(state.start_seconds.unwrap(), 0.5, 1e-9);
    assert_close(state.end_seconds.unwrap(), 1.0 + 3.0 * 0.6, 1e-9);
}

#[test]
fn scratching_backwards_inside_a_loop_goes_round_at_its_start() {
    let (mut controller, mut renderer) = deck_on_grid();
    controller.seek(DeckId::A, 2.05).unwrap();
    controller.beat_loop(DeckId::A, BeatLength::TWO).unwrap();
    controller.set_jog_touch(DeckId::A, true).unwrap();
    for _ in 0..40 {
        controller.jog_scratch(DeckId::A, -8).unwrap();
        seconds(&mut renderer, BLOCK as f64 / f64::from(SAMPLE_RATE));
    }
    seconds(&mut renderer, 0.2);
    let now = position(&controller);
    assert!((2.0..3.0).contains(&now), "position {now}");
}
