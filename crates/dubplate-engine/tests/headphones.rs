//! The headphone cue: channels cued before their fader, mixed with the master and sent to
//! outputs 3 and 4 of a four-channel device, and the channel level meters.

mod common;

use common::{SAMPLE_RATE, assert_close, constant_track};
use dubplate_engine::{Controller, DeckId, EqBand, Renderer, offline_engine};

const FRAMES: usize = 4_800;

/// Deck A plays 0.5 and deck B 0.25 on both sides, both at full trim.
fn two_playing_decks() -> (Controller, Renderer) {
    let (mut controller, renderer) = offline_engine(SAMPLE_RATE);
    controller
        .load_track(DeckId::A, constant_track(0.5, 0.5, 10.0, SAMPLE_RATE))
        .expect("load A");
    controller
        .load_track(DeckId::B, constant_track(0.25, 0.25, 10.0, SAMPLE_RATE))
        .expect("load B");
    controller.play(DeckId::A).expect("play A");
    controller.play(DeckId::B).expect("play B");
    (controller, renderer)
}

/// Renders a while on `channel_count` channels and returns the last frame.
fn last_frame(renderer: &mut Renderer, channel_count: usize) -> Vec<f32> {
    let mut output = vec![0.0_f32; FRAMES * channel_count];
    for _ in 0..4 {
        renderer.render(&mut output, channel_count);
    }
    output[output.len() - channel_count..].to_vec()
}

#[test]
fn a_cued_channel_plays_in_the_headphones_even_with_its_fader_closed() {
    let (mut controller, mut renderer) = two_playing_decks();
    controller.set_channel_fader(DeckId::A, 0.0).unwrap();
    controller.set_channel_fader(DeckId::B, 0.0).unwrap();
    controller.set_headphone_cue(DeckId::A, true).unwrap();
    controller.set_headphone_mix(0.0).unwrap();
    controller.set_headphone_level(1.0).unwrap();
    let frame = last_frame(&mut renderer, 4);
    assert_eq!(&frame[..2], &[0.0, 0.0], "the master is silent");
    assert_close(f64::from(frame[2]), 0.5, 1e-6);
    assert_close(f64::from(frame[3]), 0.5, 1e-6);
    assert!(controller.headphones().is_cued(DeckId::A));
}

#[test]
fn the_mixing_knob_blends_the_cue_with_the_master() {
    let (mut controller, mut renderer) = two_playing_decks();
    controller.set_crossfader(1.0).unwrap();
    controller.set_headphone_cue(DeckId::A, true).unwrap();
    controller.set_headphone_level(1.0).unwrap();
    controller.set_headphone_mix(1.0).unwrap();
    let master_only = last_frame(&mut renderer, 4);
    assert_close(f64::from(master_only[2]), f64::from(master_only[0]), 1e-6);
    assert_close(f64::from(master_only[0]), 0.25, 1e-6);
    controller.set_headphone_mix(0.5).unwrap();
    let blend = last_frame(&mut renderer, 4);
    let side = std::f64::consts::FRAC_1_SQRT_2;
    assert_close(f64::from(blend[2]), (0.5 + 0.25) * side, 1e-5);
}

#[test]
fn master_cue_puts_the_master_on_the_cue_side() {
    let (mut controller, mut renderer) = two_playing_decks();
    controller.set_crossfader(1.0).unwrap();
    controller.set_master_cue(true).unwrap();
    controller.set_headphone_mix(0.0).unwrap();
    controller.set_headphone_level(1.0).unwrap();
    let frame = last_frame(&mut renderer, 4);
    assert_close(f64::from(frame[2]), 0.25, 1e-6);
}

#[test]
fn the_cue_hears_the_eq_and_the_level_knob_turns_it_down() {
    let (mut controller, mut renderer) = two_playing_decks();
    controller.set_headphone_cue(DeckId::A, true).unwrap();
    controller.set_headphone_mix(0.0).unwrap();
    controller.set_headphone_level(0.5).unwrap();
    controller.set_eq(DeckId::A, EqBand::Low, 0.0).unwrap();
    let frame = last_frame(&mut renderer, 4);
    assert!(
        frame[2].abs() < 1e-3,
        "a constant is all low and the low is killed"
    );
    controller.set_eq(DeckId::A, EqBand::Low, 0.5).unwrap();
    let frame = last_frame(&mut renderer, 4);
    assert_close(f64::from(frame[2]), 0.5 * 0.25, 1e-5);
    controller.set_headphone_level(0.0).unwrap();
    let frame = last_frame(&mut renderer, 4);
    assert_eq!(frame[2], 0.0);
}

#[test]
fn a_stereo_device_plays_only_the_master_and_extra_channels_stay_silent() {
    let (mut controller, mut renderer) = two_playing_decks();
    controller.set_headphone_cue(DeckId::A, true).unwrap();
    controller.set_headphone_level(1.0).unwrap();
    let stereo = last_frame(&mut renderer, 2);
    assert_eq!(stereo.len(), 2);
    let six = last_frame(&mut renderer, 6);
    assert_eq!(&six[4..], &[0.0, 0.0]);
    assert!(six[2] != 0.0);
}

#[test]
fn channel_meters_show_the_level_before_the_fader() {
    let (mut controller, mut renderer) = two_playing_decks();
    controller.set_channel_fader(DeckId::A, 0.0).unwrap();
    controller.set_gain(DeckId::B, 2.0).unwrap();
    last_frame(&mut renderer, 2);
    controller.take_meters();
    last_frame(&mut renderer, 2);
    let meters = controller.take_meters();
    assert_close(f64::from(meters.channel_peaks[0]), 0.5, 1e-6);
    assert_eq!(meters.deck_peaks[0], 0.0);
    assert_close(f64::from(meters.channel_peaks[1]), 0.5, 1e-6);
}

#[test]
fn headphone_knobs_are_checked() {
    let (mut controller, _renderer) = offline_engine(SAMPLE_RATE);
    assert!(controller.set_headphone_mix(1.5).is_err());
    assert!(controller.set_headphone_level(f32::NAN).is_err());
}
