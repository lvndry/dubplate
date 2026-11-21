//! The mixer channel's isolator EQ and filter knob, heard through the whole engine.

mod common;

use common::{
    SAMPLE_RATE, largest_step, left_channel, mixer_latency_frames, render_frames, sine_track,
};
use dubplate_engine::{DeckId, EqBand, gain_to_decibels, offline_engine};

const BLOCK: usize = 256;

/// Plays a sine on deck A through its channel and returns the settled level in decibels.
fn level_db(frequency: f64, set_up: impl FnOnce(&mut dubplate_engine::Controller)) -> f32 {
    let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
    controller
        .load_track(DeckId::A, sine_track(frequency, 0.5, 0.5, 3.0, SAMPLE_RATE))
        .expect("load the test track");
    controller.set_crossfader(0.0).expect("set the crossfader");
    set_up(&mut controller);
    controller.play(DeckId::A).expect("play deck A");
    let output = left_channel(&render_frames(&mut renderer, SAMPLE_RATE as usize, BLOCK));
    let settled = &output[SAMPLE_RATE as usize / 2..];
    let mean_square =
        settled.iter().map(|sample| sample * sample).sum::<f32>() / settled.len() as f32;
    gain_to_decibels(mean_square.sqrt() * std::f32::consts::SQRT_2 / 0.5)
}

#[test]
fn eq_knobs_at_the_centre_and_a_flat_filter_leave_the_music_untouched() {
    let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
    let track = sine_track(997.0, 0.5, 0.25, 2.0, SAMPLE_RATE);
    controller.load_track(DeckId::A, track.clone()).unwrap();
    controller.set_crossfader(0.0).unwrap();
    controller.play(DeckId::A).unwrap();
    let output = render_frames(&mut renderer, SAMPLE_RATE as usize, BLOCK);
    let latency = mixer_latency_frames(&controller);
    assert!(latency > 0, "the EQ reports its latency");
    for frame in [1_000, 20_000, 40_000] {
        let [left, right] = track.frame(frame - latency).unwrap();
        assert_eq!(output[frame * 2], left);
        assert_eq!(output[frame * 2 + 1], right);
    }
}

#[test]
fn turning_an_eq_knob_fully_left_kills_its_band() {
    let killed = |band| {
        move |controller: &mut dubplate_engine::Controller| {
            controller.set_eq(DeckId::A, band, 0.0).unwrap();
        }
    };
    assert!(level_db(55.0, killed(EqBand::Low)) < -60.0);
    assert!(level_db(1_000.0, killed(EqBand::Mid)) < -25.0);
    assert!(level_db(8_000.0, killed(EqBand::High)) < -40.0);
    assert!(level_db(8_000.0, killed(EqBand::Low)).abs() < 0.5);
    assert!(level_db(55.0, killed(EqBand::High)).abs() < 0.5);
}

#[test]
fn an_eq_knob_fully_right_boosts_six_decibels() {
    let boosted = level_db(55.0, |controller| {
        controller.set_eq(DeckId::A, EqBand::Low, 1.0).unwrap()
    });
    assert!((boosted - 6.0).abs() < 0.3, "boost {boosted} dB");
}

#[test]
fn the_filter_knob_is_a_low_pass_to_the_left_and_a_high_pass_to_the_right() {
    let filtered = |position: f32| {
        move |controller: &mut dubplate_engine::Controller| {
            controller.set_filter(DeckId::A, position).unwrap();
        }
    };
    assert!(level_db(5_000.0, filtered(0.1)) < -30.0);
    assert!(level_db(60.0, filtered(0.1)).abs() < 1.0);
    assert!(level_db(60.0, filtered(0.9)) < -30.0);
    assert!(level_db(8_000.0, filtered(0.9)).abs() < 1.5);
    assert!(level_db(60.0, filtered(0.5)).abs() < 0.01);
}

#[test]
fn flicking_a_kill_and_sweeping_the_filter_never_click() {
    let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
    controller
        .load_track(DeckId::A, sine_track(80.0, 0.8, 0.8, 4.0, SAMPLE_RATE))
        .unwrap();
    controller.set_crossfader(0.0).unwrap();
    controller.play(DeckId::A).unwrap();
    render_frames(&mut renderer, SAMPLE_RATE as usize / 4, BLOCK);
    let natural_step = 0.8 * std::f32::consts::TAU * 80.0 / SAMPLE_RATE as f32;
    let mut worst = 0.0_f32;
    for position in [0.0, 1.0, 0.0, 0.5] {
        controller.set_eq(DeckId::A, EqBand::Low, position).unwrap();
        controller.set_filter(DeckId::A, 1.0 - position).unwrap();
        let output = left_channel(&render_frames(&mut renderer, BLOCK * 4, BLOCK));
        worst = worst.max(largest_step(&output));
    }
    assert!(
        worst < natural_step * 3.0,
        "step {worst}, natural {natural_step}"
    );
}

#[test]
fn the_controller_remembers_each_channel_knob() {
    let (mut controller, _renderer) = offline_engine(SAMPLE_RATE);
    assert_eq!(controller.channel(DeckId::B).eq(EqBand::Mid), 0.5);
    controller.set_eq(DeckId::B, EqBand::Mid, 0.2).unwrap();
    controller.set_filter(DeckId::B, 0.7).unwrap();
    controller.set_filter_resonance(DeckId::B, 0.9).unwrap();
    let channel = controller.channel(DeckId::B);
    assert_eq!(channel.eq(EqBand::Mid), 0.2);
    assert_eq!(channel.filter, 0.7);
    assert_eq!(channel.filter_resonance, 0.9);
    assert_eq!(controller.channel(DeckId::A).filter, 0.5);
    assert!(controller.set_eq(DeckId::A, EqBand::Low, 1.5).is_err());
    assert!(controller.set_filter(DeckId::A, f32::NAN).is_err());
}
