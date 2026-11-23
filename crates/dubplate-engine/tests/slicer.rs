//! The slicer through the whole engine, rendered offline and checked to the sample.

mod common;

use common::{SAMPLE_RATE, render_frames, sine_track};
use dubplate_engine::{
    BeatGrid, BeatLength, Controller, DeckId, EngineError, Renderer, SlicerDomain, SlicerMode,
    SlicerRoll, SlicerSlice, offline_engine,
};

const BLOCK: usize = 256;
const BEAT: usize = 24_000;
const EIGHT_BEATS: usize = 8 * BEAT;

/// A deck A holding a 60 s track on a 120 BPM grid whose first beat is at 0, so a beat is
/// 24,000 frames; the 8-beat domain is 192,000 frames and each of its slices one beat.
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

fn slice(index: u8) -> SlicerSlice {
    SlicerSlice::from_index(index).expect("a slice under 8")
}

fn render(renderer: &mut Renderer, frames: usize) {
    render_frames(renderer, frames, BLOCK);
}

fn render_one(renderer: &mut Renderer) {
    let mut frame = [0.0_f32; 2];
    renderer.render(&mut frame, 2);
}

/// The playhead in track frames.
fn position(controller: &Controller) -> u64 {
    (controller.deck_state(DeckId::A).position_seconds * f64::from(SAMPLE_RATE)).round() as u64
}

/// Plays deck A from `frame` with the slicer on in `mode`, having rendered one frame to take
/// the commands in: the playhead is at `frame + 1`.
fn play_with_slicer(
    controller: &mut Controller,
    renderer: &mut Renderer,
    frame: usize,
    mode: SlicerMode,
    domain: SlicerDomain,
) {
    controller
        .seek(DeckId::A, frame as f64 / f64::from(SAMPLE_RATE))
        .expect("seek");
    controller.play(DeckId::A).expect("play");
    controller
        .slicer_on(DeckId::A, mode, domain)
        .expect("turn the slicer on");
    render_one(renderer);
    assert_eq!(position(controller), frame as u64 + 1);
}

#[test]
fn a_pad_jumps_exactly_on_the_next_slice_boundary() {
    let (mut controller, mut renderer) = deck_on_grid();
    play_with_slicer(
        &mut controller,
        &mut renderer,
        30_000,
        SlicerMode::Slicer,
        SlicerDomain::Eight,
    );
    render(&mut renderer, 999);
    assert_eq!(position(&controller), 31_000);
    controller
        .slicer_pad(DeckId::A, slice(5), true, None)
        .unwrap();
    render(&mut renderer, 16_998);
    assert_eq!(position(&controller), 47_998);
    render_one(&mut renderer);
    assert_eq!(
        position(&controller),
        47_999,
        "the slice has not played in yet"
    );
    render_one(&mut renderer);
    assert_eq!(
        position(&controller),
        5 * BEAT as u64,
        "slice 5 starts at the boundary"
    );
    render_one(&mut renderer);
    assert_eq!(position(&controller), 5 * BEAT as u64 + 1);
    assert_eq!(controller.slicer_state(DeckId::A).slice, Some(slice(5)));
}

#[test]
fn a_held_pad_goes_round_its_slice() {
    let (mut controller, mut renderer) = deck_on_grid();
    play_with_slicer(
        &mut controller,
        &mut renderer,
        30_000,
        SlicerMode::Slicer,
        SlicerDomain::Eight,
    );
    controller
        .slicer_pad(DeckId::A, slice(5), true, None)
        .unwrap();
    render(&mut renderer, 17_999 + 23_999);
    assert_eq!(position(&controller), 6 * BEAT as u64 - 1);
    render_one(&mut renderer);
    assert_eq!(
        position(&controller),
        5 * BEAT as u64,
        "the slice wraps on its end"
    );
}

#[test]
fn releasing_returns_to_the_underlying_position_which_has_advanced() {
    let (mut controller, mut renderer) = deck_on_grid();
    play_with_slicer(
        &mut controller,
        &mut renderer,
        30_000,
        SlicerMode::Slicer,
        SlicerDomain::Eight,
    );
    render(&mut renderer, 999);
    controller
        .slicer_pad(DeckId::A, slice(5), true, None)
        .unwrap();
    render(&mut renderer, 46_000);
    let underlying = 31_000 + 46_000;
    assert!((5 * BEAT as u64..6 * BEAT as u64).contains(&position(&controller)));
    controller
        .slicer_pad(DeckId::A, slice(5), false, None)
        .unwrap();
    render_one(&mut renderer);
    assert_eq!(position(&controller), underlying + 1);
    render(&mut renderer, 99);
    assert_eq!(
        position(&controller),
        underlying + 100,
        "the track plays on from there"
    );
}

#[test]
fn releasing_a_pad_that_is_not_held_changes_nothing() {
    let (mut controller, mut renderer) = deck_on_grid();
    play_with_slicer(
        &mut controller,
        &mut renderer,
        30_000,
        SlicerMode::Slicer,
        SlicerDomain::Eight,
    );
    controller
        .slicer_pad(DeckId::A, slice(5), true, None)
        .unwrap();
    controller
        .slicer_pad(DeckId::A, slice(2), true, None)
        .unwrap();
    controller
        .slicer_pad(DeckId::A, slice(5), false, None)
        .unwrap();
    render(&mut renderer, 17_999);
    assert_eq!(
        position(&controller),
        2 * BEAT as u64,
        "slice 2 is still held"
    );
}

#[test]
fn slicer_loop_wraps_exactly_at_the_domain_end() {
    let (mut controller, mut renderer) = deck_on_grid();
    play_with_slicer(
        &mut controller,
        &mut renderer,
        90_000,
        SlicerMode::SlicerLoop,
        SlicerDomain::Four,
    );
    render(&mut renderer, 5_998);
    assert_eq!(position(&controller), 95_999);
    render_one(&mut renderer);
    assert_eq!(position(&controller), 0, "the domain's end is its start");
    render(&mut renderer, 96_000);
    assert_eq!(position(&controller), 0, "and it keeps going round");
    let state = controller.slicer_state(DeckId::A);
    assert_eq!(state.mode, Some(SlicerMode::SlicerLoop));
    assert_eq!(state.domain_start_seconds, Some(0.0));
    assert_eq!(state.domain_end_seconds, Some(2.0));
}

#[test]
fn slicer_loop_pads_wrap_the_underlying_position_on_the_domain() {
    let (mut controller, mut renderer) = deck_on_grid();
    play_with_slicer(
        &mut controller,
        &mut renderer,
        0,
        SlicerMode::SlicerLoop,
        SlicerDomain::Four,
    );
    render(&mut renderer, 20_000);
    controller
        .slicer_pad(DeckId::A, slice(7), true, None)
        .unwrap();
    render(&mut renderer, 90_000);
    controller
        .slicer_pad(DeckId::A, slice(7), false, None)
        .unwrap();
    render_one(&mut renderer);
    assert_eq!(position(&controller), (20_001 + 90_000) % 96_000 + 1);
}

#[test]
fn slicer_advances_the_domain_when_the_track_leaves_it() {
    let (mut controller, mut renderer) = deck_on_grid();
    play_with_slicer(
        &mut controller,
        &mut renderer,
        EIGHT_BEATS - 2_000,
        SlicerMode::Slicer,
        SlicerDomain::Eight,
    );
    render(&mut renderer, 1_998);
    assert_eq!(position(&controller), EIGHT_BEATS as u64 - 1);
    let before = controller.slicer_state(DeckId::A);
    assert_eq!(before.domain_start_seconds, Some(0.0));
    assert_eq!(before.domain_end_seconds, Some(4.0));
    assert_eq!(before.slice, Some(slice(7)));
    render_one(&mut renderer);
    assert_eq!(
        position(&controller),
        EIGHT_BEATS as u64,
        "the music plays on"
    );
    let after = controller.slicer_state(DeckId::A);
    assert_eq!(after.domain_start_seconds, Some(4.0));
    assert_eq!(after.domain_end_seconds, Some(8.0));
    assert_eq!(after.slice, Some(slice(0)));
}

#[test]
fn a_pad_held_across_the_domain_end_plays_the_same_slice_of_the_next_domain() {
    let (mut controller, mut renderer) = deck_on_grid();
    play_with_slicer(
        &mut controller,
        &mut renderer,
        EIGHT_BEATS - 5_000,
        SlicerMode::Slicer,
        SlicerDomain::Eight,
    );
    controller
        .slicer_pad(DeckId::A, slice(3), true, None)
        .unwrap();
    render(&mut renderer, 4_999);
    assert_eq!(
        position(&controller),
        (EIGHT_BEATS + 3 * BEAT) as u64,
        "slice 3 of the next domain plays in at the old domain's end"
    );
    let state = controller.slicer_state(DeckId::A);
    assert_eq!(state.domain_start_seconds, Some(4.0));
}

#[test]
fn a_roll_repeats_its_share_of_the_domain() {
    for roll in SlicerRoll::ALL {
        let length = (EIGHT_BEATS as f64 * roll.fraction()) as u64;
        let (mut controller, mut renderer) = deck_on_grid();
        play_with_slicer(
            &mut controller,
            &mut renderer,
            5_000,
            SlicerMode::Slicer,
            SlicerDomain::Eight,
        );
        controller
            .slicer_pad(DeckId::A, slice(2), true, Some(roll))
            .unwrap();
        render(&mut renderer, 18_998);
        assert_eq!(position(&controller), 23_999, "{roll:?}");
        render_one(&mut renderer);
        assert_eq!(
            position(&controller),
            2 * BEAT as u64,
            "{roll:?} plays in on the boundary"
        );
        render(&mut renderer, length as usize - 1);
        assert_eq!(
            position(&controller),
            2 * BEAT as u64 + length - 1,
            "{roll:?}"
        );
        render_one(&mut renderer);
        assert_eq!(
            position(&controller),
            2 * BEAT as u64,
            "{roll:?} repeats after {length}"
        );
        render(&mut renderer, length as usize);
        assert_eq!(
            position(&controller),
            2 * BEAT as u64,
            "{roll:?} repeats again"
        );
    }
}

#[test]
fn turning_the_slicer_off_returns_to_the_underlying_position() {
    let (mut controller, mut renderer) = deck_on_grid();
    play_with_slicer(
        &mut controller,
        &mut renderer,
        30_000,
        SlicerMode::SlicerLoop,
        SlicerDomain::Eight,
    );
    controller
        .slicer_pad(DeckId::A, slice(5), true, None)
        .unwrap();
    render(&mut renderer, 40_000);
    controller.slicer_off(DeckId::A).unwrap();
    render_one(&mut renderer);
    assert_eq!(position(&controller), 30_001 + 40_000 + 1);
    assert!(!controller.slicer_state(DeckId::A).enabled);
    render(&mut renderer, EIGHT_BEATS);
    assert_eq!(
        position(&controller),
        30_001 + 40_000 + 1 + EIGHT_BEATS as u64,
        "the domain no longer loops"
    );
}

#[test]
fn a_loop_is_set_aside_while_the_slicer_is_on_and_comes_back_after() {
    let (mut controller, mut renderer) = deck_on_grid();
    controller.seek(DeckId::A, 1.0).unwrap();
    controller.play(DeckId::A).unwrap();
    render(&mut renderer, 10);
    controller.beat_loop(DeckId::A, BeatLength::FOUR).unwrap();
    render(&mut renderer, 10);
    let looped = controller.loop_state(DeckId::A);
    assert!(looped.active);
    controller
        .slicer_on(DeckId::A, SlicerMode::Slicer, SlicerDomain::SixtyFour)
        .unwrap();
    render(&mut renderer, 2 * 96_000);
    assert!(
        position(&controller) > 144_000,
        "the loop did not hold the playhead"
    );
    controller.slicer_off(DeckId::A).unwrap();
    render(&mut renderer, 10);
    assert!(controller.loop_state(DeckId::A).active, "the loop is back");
    controller.seek(DeckId::A, 2.0).unwrap();
    render(&mut renderer, 96_000);
    assert!(
        position(&controller) < 144_000,
        "and it holds the playhead again"
    );
}

#[test]
fn pads_do_nothing_while_the_slicer_is_off() {
    let (mut controller, mut renderer) = deck_on_grid();
    controller.seek(DeckId::A, 0.5).unwrap();
    controller.play(DeckId::A).unwrap();
    controller
        .slicer_pad(DeckId::A, slice(5), true, None)
        .unwrap();
    render(&mut renderer, 100_000);
    assert_eq!(position(&controller), 24_000 + 100_000);
}

#[test]
fn the_slicer_is_refused_without_a_beat_grid() {
    let (mut controller, _renderer) = offline_engine(SAMPLE_RATE);
    controller
        .load_track(DeckId::A, sine_track(440.0, 0.5, 0.5, 10.0, SAMPLE_RATE))
        .unwrap();
    let result = controller.slicer_on(DeckId::A, SlicerMode::Slicer, SlicerDomain::Eight);
    assert!(
        matches!(result, Err(EngineError::NoBeatGrid { .. })),
        "{result:?}"
    );
    assert!(!controller.slicer_state(DeckId::A).enabled);
}

#[test]
fn removing_the_beat_grid_turns_the_slicer_off() {
    let (mut controller, mut renderer) = deck_on_grid();
    play_with_slicer(
        &mut controller,
        &mut renderer,
        30_000,
        SlicerMode::SlicerLoop,
        SlicerDomain::Four,
    );
    controller
        .slicer_pad(DeckId::A, slice(1), true, None)
        .unwrap();
    render(&mut renderer, 5_999);
    assert_eq!(position(&controller), 12_000);

    controller.set_beat_grid(DeckId::A, None).unwrap();
    render(&mut renderer, 10);
    assert!(!controller.slicer_state(DeckId::A).enabled);
    assert_eq!(position(&controller), 36_010);
}
