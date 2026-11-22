//! The master and headphone limiters: however hard the decks are pushed, nothing clips.

mod common;

use common::{SAMPLE_RATE, constant_track, render_frames, sine_track};
use dubplate_engine::{Controller, DeckId, LIMITER_CEILING, Renderer, offline_engine};

const BLOCK: usize = 256;

fn two_loud_decks(trim: f32) -> (Controller, Renderer) {
    let (mut controller, renderer) = offline_engine(SAMPLE_RATE);
    controller
        .load_track(DeckId::A, sine_track(60.0, 0.9, 0.9, 10.0, SAMPLE_RATE))
        .expect("load A");
    controller
        .load_track(DeckId::B, sine_track(97.0, 0.9, 0.9, 10.0, SAMPLE_RATE))
        .expect("load B");
    for deck in DeckId::ALL {
        controller.set_gain(deck, trim).expect("trim");
        controller.play(deck).expect("play");
    }
    (controller, renderer)
}

#[test]
fn the_master_and_the_headphones_never_pass_the_ceiling() {
    let (mut controller, mut renderer) = two_loud_decks(4.0);
    controller.set_master_gain(4.0).unwrap();
    controller.set_headphone_cue(DeckId::A, true).unwrap();
    controller.set_headphone_cue(DeckId::B, true).unwrap();
    controller.set_headphone_level(1.0).unwrap();
    let mut output = vec![0.0_f32; BLOCK * 4];
    for _ in 0..400 {
        renderer.render(&mut output, 4);
        assert!(output.iter().all(|sample| sample.abs() <= LIMITER_CEILING));
    }
    let meters = controller.take_meters();
    assert!(meters.master_peak <= LIMITER_CEILING);
    assert!(meters.master_peak > LIMITER_CEILING * 0.9);
    assert!(meters.headphone_peak <= LIMITER_CEILING);
    assert!(
        meters.master_limiter_gain < 0.2,
        "gain {}",
        meters.master_limiter_gain
    );
}

#[test]
fn music_under_the_ceiling_is_left_alone() {
    let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
    controller
        .load_track(DeckId::A, constant_track(0.5, 0.5, 4.0, SAMPLE_RATE))
        .unwrap();
    controller.set_crossfader(0.0).unwrap();
    controller.play(DeckId::A).unwrap();
    let output = render_frames(&mut renderer, SAMPLE_RATE as usize, BLOCK);
    assert_eq!(output[output.len() - 1], 0.5);
    assert_eq!(controller.take_meters().master_limiter_gain, 1.0);
}
