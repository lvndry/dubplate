//! Measures the echo, ping-pong and reverb on rendered audio.

mod common;

use common::{Deck, Stereo, decibels, loudest_index, peak, rms, seconds_to_frames};
use dubplate_fx::{BeatLength, Echo, Effect, EffectConfig, PingPong, Reverb};

const IMPULSE_AT: usize = 4_800;

fn impulse(frames: usize, at: usize) -> Stereo {
    let mut audio = Stereo::silence(frames);
    audio.left[at] = 1.0;
    audio.right[at] = 1.0;
    audio
}

fn echo_with(feedback: f32) -> Echo {
    let mut echo = Echo::new(&EffectConfig::default());
    echo.set_beat(BeatLength::HALF);
    echo.set_feedback(feedback);
    echo.set_mix(0.5);
    echo.set_enabled(true);
    echo
}

#[test]
fn echo_repeats_half_a_beat_at_120_bpm_after_a_quarter_second() {
    let mut echo = echo_with(0.0);
    let mut audio = impulse(seconds_to_frames(1.0), IMPULSE_AT);
    Deck::new(120.0).play(&mut echo, &mut audio);

    let after_impulse = &audio.left[IMPULSE_AT + 1..];
    let repeat_at = IMPULSE_AT + 1 + loudest_index(after_impulse);
    let expected = IMPULSE_AT + seconds_to_frames(0.25);
    assert!(
        repeat_at.abs_diff(expected) <= 1,
        "first repeat at {repeat_at}, expected {expected}"
    );
    assert!((audio.left[repeat_at] - 1.0).abs() < 1e-3);
    assert_eq!(
        audio.left[IMPULSE_AT], 1.0,
        "the dry signal passes at full level"
    );
}

#[test]
fn echo_follows_a_tempo_change_live() {
    let mut echo = echo_with(0.0);
    let mut deck = Deck::new(120.0);
    let mut warm_up = Stereo::silence(seconds_to_frames(0.5));
    deck.play(&mut echo, &mut warm_up);

    deck.tempo_bpm = 128.0;
    let mut audio = impulse(seconds_to_frames(2.0), seconds_to_frames(1.0));
    deck.play(&mut echo, &mut audio);

    let impulse_at = seconds_to_frames(1.0);
    let repeat_at = impulse_at + 1 + loudest_index(&audio.left[impulse_at + 1..]);
    let expected = impulse_at + 11_250;
    assert!(
        repeat_at.abs_diff(expected) <= 1,
        "repeat at {repeat_at}, expected {expected} for half a beat at 128 BPM"
    );
}

#[test]
fn echo_keeps_its_tail_when_switched_off_and_takes_in_nothing_new() {
    let mut echo = echo_with(0.5);
    let mut deck = Deck::new(120.0);
    let frames = seconds_to_frames(1.0);
    let mut audio = impulse(frames, IMPULSE_AT);
    let late_impulse = IMPULSE_AT + seconds_to_frames(0.31);
    audio.left[late_impulse] = 1.0;
    audio.right[late_impulse] = 1.0;

    let switch_off_at = IMPULSE_AT + seconds_to_frames(0.1);
    deck.play_range(&mut echo, &mut audio, 0, switch_off_at);
    echo.set_enabled(false);
    deck.play_range(&mut echo, &mut audio, switch_off_at, frames);

    let mut always_on = echo_with(0.5);
    let mut reference = impulse(frames, IMPULSE_AT);
    Deck::new(120.0).play(&mut always_on, &mut reference);

    let first_repeat = IMPULSE_AT + seconds_to_frames(0.25);
    let third_repeat = IMPULSE_AT + seconds_to_frames(0.75);
    assert!(
        audio.left[first_repeat] > 0.95,
        "the first repeat rings out"
    );
    for sample_index in first_repeat - 8..third_repeat + 256 {
        if sample_index.abs_diff(late_impulse) < 2 {
            continue;
        }
        let difference = (audio.left[sample_index] - reference.left[sample_index]).abs();
        assert!(
            difference < 1e-6,
            "the tail differs from an echo left on by {difference} at sample {sample_index}"
        );
    }
    assert!(echo.is_active(), "the tail is still ringing");

    let blocked_repeat = late_impulse + seconds_to_frames(0.25);
    assert!(
        peak(&audio.left[blocked_repeat - 8..blocked_repeat + 8]) < 1e-3,
        "audio played after switching off must not be echoed"
    );
    assert_eq!(
        audio.left[late_impulse], 1.0,
        "the dry signal is back at full level"
    );
}

#[test]
fn a_finished_tail_returns_the_echo_to_an_exact_bypass() {
    let mut echo = echo_with(0.5);
    let mut deck = Deck::new(120.0);
    let mut audio = impulse(seconds_to_frames(0.2), IMPULSE_AT);
    deck.play(&mut echo, &mut audio);
    echo.set_enabled(false);
    let mut ring_out = Stereo::silence(seconds_to_frames(12.0));
    deck.play(&mut echo, &mut ring_out);
    assert!(!echo.is_active());

    let music = common::noise(0.8, 4_096, 7);
    let mut audio = Stereo::mono(music.clone());
    deck.play(&mut echo, &mut audio);
    assert_eq!(audio.left, music);
}

#[test]
fn a_new_beat_length_crossfades_without_a_click() {
    let mut echo = echo_with(0.6);
    echo.set_mix(0.5);
    let mut deck = Deck::new(124.0);
    let frames = seconds_to_frames(3.0);
    let mut audio = Stereo::mono(common::sine(220.0, 0.5, frames));
    let change_at = seconds_to_frames(1.5);
    deck.play_range(&mut echo, &mut audio, 0, change_at);
    echo.set_beat(BeatLength::THREE_QUARTERS);
    deck.play_range(&mut echo, &mut audio, change_at, frames);

    let around_change = &audio.left[change_at - 2_000..change_at + 4_000];
    let kink = common::largest_kink(around_change);
    assert!(kink < 0.01, "largest kink {kink} around the beat change");
}

#[test]
fn ping_pong_bounces_left_then_right() {
    let mut delay = PingPong::new(&EffectConfig::default());
    delay.set_beat(BeatLength::HALF);
    delay.set_feedback(0.5);
    delay.set_mix(0.5);
    delay.set_enabled(true);
    let mut audio = impulse(seconds_to_frames(1.0), IMPULSE_AT);
    Deck::new(120.0).play(&mut delay, &mut audio);

    let first = IMPULSE_AT + 12_000;
    let second = IMPULSE_AT + 24_000;
    assert!(audio.left[first] > 0.9, "first bounce on the left");
    assert!(
        audio.right[first].abs() < 1e-3,
        "first bounce not on the right"
    );
    assert!(audio.right[second] > 0.4, "second bounce on the right");
    assert!(
        audio.left[second].abs() < 1e-3,
        "second bounce not on the left"
    );
}

#[test]
fn reverb_decays_in_the_time_it_is_set_to() {
    let mut reverb = Reverb::new(&EffectConfig::default());
    reverb.set_decay_seconds(2.0);
    reverb.set_damping(0.0);
    reverb.set_low_cut_hz(20.0);
    reverb.set_pre_delay_seconds(0.0);
    reverb.set_mix(1.0);
    reverb.set_enabled(true);
    let mut audio = Stereo::mono(common::noise(0.5, seconds_to_frames(4.0), 3));
    for sample_index in seconds_to_frames(0.5)..audio.frames() {
        audio.left[sample_index] = 0.0;
        audio.right[sample_index] = 0.0;
    }
    Deck::new(120.0).play(&mut reverb, &mut audio);

    let window = seconds_to_frames(0.1);
    let level_at = |seconds: f32| {
        let start = seconds_to_frames(seconds);
        rms(&audio.left[start..start + window])
    };
    let drop = decibels(level_at(0.6)) - decibels(level_at(1.6));
    let measured_decay = 60.0 / drop;
    assert!(
        (1.6..=2.4).contains(&measured_decay),
        "measured a decay of {measured_decay} s for a 2 s setting"
    );
}

#[test]
fn reverb_rings_out_when_switched_off() {
    let mut reverb = Reverb::new(&EffectConfig::default());
    reverb.set_decay_seconds(3.0);
    reverb.set_mix(0.5);
    reverb.set_enabled(true);
    let mut deck = Deck::new(120.0);
    let mut audio = Stereo::mono(common::noise(0.5, seconds_to_frames(0.5), 11));
    deck.play(&mut reverb, &mut audio);
    reverb.set_enabled(false);
    let mut after = Stereo::silence(seconds_to_frames(1.0));
    deck.play(&mut reverb, &mut after);

    let tail = rms(&after.left[seconds_to_frames(0.2)..seconds_to_frames(0.4)]);
    assert!(decibels(tail) > -40.0, "tail level {} dB", decibels(tail));
    assert!(reverb.is_active());
}

#[test]
fn reverb_sits_under_the_dry_level_with_no_note_ringing_out() {
    let mut gains = Vec::new();
    for step in 0..16 {
        let frequency = 300.0 * 1.12_f32.powi(step);
        let mut reverb = Reverb::new(&EffectConfig::default());
        reverb.set_mix(1.0);
        reverb.set_enabled(true);
        let input = common::sine(frequency, 0.5, seconds_to_frames(3.0));
        let mut audio = Stereo::mono(input.clone());
        Deck::new(120.0).play(&mut reverb, &mut audio);
        let settled = seconds_to_frames(1.5);
        gains.push(decibels(
            rms(&audio.left[settled..]) / rms(&input[settled..]),
        ));
    }
    let mean = gains.iter().sum::<f32>() / gains.len() as f32;
    let loudest = gains.iter().copied().fold(f32::MIN, f32::max);
    assert!(
        (-7.0..=-1.0).contains(&mean),
        "mean wet level {mean} dB against the dry"
    );
    assert!(
        loudest < 0.0,
        "a held note rang out at {loudest} dB: {gains:?}"
    );
}
