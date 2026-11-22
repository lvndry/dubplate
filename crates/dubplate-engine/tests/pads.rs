//! Pad performance through the whole engine: rolls, slip, pad effects and key shift.

mod common;

use common::{SAMPLE_RATE, assert_close, left_channel, render_frames, sine_track};
use dubplate_engine::{
    BeatGrid, BeatLength, Controller, DeckId, EffectKind, MasterTempo, PadEffect, Renderer,
    SyncMode, offline_engine,
};

const BLOCK: usize = 256;

fn deck_on_grid(frequency: f64) -> (Controller, Renderer) {
    let (mut controller, renderer) = offline_engine(SAMPLE_RATE);
    controller
        .load_track(
            DeckId::A,
            sine_track(frequency, 0.5, 0.5, 60.0, SAMPLE_RATE),
        )
        .expect("load");
    controller
        .set_beat_grid(DeckId::A, Some(BeatGrid::new(120.0, 0.0).expect("grid")))
        .expect("grid");
    controller.set_crossfader(0.0).expect("crossfader");
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

fn frequency(signal: &[f32]) -> f64 {
    let crossings: Vec<usize> = signal
        .windows(2)
        .enumerate()
        .filter(|(_, pair)| pair[0] < 0.0 && pair[1] >= 0.0)
        .map(|(index, _)| index)
        .collect();
    (crossings.len() - 1) as f64 * f64::from(SAMPLE_RATE)
        / (crossings[crossings.len() - 1] - crossings[0]) as f64
}

fn rms(signal: &[f32]) -> f32 {
    (signal.iter().map(|sample| sample * sample).sum::<f32>() / signal.len() as f32).sqrt()
}

#[test]
fn a_roll_repeats_the_beat_and_lands_where_the_track_would_have_been() {
    let (mut controller, mut renderer) = deck_on_grid(440.0);
    controller.seek(DeckId::A, 10.1).unwrap();
    controller.play(DeckId::A).unwrap();
    seconds(&mut renderer, 0.2);
    let start = position(&controller);
    controller.beat_roll(DeckId::A, BeatLength::HALF).unwrap();
    seconds(&mut renderer, 1.5);
    let rolling = position(&controller);
    assert!((10.25..10.5).contains(&rolling), "rolling at {rolling}");
    let shadow = controller.slip_state(DeckId::A).shadow_seconds.unwrap();
    assert_close(shadow, start + 1.5, 0.01);
    controller.release_roll(DeckId::A).unwrap();
    seconds(&mut renderer, 0.5);
    assert_close(position(&controller), start + 2.0, 0.01);
    assert_eq!(controller.slip_state(DeckId::A).shadow_seconds, None);
    assert!(!controller.loop_state(DeckId::A).active);
}

#[test]
fn with_slip_on_a_scratch_and_a_loop_land_back_in_time() {
    let (mut controller, mut renderer) = deck_on_grid(440.0);
    controller.set_slip(DeckId::A, true).unwrap();
    controller.seek(DeckId::A, 5.0).unwrap();
    controller.play(DeckId::A).unwrap();
    seconds(&mut renderer, 0.1);
    let touched_at = position(&controller);
    controller.set_jog_touch(DeckId::A, true).unwrap();
    for _ in 0..20 {
        controller.jog_scratch(DeckId::A, -6).unwrap();
        seconds(&mut renderer, 0.05);
    }
    controller.set_jog_touch(DeckId::A, false).unwrap();
    seconds(&mut renderer, 0.5);
    assert_close(position(&controller), touched_at + 1.5, 0.02);
    assert!(controller.slip_state(DeckId::A).enabled);

    let looped_at = position(&controller);
    controller.beat_loop(DeckId::A, BeatLength::ONE).unwrap();
    seconds(&mut renderer, 3.0);
    controller.reloop_or_exit(DeckId::A).unwrap();
    seconds(&mut renderer, 0.5);
    assert_close(position(&controller), looped_at + 3.5, 0.02);
}

#[test]
fn a_held_echo_rings_out_after_release_and_a_brake_gives_the_live_track_back() {
    let (mut controller, mut renderer) = deck_on_grid(440.0);
    controller.play(DeckId::A).unwrap();
    seconds(&mut renderer, 0.5);
    let echo = PadEffect {
        kind: EffectKind::Echo,
        beat: BeatLength::HALF,
        depth: 0.8,
    };
    controller.hold_pad_effect(DeckId::A, echo).unwrap();
    assert_eq!(controller.pad_effect(DeckId::A), Some(echo));
    seconds(&mut renderer, 1.0);
    controller.release_pad_effect(DeckId::A).unwrap();
    assert_eq!(controller.pad_effect(DeckId::A), None);

    let started = position(&controller);
    controller
        .hold_pad_effect(
            DeckId::A,
            PadEffect {
                kind: EffectKind::Brake,
                beat: BeatLength::HALF,
                depth: 1.0,
            },
        )
        .unwrap();
    seconds(&mut renderer, 0.5);
    let braked = seconds(&mut renderer, 0.5);
    assert!(
        rms(&braked) < 0.05,
        "the brake stops the sound: {}",
        rms(&braked)
    );
    controller.release_pad_effect(DeckId::A).unwrap();
    let live = seconds(&mut renderer, 1.0);
    assert!(rms(&live[24_000..]) > 0.3, "the live track is back");
    assert_close(position(&controller), started + 2.0, 0.01);
}

#[test]
fn key_shift_moves_the_key_at_any_tempo() {
    let (mut controller, mut renderer) = deck_on_grid(500.0);
    controller.set_key_shift(DeckId::A, 12.0).unwrap();
    controller.play(DeckId::A).unwrap();
    let octave_up = seconds(&mut renderer, 2.0);
    let cents = 1_200.0 * (frequency(&octave_up[48_000..]) / 1_000.0).log2();
    assert!(cents.abs() < 3.0, "an octave up is {cents} cents off");
    assert_eq!(controller.key_shift(DeckId::A), 12.0);

    controller.set_key_shift(DeckId::A, 1.0).unwrap();
    controller.set_tempo(DeckId::A, 1.08).unwrap();
    seconds(&mut renderer, 0.5);
    let shifted = seconds(&mut renderer, 1.0);
    let expected = 500.0 * 1.08 * 2f64.powf(1.0 / 12.0);
    let measured = frequency(&shifted);
    let cents = 1_200.0 * (measured / expected).log2();
    assert!(
        cents.abs() < 3.0,
        "a semitone over the tempo's pitch is {cents} cents off"
    );
    assert!(controller.set_key_shift(DeckId::A, 13.0).is_err());
}

#[test]
fn beat_sync_leaves_a_rolling_deck_alone() {
    let (mut controller, mut renderer) = deck_on_grid(440.0);
    controller
        .load_track(DeckId::B, sine_track(330.0, 0.5, 0.5, 60.0, SAMPLE_RATE))
        .unwrap();
    controller
        .set_beat_grid(DeckId::B, Some(BeatGrid::new(124.0, 0.0).unwrap()))
        .unwrap();
    controller
        .set_master_tempo(MasterTempo::Deck(DeckId::B))
        .unwrap();
    controller.set_sync(DeckId::A, SyncMode::Beat).unwrap();
    controller.play(DeckId::B).unwrap();
    controller.play(DeckId::A).unwrap();
    seconds(&mut renderer, 1.0);
    controller
        .beat_roll(DeckId::A, BeatLength::QUARTER)
        .unwrap();
    seconds(&mut renderer, 0.01);
    let start = controller.loop_state(DeckId::A).start_seconds.unwrap();
    seconds(&mut renderer, 1.0);
    let now = position(&controller);
    assert!(
        now >= start && now < start + 0.13,
        "the roll holds at {now}, from {start}"
    );
    controller.release_roll(DeckId::A).unwrap();
    seconds(&mut renderer, 1.0);
    let beat = |deck: DeckId| {
        controller
            .beat_grid(deck)
            .unwrap()
            .beat_at(controller.deck_state(deck).position_seconds)
    };
    let difference = beat(DeckId::B) - beat(DeckId::A);
    assert!(
        (difference - difference.round()).abs() < 0.01,
        "back in phase after the roll"
    );
}
