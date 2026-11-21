//! Jog wheels through the whole engine: scratch, nudge and search, as a DDJ-FLX4 drives them.

mod common;

use common::{
    SAMPLE_RATE, assert_close, constant_track, largest_step, left_channel, ramp_track,
    render_frames,
};
use dubplate_engine::{Controller, DeckId, Renderer, offline_engine};

const BLOCK: usize = 256;
const TICKS_PER_REVOLUTION: f64 = 720.0;
const SCRATCH_SECONDS_PER_REVOLUTION: f64 = 1.8;

fn playing_engine() -> (Controller, Renderer) {
    let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
    controller
        .load_track(DeckId::A, constant_track(0.5, 0.5, 60.0, SAMPLE_RATE))
        .expect("load the test track");
    controller.set_crossfader(0.0).expect("set the crossfader");
    controller.play(DeckId::A).expect("play deck A");
    render_frames(&mut renderer, BLOCK * 8, BLOCK);
    (controller, renderer)
}

fn position(controller: &Controller) -> f64 {
    controller.deck_state(DeckId::A).position_seconds
}

#[test]
fn touching_the_platter_stops_the_music_and_letting_go_resumes_at_the_tempo() {
    let (mut controller, mut renderer) = playing_engine();
    controller.set_tempo(DeckId::A, 1.05).unwrap();
    render_frames(&mut renderer, BLOCK * 4, BLOCK);
    controller.set_jog_touch(DeckId::A, true).unwrap();
    render_frames(&mut renderer, SAMPLE_RATE as usize / 10, BLOCK);
    let held = controller.deck_state(DeckId::A);
    assert!(held.platter_touched);
    assert!(held.playing);
    assert!(
        held.speed.abs() < 1e-3,
        "speed under the hand {}",
        held.speed
    );
    let held_at = held.position_seconds;
    render_frames(&mut renderer, SAMPLE_RATE as usize / 10, BLOCK);
    assert_close(position(&controller), held_at, 1e-4);

    controller.set_jog_touch(DeckId::A, false).unwrap();
    render_frames(&mut renderer, SAMPLE_RATE as usize / 5, BLOCK);
    let released = controller.deck_state(DeckId::A);
    assert!(!released.platter_touched);
    assert_close(released.speed, 1.05, 1e-3);
}

#[test]
fn a_held_record_is_silent_and_the_stop_never_clicks() {
    let (mut controller, mut renderer) = playing_engine();
    controller.set_jog_touch(DeckId::A, true).unwrap();
    let stopping = left_channel(&render_frames(
        &mut renderer,
        SAMPLE_RATE as usize / 5,
        BLOCK,
    ));
    assert!(
        largest_step(&stopping) < 0.01,
        "step {}",
        largest_step(&stopping)
    );
    assert!(stopping[stopping.len() - 1].abs() < 1e-3);
}

#[test]
fn a_full_turn_of_the_platter_scratches_one_point_eight_seconds() {
    let (mut controller, mut renderer) = playing_engine();
    controller.set_jog_touch(DeckId::A, true).unwrap();
    render_frames(&mut renderer, SAMPLE_RATE as usize / 10, BLOCK);
    let start = position(&controller);
    for _ in 0..120 {
        controller.jog_scratch(DeckId::A, 6).unwrap();
        render_frames(&mut renderer, BLOCK, BLOCK);
    }
    render_frames(&mut renderer, SAMPLE_RATE as usize / 10, BLOCK);
    assert_close(
        position(&controller) - start,
        SCRATCH_SECONDS_PER_REVOLUTION,
        1e-4,
    );
    for _ in 0..60 {
        controller.jog_scratch(DeckId::A, -12).unwrap();
        render_frames(&mut renderer, BLOCK, BLOCK);
    }
    render_frames(&mut renderer, SAMPLE_RATE as usize / 10, BLOCK);
    assert_close(position(&controller), start, 1e-4);
}

#[test]
fn scratching_backwards_plays_the_music_backwards() {
    let frame_count = SAMPLE_RATE as usize * 10;
    let slope = 1.0 / frame_count as f32;
    let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
    controller
        .load_track(DeckId::A, ramp_track(slope, frame_count, SAMPLE_RATE))
        .unwrap();
    controller.set_crossfader(0.0).unwrap();
    controller.seek(DeckId::A, 5.0).unwrap();
    controller.set_jog_touch(DeckId::A, true).unwrap();
    let mut output = Vec::new();
    for _ in 0..80 {
        controller.jog_scratch(DeckId::A, -4).unwrap();
        output.extend(left_channel(&render_frames(&mut renderer, BLOCK, BLOCK)));
    }
    let speed = controller.deck_state(DeckId::A).speed;
    let expected_speed =
        -4.0 / TICKS_PER_REVOLUTION * SCRATCH_SECONDS_PER_REVOLUTION * f64::from(SAMPLE_RATE)
            / BLOCK as f64;
    assert_close(speed, expected_speed, 0.02);
    let late = &output[output.len() - BLOCK * 4..];
    assert!(
        late.windows(2).all(|pair| pair[1] < pair[0]),
        "the ramp plays backwards"
    );
}

#[test]
fn a_nudge_bends_the_tempo_for_a_moment_and_moves_the_music_ahead() {
    let (mut controller, mut renderer) = playing_engine();
    let start = position(&controller);
    for _ in 0..10 {
        controller.jog_nudge(DeckId::A, 9).unwrap();
        render_frames(&mut renderer, BLOCK, BLOCK);
    }
    let bent = controller.deck_state(DeckId::A).speed;
    assert!(bent > 1.05 && bent < 1.9, "bent speed {bent}");
    render_frames(&mut renderer, SAMPLE_RATE as usize, BLOCK);
    let played = (10 * BLOCK + SAMPLE_RATE as usize) as f64 / f64::from(SAMPLE_RATE);
    let ahead = position(&controller) - start - played;
    assert_close(ahead, 90.0 / TICKS_PER_REVOLUTION * 0.3, 1e-3);
    assert_close(controller.deck_state(DeckId::A).speed, 1.0, 1e-6);
}

#[test]
fn turning_a_stopped_deck_moves_the_playhead_by_hand() {
    let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
    controller
        .load_track(DeckId::A, constant_track(0.5, 0.5, 30.0, SAMPLE_RATE))
        .unwrap();
    controller.seek(DeckId::A, 2.0).unwrap();
    let mut heard = false;
    for _ in 0..20 {
        controller.jog_nudge(DeckId::A, 3).unwrap();
        let output = render_frames(&mut renderer, BLOCK, BLOCK);
        heard |= output.iter().any(|sample| sample.abs() > 0.1);
    }
    render_frames(&mut renderer, SAMPLE_RATE as usize / 4, BLOCK);
    assert!(heard, "the music sounds under the hand");
    let expected = 2.0 + 60.0 / TICKS_PER_REVOLUTION * SCRATCH_SECONDS_PER_REVOLUTION;
    assert_close(position(&controller), expected, 1e-4);
    let state = controller.deck_state(DeckId::A);
    assert!(!state.playing);
    assert_eq!(state.speed, 0.0);
    let silence = render_frames(&mut renderer, BLOCK, BLOCK);
    assert!(silence.iter().all(|sample| *sample == 0.0));
}

#[test]
fn shift_and_jog_search_fifteen_seconds_per_turn() {
    let (mut controller, mut renderer) = playing_engine();
    let start = position(&controller);
    controller.jog_search(DeckId::A, 360).unwrap();
    render_frames(&mut renderer, BLOCK, BLOCK);
    let moved = position(&controller) - start - BLOCK as f64 / f64::from(SAMPLE_RATE);
    assert_close(moved, 7.5, 1e-6);
    controller.jog_search(DeckId::A, -1_440).unwrap();
    render_frames(&mut renderer, BLOCK, BLOCK);
    assert!(
        position(&controller) < 0.1,
        "search stops at the start of the track"
    );
}

#[test]
fn out_of_vinyl_mode_the_platter_top_nudges() {
    let (mut controller, mut renderer) = playing_engine();
    controller.set_vinyl_mode(DeckId::A, false).unwrap();
    controller.set_jog_touch(DeckId::A, true).unwrap();
    controller.jog_scratch(DeckId::A, 20).unwrap();
    render_frames(&mut renderer, BLOCK, BLOCK);
    let state = controller.deck_state(DeckId::A);
    assert!(!state.vinyl_mode);
    assert!(!state.platter_touched);
    assert!(state.speed > 1.0);
}

#[test]
fn a_finer_jog_moves_less_per_tick() {
    let (mut controller, mut renderer) = playing_engine();
    controller.set_jog_ticks_per_revolution(1_440).unwrap();
    let start = position(&controller);
    controller.jog_search(DeckId::A, 360).unwrap();
    render_frames(&mut renderer, BLOCK, BLOCK);
    let moved = position(&controller) - start - BLOCK as f64 / f64::from(SAMPLE_RATE);
    assert_close(moved, 3.75, 1e-6);
    assert!(controller.set_jog_ticks_per_revolution(0).is_err());
}
