//! Halving and doubling a loop that is on: it keeps going round, from the same start, with the
//! playhead inside it.

mod common;

use common::{SAMPLE_RATE, assert_close, sine_track};
use dubplate_engine::{
    BeatGrid, BeatLength, Controller, DeckId, MasterTempo, Renderer, SyncMode, offline_engine,
};

const BLOCK: usize = 256;
const LOOP_START_SECONDS: f64 = 10.0;

fn deck_looping(quantize: bool, length: BeatLength, run_frames: usize) -> (Controller, Renderer) {
    let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
    controller
        .load_track(DeckId::A, sine_track(440.0, 0.5, 0.5, 60.0, SAMPLE_RATE))
        .expect("load the test track");
    controller
        .set_beat_grid(DeckId::A, Some(BeatGrid::new(120.0, 0.0).expect("grid")))
        .expect("set the grid");
    controller.set_crossfader(0.0).expect("set the crossfader");
    controller.set_quantize(quantize).expect("set quantize");
    controller
        .seek(DeckId::A, LOOP_START_SECONDS)
        .expect("seek");
    controller.play(DeckId::A).expect("play");
    render(&mut renderer, BLOCK);
    controller.beat_loop(DeckId::A, length).expect("loop");
    render(&mut renderer, run_frames);
    (controller, renderer)
}

fn render(renderer: &mut Renderer, frames: usize) {
    let mut rendered = 0;
    while rendered < frames {
        let block = BLOCK.min(frames - rendered);
        let mut output = vec![0.0_f32; block * 2];
        renderer.render(&mut output, 2);
        rendered += block;
    }
}

fn assert_going_round(controller: &Controller, start: f64) {
    let state = controller.loop_state(DeckId::A);
    assert!(state.active, "the loop went off");
    assert_close(state.start_seconds.expect("a start"), start, 1e-9);
    let end = state.end_seconds.expect("an end");
    let position = controller.deck_state(DeckId::A).position_seconds;
    assert!(
        (start..end).contains(&position),
        "position {position} outside {start}..{end}"
    );
    assert!(controller.deck_state(DeckId::A).playing);
}

fn loop_beats(controller: &Controller) -> f64 {
    let state = controller.loop_state(DeckId::A);
    (state.end_seconds.expect("an end") - state.start_seconds.expect("a start")) * 2.0
}

#[test]
fn halving_all_the_way_down_keeps_the_loop_going_at_any_playhead() {
    for quantize in [true, false] {
        for offset_blocks in 0..8 {
            let (mut controller, mut renderer) =
                deck_looping(quantize, BeatLength::THIRTY_TWO, offset_blocks * 37 + 1);
            let start = controller.loop_state(DeckId::A).start_seconds.unwrap();
            let mut expected = 32.0;
            while expected > 1.0 / 32.0 {
                controller.halve_loop(DeckId::A).unwrap();
                render(&mut renderer, 1 + (offset_blocks * 11) % BLOCK);
                expected /= 2.0;
                assert_going_round(&controller, start);
                assert_close(loop_beats(&controller), expected, 1e-9);
            }
            controller.halve_loop(DeckId::A).unwrap();
            render(&mut renderer, BLOCK);
            assert_going_round(&controller, start);
            assert_close(loop_beats(&controller), 1.0 / 32.0, 1e-9);
        }
    }
}

#[test]
fn doubling_all_the_way_up_keeps_the_loop_going_at_any_playhead() {
    for quantize in [true, false] {
        for offset_blocks in 0..8 {
            let (mut controller, mut renderer) =
                deck_looping(quantize, BeatLength::THIRTY_SECOND, offset_blocks * 3 + 1);
            let start = controller.loop_state(DeckId::A).start_seconds.unwrap();
            let mut expected = 1.0 / 32.0;
            while expected < 32.0 {
                controller.double_loop(DeckId::A).unwrap();
                render(&mut renderer, 1 + (offset_blocks * 13) % BLOCK);
                expected *= 2.0;
                assert_going_round(&controller, start);
                assert_close(loop_beats(&controller), expected, 1e-9);
            }
            controller.double_loop(DeckId::A).unwrap();
            render(&mut renderer, BLOCK);
            assert_going_round(&controller, start);
            assert_close(loop_beats(&controller), 32.0, 1e-9);
        }
    }
}

#[test]
fn halving_and_doubling_in_a_row_keep_the_loop_going() {
    for quantize in [true, false] {
        let (mut controller, mut renderer) = deck_looping(quantize, BeatLength::FOUR, 5_000);
        let start = controller.loop_state(DeckId::A).start_seconds.unwrap();
        for step in 0..60 {
            if step % 3 == 2 {
                controller.double_loop(DeckId::A).unwrap();
            } else {
                controller.halve_loop(DeckId::A).unwrap();
            }
            render(&mut renderer, 1 + (step * 97) % 3_000);
            assert_going_round(&controller, start);
        }
    }
}

#[test]
fn a_playhead_past_the_new_end_goes_into_the_halved_loop_in_rhythm() {
    for quantize in [true, false] {
        let (mut controller, mut renderer) = deck_looping(quantize, BeatLength::FOUR, 60_000);
        let start = controller.loop_state(DeckId::A).start_seconds.unwrap();
        let before = controller.deck_state(DeckId::A).position_seconds;
        let into_loop = before - start;
        assert!(into_loop > 1.0, "the playhead is in the second half");
        controller.halve_loop(DeckId::A).unwrap();
        render(&mut renderer, 1);
        assert_going_round(&controller, start);
        let landed = controller.deck_state(DeckId::A).position_seconds - start;
        let expected = (into_loop + 1.0 / f64::from(SAMPLE_RATE)).rem_euclid(1.0);
        assert_close(landed, expected, 1e-6);
    }
}

#[test]
fn a_short_manual_loop_goes_round_with_and_without_key_lock() {
    for key_lock in [false, true] {
        for tempo in [1.0, 1.05] {
            let (mut controller, mut renderer) = deck_looping(false, BeatLength::FOUR, 5_000);
            controller.set_key_lock(DeckId::A, key_lock).unwrap();
            controller.set_tempo(DeckId::A, tempo).unwrap();
            controller.reloop_or_exit(DeckId::A).unwrap();
            render(&mut renderer, 20_000);
            controller.loop_in(DeckId::A).unwrap();
            render(&mut renderer, 1_300);
            controller.loop_out(DeckId::A).unwrap();
            for _ in 0..20 {
                render(&mut renderer, 700);
                let state = controller.loop_state(DeckId::A);
                let position = controller.deck_state(DeckId::A).position_seconds;
                assert!(state.active);
                let start = state.start_seconds.unwrap();
                let end = state.end_seconds.unwrap();
                assert!(
                    (start..end).contains(&position),
                    "key lock {key_lock} tempo {tempo}: {position} outside {start}..{end}"
                );
            }
        }
    }
}

#[test]
fn loop_out_on_an_exited_loop_ends_it_at_the_playhead_and_goes_round() {
    let (mut controller, mut renderer) = deck_looping(false, BeatLength::ONE, 1_000);
    controller.set_key_lock(DeckId::A, true).unwrap();
    controller.set_tempo(DeckId::A, 1.05).unwrap();
    render(&mut renderer, 3_000);
    let start = controller.loop_state(DeckId::A).start_seconds.unwrap();
    controller.reloop_or_exit(DeckId::A).unwrap();
    render(&mut renderer, 600);
    assert!(!controller.loop_state(DeckId::A).active);
    controller.loop_out(DeckId::A).unwrap();
    render(&mut renderer, 1);
    let state = controller.loop_state(DeckId::A);
    let position = controller.deck_state(DeckId::A).position_seconds;
    assert!(state.active);
    assert_close(state.start_seconds.unwrap(), start, 1e-9);
    let end = state.end_seconds.unwrap();
    assert!(
        (start..end).contains(&position),
        "{position} outside {start}..{end}"
    );
}

#[test]
fn letting_go_of_a_roll_lands_back_inside_the_loop_that_was_on() {
    for key_lock in [false, true] {
        let (mut controller, mut renderer) = deck_looping(true, BeatLength::ONE, 3_000);
        controller.set_key_lock(DeckId::A, key_lock).unwrap();
        controller.set_tempo(DeckId::A, 1.05).unwrap();
        render(&mut renderer, 3_000);
        let state = controller.loop_state(DeckId::A);
        let (start, end) = (state.start_seconds.unwrap(), state.end_seconds.unwrap());
        controller
            .beat_roll(DeckId::A, BeatLength::THIRTY_TWO)
            .unwrap();
        render(&mut renderer, 2_000);
        controller.release_roll(DeckId::A).unwrap();
        render(&mut renderer, 100);
        let state = controller.loop_state(DeckId::A);
        let position = controller.deck_state(DeckId::A).position_seconds;
        assert!(state.active);
        assert_close(state.start_seconds.unwrap(), start, 1e-9);
        assert_close(state.end_seconds.unwrap(), end, 1e-9);
        assert!(
            (start..end).contains(&position),
            "key lock {key_lock}: {position} outside {start}..{end}"
        );
    }
}

#[test]
fn halving_a_loop_on_a_deck_in_beat_sync_keeps_it_in_the_loop() {
    for mode in [SyncMode::Beat, SyncMode::Tempo] {
        let (mut controller, mut renderer) = deck_looping(true, BeatLength::FOUR, 2_000);
        controller
            .load_track(DeckId::B, sine_track(220.0, 0.5, 0.5, 60.0, SAMPLE_RATE))
            .unwrap();
        controller
            .set_beat_grid(DeckId::B, Some(BeatGrid::new(126.0, 0.0).unwrap()))
            .unwrap();
        controller.seek(DeckId::B, 20.0).unwrap();
        controller.play(DeckId::B).unwrap();
        controller
            .set_master_tempo(MasterTempo::Deck(DeckId::B))
            .unwrap();
        controller.set_sync(DeckId::A, mode).unwrap();
        render(&mut renderer, 20_000);
        let start = controller.loop_state(DeckId::A).start_seconds.unwrap();
        for _ in 0..7 {
            controller.halve_loop(DeckId::A).unwrap();
            for _ in 0..12 {
                render(&mut renderer, 700);
                assert_going_round(&controller, start);
            }
        }
    }
}
