//! Measures the resonant filters and the sweep filter on rendered audio.

mod common;

use std::f32::consts::{FRAC_1_SQRT_2, PI};

use common::{Deck, SAMPLE_RATE, Stereo, decibels, largest_kink, rms, seconds_to_frames, sine};
use dubplate_fx::{Effect, Filter, FilterMode, FilterSlope, SweepFilter};

/// The level in decibels a sine keeps after passing through `filter`, measured once settled.
fn sine_gain(filter: &mut dyn Effect, frequency: f32) -> f32 {
    let frames = seconds_to_frames(0.5);
    let input = sine(frequency, 0.5, frames);
    let mut audio = Stereo::mono(input.clone());
    Deck::new(120.0).play(filter, &mut audio);
    let settled = seconds_to_frames(0.25);
    decibels(rms(&audio.left[settled..]) / rms(&input[settled..]))
}

/// The response of a two-pole low-pass designed by the prewarped bilinear transform, which a
/// topology-preserving state-variable filter matches exactly.
fn bilinear_warp(frequency: f32, cutoff: f32) -> f32 {
    (PI * frequency / SAMPLE_RATE).tan() / (PI * cutoff / SAMPLE_RATE).tan()
}

fn flat_filter(mode: FilterMode, cutoff: f32, slope: FilterSlope) -> Filter {
    let mut filter = Filter::new(mode);
    filter.set_cutoff_hz(cutoff);
    filter.set_resonance(0.0);
    filter.set_slope(slope);
    filter.set_enabled(true);
    filter
}

#[test]
fn low_pass_at_1_khz_cuts_10_khz_by_the_expected_amount() {
    let warped = bilinear_warp(10_000.0, 1_000.0);
    let expected =
        decibels(1.0 / ((1.0 - warped * warped).powi(2) + (warped / FRAC_1_SQRT_2).powi(2)).sqrt());
    let mut filter = flat_filter(FilterMode::LowPass, 1_000.0, FilterSlope::TwelveDecibels);
    let measured = sine_gain(&mut filter, 10_000.0);
    assert!(
        (measured - expected).abs() < 0.5,
        "measured {measured} dB, expected {expected} dB"
    );
    assert!(measured < -40.0);
}

#[test]
fn twenty_four_decibel_low_pass_cuts_twice_as_deep() {
    let warped = bilinear_warp(10_000.0, 1_000.0);
    let expected = decibels(1.0 / (1.0 + warped.powi(8)).sqrt());
    let mut filter = flat_filter(
        FilterMode::LowPass,
        1_000.0,
        FilterSlope::TwentyFourDecibels,
    );
    let measured = sine_gain(&mut filter, 10_000.0);
    assert!(
        (measured - expected).abs() < 1.0,
        "measured {measured} dB, expected {expected} dB"
    );
}

#[test]
fn low_pass_leaves_the_passband_alone() {
    let mut filter = flat_filter(
        FilterMode::LowPass,
        1_000.0,
        FilterSlope::TwentyFourDecibels,
    );
    let measured = sine_gain(&mut filter, 100.0);
    assert!(measured.abs() < 0.1, "100 Hz moved by {measured} dB");
}

#[test]
fn high_pass_at_1_khz_cuts_100_hz_by_the_expected_amount() {
    let warped = bilinear_warp(100.0, 1_000.0);
    let squared = warped * warped;
    let expected =
        decibels(squared / ((1.0 - squared).powi(2) + (warped / FRAC_1_SQRT_2).powi(2)).sqrt());
    let mut filter = flat_filter(FilterMode::HighPass, 1_000.0, FilterSlope::TwelveDecibels);
    let measured = sine_gain(&mut filter, 100.0);
    assert!(
        (measured - expected).abs() < 0.5,
        "measured {measured} dB, expected {expected} dB"
    );
}

#[test]
fn band_pass_keeps_its_centre_and_cuts_both_sides() {
    let mut centre = flat_filter(FilterMode::BandPass, 1_000.0, FilterSlope::TwelveDecibels);
    let at_centre = sine_gain(&mut centre, 1_000.0);
    assert!(at_centre.abs() < 0.2, "centre moved by {at_centre} dB");

    let mut below = flat_filter(FilterMode::BandPass, 1_000.0, FilterSlope::TwelveDecibels);
    assert!(sine_gain(&mut below, 100.0) < -15.0);
    let mut above = flat_filter(FilterMode::BandPass, 1_000.0, FilterSlope::TwelveDecibels);
    assert!(sine_gain(&mut above, 10_000.0) < -15.0);
}

#[test]
fn resonance_lifts_the_cutoff() {
    let mut flat = flat_filter(FilterMode::LowPass, 1_000.0, FilterSlope::TwelveDecibels);
    let mut resonant = flat_filter(FilterMode::LowPass, 1_000.0, FilterSlope::TwelveDecibels);
    resonant.set_resonance(0.6);
    let lift = sine_gain(&mut resonant, 1_000.0) - sine_gain(&mut flat, 1_000.0);
    assert!(lift > 10.0, "resonance lifted the cutoff by only {lift} dB");
}

#[test]
fn sweeping_the_cutoff_and_mode_never_clicks() {
    let mut filter = Filter::new(FilterMode::LowPass);
    filter.set_cutoff_hz(200.0);
    filter.set_resonance(0.5);
    filter.set_enabled(true);
    let frames = seconds_to_frames(2.0);
    let mut audio = Stereo::mono(sine(110.0, 0.5, frames));
    let mut deck = Deck::new(120.0);
    let jump_at = seconds_to_frames(0.5);
    let mode_at = seconds_to_frames(1.0);
    deck.play_range(&mut filter, &mut audio, 0, jump_at);
    filter.set_cutoff_hz(12_000.0);
    deck.play_range(&mut filter, &mut audio, jump_at, mode_at);
    filter.set_mode(FilterMode::HighPass);
    filter.set_cutoff_hz(60.0);
    deck.play_range(&mut filter, &mut audio, mode_at, frames);

    let kink = largest_kink(&audio.left[seconds_to_frames(0.1)..]);
    assert!(kink < 0.005, "largest kink {kink}");
}

#[test]
fn sweep_filter_at_the_centre_passes_audio_bit_for_bit() {
    let mut sweep = SweepFilter::new();
    sweep.set_enabled(true);
    let input = common::noise(0.9, seconds_to_frames(1.0), 21);
    let mut audio = Stereo::mono(input.clone());
    Deck::new(120.0).play(&mut sweep, &mut audio);
    let settled = seconds_to_frames(0.1);
    assert_eq!(audio.left[settled..], input[settled..]);
    assert_eq!(audio.right[settled..], input[settled..]);
}

#[test]
fn sweep_filter_cuts_highs_to_the_left_and_lows_to_the_right() {
    let mut left_turn = SweepFilter::new();
    left_turn.set_position(-1.0);
    left_turn.set_enabled(true);
    let highs = sine_gain(&mut left_turn, 5_000.0);
    assert!(highs < -40.0, "fully left let 5 kHz through at {highs} dB");

    let mut right_turn = SweepFilter::new();
    right_turn.set_position(1.0);
    right_turn.set_enabled(true);
    let lows = sine_gain(&mut right_turn, 100.0);
    assert!(lows < -40.0, "fully right let 100 Hz through at {lows} dB");

    let mut slight = SweepFilter::new();
    slight.set_position(-0.2);
    slight.set_enabled(true);
    let bass = sine_gain(&mut slight, 100.0);
    assert!(
        bass.abs() < 0.5,
        "a slight low-pass moved the bass by {bass} dB"
    );
}

#[test]
fn sweeping_through_the_centre_never_clicks() {
    let mut sweep = SweepFilter::new();
    sweep.set_position(-0.8);
    sweep.set_enabled(true);
    let frames = seconds_to_frames(2.0);
    let mut audio = Stereo::mono(sine(220.0, 0.5, frames));
    let mut deck = Deck::new(120.0);
    let jump_at = seconds_to_frames(0.5);
    deck.play_range(&mut sweep, &mut audio, 0, jump_at);
    sweep.set_position(0.8);
    deck.play_range(&mut sweep, &mut audio, jump_at, frames);

    let kink = largest_kink(&audio.left[seconds_to_frames(0.1)..]);
    assert!(kink < 0.005, "largest kink {kink}");
}
