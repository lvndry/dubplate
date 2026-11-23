//! Pad performance through the whole engine: rolls, slip, pad effects and key shift.

mod common;

use common::{SAMPLE_RATE, assert_close, left_channel, render_frames, sine_track};
use dubplate_engine::{
    BeatGrid, BeatLength, Controller, DeckId, EffectKind, MasterTempo, PadEffect, PadRelease,
    ParameterValue, Renderer, SyncMode, Track, offline_engine,
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
    assert!((10.0..10.25).contains(&rolling), "rolling at {rolling}");
    let shadow = controller.slip_state(DeckId::A).shadow_seconds.unwrap();
    assert_close(shadow, start + 1.5, 0.01);
    controller.release_roll(DeckId::A).unwrap();
    seconds(&mut renderer, 0.5);
    assert_close(position(&controller), start + 2.0, 0.01);
    assert_eq!(controller.slip_state(DeckId::A).shadow_seconds, None);
    assert!(!controller.loop_state(DeckId::A).active);
}

#[test]
fn a_short_roll_tap_changes_the_audio_before_the_first_wrap() {
    let audition = |quantize, has_grid: bool, roll| {
        let (mut controller, mut renderer) = deck_on_grid(443.0);
        if !has_grid {
            controller.set_beat_grid(DeckId::A, None).unwrap();
        }
        controller.set_quantize(quantize).unwrap();
        controller.seek(DeckId::A, 10.0).unwrap();
        controller.play(DeckId::A).unwrap();
        seconds(&mut renderer, 0.03);
        if let Some(length) = roll {
            controller.beat_roll(DeckId::A, length).unwrap();
        }
        let tapped = seconds(&mut renderer, 0.025);
        if roll.is_some() {
            controller.release_roll(DeckId::A).unwrap();
        }
        let released = seconds(&mut renderer, 0.025);
        assert_close(position(&controller), 10.08, 0.0001);
        (tapped, released)
    };
    let (dry_tap, dry_release) = audition(true, true, None);
    let settled = SAMPLE_RATE as usize / 100;
    let difference = |signal: &[f32], dry: &[f32]| {
        let differences: Vec<f32> = signal[settled..]
            .iter()
            .zip(&dry[settled..])
            .map(|(wet, dry)| wet - dry)
            .collect();
        rms(&differences)
    };
    for (quantize, has_grid, length) in [
        (true, true, BeatLength::HALF),
        (true, true, BeatLength::QUARTER),
        (true, true, BeatLength::EIGHTH),
        (true, true, BeatLength::ONE),
        (false, true, BeatLength::QUARTER),
        (true, false, BeatLength::QUARTER),
    ] {
        let (tapped, released) = audition(quantize, has_grid, Some(length));
        assert!(
            difference(&tapped, &dry_tap) > 0.2,
            "pressing a {length}-beat roll should repeat earlier audio with quantize={quantize}, grid={has_grid}"
        );
        assert!(
            difference(&released, &dry_release) < 0.001,
            "releasing a {length}-beat roll should restore live audio with quantize={quantize}, grid={has_grid}"
        );
    }
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
        parameter: Some(ParameterValue::Beats(BeatLength::HALF)),
        depth: 0.8,
        release: PadRelease::RingOut,
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
                parameter: Some(ParameterValue::Beats(BeatLength::HALF)),
                depth: 1.0,
                release: PadRelease::RingOut,
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

fn audition_echo_out(pad: Option<PadRelease>, hold_seconds: f64) -> Vec<f32> {
    let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
    let samples: Vec<f32> = (0..2 * SAMPLE_RATE)
        .flat_map(|frame| {
            let time = f64::from(frame) / f64::from(SAMPLE_RATE);
            let hz = if (0.58..0.70).contains(&time) {
                440.0
            } else if (0.80..0.95).contains(&time) {
                660.0
            } else if (1.20..1.29).contains(&time) {
                990.0
            } else {
                0.0
            };
            [0.4 * (std::f64::consts::TAU * time * hz).sin() as f32; 2]
        })
        .collect();
    let track =
        Track::from_interleaved_stereo(&samples, SAMPLE_RATE).expect("valid stereo test track");
    controller
        .load_track(DeckId::A, track)
        .expect("load test track");
    controller
        .set_beat_grid(
            DeckId::A,
            Some(BeatGrid::new(120.0, 0.0).expect("valid beat grid")),
        )
        .expect("assign beat grid");
    controller.set_crossfader(0.0).expect("set crossfader");
    controller.play(DeckId::A).expect("start deck");
    seconds(&mut renderer, 0.5);
    if let Some(release) = pad {
        controller
            .hold_pad_effect(
                DeckId::A,
                PadEffect {
                    kind: EffectKind::EchoOut,
                    parameter: Some(ParameterValue::Beats(BeatLength::ONE)),
                    depth: 0.8,
                    release,
                },
            )
            .expect("press echo out");
    }
    let mut output = seconds(&mut renderer, hold_seconds);
    if hold_seconds < 1.5 {
        if pad.is_some() {
            controller
                .release_pad_effect(DeckId::A)
                .expect("release echo out");
        }
        output.extend(seconds(&mut renderer, 1.5 - hold_seconds));
    }
    output
}

fn echo_out_window(signal: &[f32], start: f64, end: f64) -> &[f32] {
    let frame_at = |time: f64| (time * f64::from(SAMPLE_RATE)).round() as usize;
    &signal[frame_at(start)..frame_at(end)]
}

fn assert_echo_out_passes_live_note(live: &[f32], dry: &[f32]) {
    assert!(rms(dry) > 0.1);
    let difference: Vec<f32> = live.iter().zip(dry).map(|(live, dry)| live - dry).collect();
    assert!(
        rms(&difference) < 0.001,
        "the live note remains audible at its original level"
    );
}

#[test]
fn echo_out_plays_the_captured_beat_before_repeating_it() {
    let dry = audition_echo_out(None, 1.5);
    let echo = audition_echo_out(Some(PadRelease::RingOut), 1.5);
    for (start, end, hz) in [(0.11, 0.18, 440.0), (0.33, 0.42, 660.0)] {
        let original = echo_out_window(&dry, start, end);
        assert_echo_out_passes_live_note(echo_out_window(&echo, start, end), original);

        let first = echo_out_window(&echo, start + 0.5, end + 0.5);
        let second = echo_out_window(&echo, start + 1.0, end + 1.0);
        let dry_level = rms(original);
        let first_level = rms(first);
        let second_level = rms(second);
        assert!((0.5 * dry_level..dry_level).contains(&first_level));
        assert!((0.2 * first_level..0.9 * first_level).contains(&second_level));
        assert_close(frequency(first), hz, 2.0);
        assert_close(frequency(second), hz, 2.0);
    }
    assert!(rms(echo_out_window(&dry, 0.73, 0.78)) > 0.1);
    assert!(
        rms(echo_out_window(&echo, 0.73, 0.78)) < 0.005,
        "the new live note stays out of the repeated beat"
    );
}

#[test]
fn a_quick_echo_out_tap_leaves_the_live_music_audible_and_honors_release() {
    let dry = audition_echo_out(None, 0.1);
    let ring = audition_echo_out(Some(PadRelease::RingOut), 0.1);
    let cut = audition_echo_out(Some(PadRelease::Dry), 0.1);

    for (start, end) in [(0.09, 0.10), (0.115, 0.17), (0.33, 0.42), (0.73, 0.78)] {
        let original = echo_out_window(&dry, start, end);
        assert_echo_out_passes_live_note(echo_out_window(&ring, start, end), original);
        assert_echo_out_passes_live_note(echo_out_window(&cut, start, end), original);
    }
    let first = echo_out_window(&ring, 0.59, 0.605);
    assert!(
        rms(first) > 0.06,
        "the captured note still echoes after release"
    );
    assert_close(frequency(first), 440.0, 2.0);
    assert!(
        rms(echo_out_window(&cut, 0.59, 0.605)) < 0.005,
        "a dry release cuts the delayed note"
    );
}

#[test]
fn a_pad_set_to_go_back_to_dry_takes_its_echoes_out_when_it_is_let_go() {
    let mut tails = Vec::new();
    for release in [PadRelease::RingOut, PadRelease::Dry] {
        let (mut controller, mut renderer) = deck_on_grid(440.0);
        controller.play(DeckId::A).unwrap();
        seconds(&mut renderer, 0.5);
        let echo = PadEffect {
            kind: EffectKind::Echo,
            parameter: Some(ParameterValue::Beats(BeatLength::HALF)),
            depth: 0.8,
            release,
        };
        controller.hold_pad_effect(DeckId::A, echo).unwrap();
        seconds(&mut renderer, 1.0);
        controller.pause(DeckId::A).unwrap();
        controller.release_pad_effect(DeckId::A).unwrap();
        seconds(&mut renderer, 0.1);
        tails.push(rms(&seconds(&mut renderer, 0.4)));
    }
    let [ring_out, dry] = tails[..] else {
        unreachable!()
    };
    assert!(ring_out > 0.05, "the echoes ring out: {ring_out}");
    assert!(dry < 0.005, "back to dry leaves silence: {dry}");
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
