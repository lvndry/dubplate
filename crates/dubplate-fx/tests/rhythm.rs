//! Measures the gate, crusher, flanger, phaser, brake and backspin on rendered audio.

mod common;

use common::{Deck, Stereo, largest_kink, peak, rms, seconds_to_frames, sine};
use dubplate_fx::{
    Backspin, BeatLength, Brake, Crusher, Effect, EffectConfig, Flanger, Gate, Phaser,
};

#[test]
fn gate_opens_and_closes_on_the_beat_grid() {
    let mut gate = Gate::new();
    gate.set_beat(BeatLength::QUARTER);
    gate.set_length(0.5);
    gate.set_gate_depth(1.0);
    gate.set_enabled(true);
    let mut audio = Stereo::mono(vec![0.5; seconds_to_frames(1.0)]);
    Deck::new(120.0).play(&mut gate, &mut audio);

    let step = 6_000;
    let millisecond = 48;
    for step_index in 1..7 {
        let step_start = step_index * step;
        let closes_at = step_start + step / 2;
        assert!(
            (audio.left[step_start + 5 * millisecond] - 0.5).abs() < 1e-3,
            "open 5 ms into step {step_index}"
        );
        assert!(
            audio.left[closes_at + 25 * millisecond].abs() < 1e-3,
            "closed 25 ms after the half step in step {step_index}"
        );
        assert!(
            audio.left[closes_at - 1] > 0.49,
            "still open just before closing in step {step_index}"
        );
        assert!(
            audio.left[step_start - 1] < 0.01,
            "still closed just before step {step_index}"
        );
    }
}

#[test]
fn gate_pattern_mutes_the_chosen_steps() {
    let mut gate = Gate::new();
    gate.set_beat(BeatLength::QUARTER);
    gate.set_length(1.0);
    gate.set_pattern(0b0101);
    gate.set_enabled(true);
    let mut audio = Stereo::mono(vec![0.5; seconds_to_frames(1.0)]);
    Deck::new(120.0).play(&mut gate, &mut audio);
    assert!((audio.left[3_000] - 0.5).abs() < 1e-3, "step 0 plays");
    assert!(audio.left[9_000].abs() < 1e-3, "step 1 is muted");
    assert!((audio.left[15_000] - 0.5).abs() < 1e-3, "step 2 plays");
}

#[test]
fn gate_edges_are_soft_enough_not_to_click() {
    let mut gate = Gate::new();
    gate.set_beat(BeatLength::SIXTEENTH);
    gate.set_enabled(true);
    let mut audio = Stereo::mono(vec![1.0; seconds_to_frames(1.0)]);
    Deck::new(128.0).play(&mut gate, &mut audio);
    let largest_step = audio
        .left
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).abs())
        .fold(0.0, f32::max);
    assert!(
        largest_step < 0.06,
        "the gate moved {largest_step} in one sample"
    );
}

#[test]
fn crusher_reduces_bits_and_holds_samples() {
    let mut crusher = Crusher::new();
    crusher.set_bits(4.0);
    crusher.set_rate_hz(1_000.0);
    crusher.set_enabled(true);
    let mut audio = Stereo::mono(sine(97.0, 0.9, seconds_to_frames(0.5)));
    Deck::new(120.0).play(&mut crusher, &mut audio);

    let settled = &audio.left[seconds_to_frames(0.1)..];
    for sample in settled {
        let steps = sample * 8.0;
        assert!(
            (steps - steps.round()).abs() < 1e-4,
            "{sample} is not on a 4-bit step"
        );
    }
    let changes = settled.windows(2).filter(|pair| pair[0] != pair[1]).count();
    let expected = (settled.len() as f32 / 48.0) as usize;
    assert!(
        changes <= expected + 2,
        "{changes} changes for about {expected} held samples"
    );
}

/// Loops one beat of noise through a sweep of `sweep` beats and returns how much the output
/// differs, relative to its level, one beat later and two beats later.
fn sweep_repeat_gaps(mut effect: Box<dyn Effect>, sweep: BeatLength) -> (f32, f32) {
    let beat_frames = 24_000;
    let one_beat_of_noise = common::noise(0.5, beat_frames, 9);
    let looped: Vec<f32> = one_beat_of_noise
        .iter()
        .copied()
        .cycle()
        .take(beat_frames * 8)
        .collect();
    effect.set_beat(sweep);
    effect.set_enabled(true);
    let mut audio = Stereo::mono(looped);
    Deck::new(120.0).play(effect.as_mut(), &mut audio);

    let window = |start: usize| &audio.left[start..start + 6_000];
    let difference = |first: &[f32], second: &[f32]| {
        let gaps: Vec<f32> = first
            .iter()
            .zip(second)
            .map(|(one, other)| one - other)
            .collect();
        rms(&gaps) / rms(first)
    };
    (
        difference(window(beat_frames * 4), window(beat_frames * 5)),
        difference(window(beat_frames * 4), window(beat_frames * 6)),
    )
}

#[test]
fn flanger_and_phaser_sweep_in_time_with_the_beat() {
    let builders: [fn() -> Box<dyn Effect>; 2] = [
        || Box::new(Flanger::new(&EffectConfig::default())),
        || Box::new(Phaser::new()),
    ];
    for build in builders {
        let name = build().kind();
        let (one_beat_later, _) = sweep_repeat_gaps(build(), BeatLength::ONE);
        assert!(
            one_beat_later < 0.01,
            "{name}: a one-beat sweep should repeat every beat, differs by {one_beat_later}"
        );
        let (one_beat_later, two_beats_later) = sweep_repeat_gaps(build(), BeatLength::TWO);
        assert!(
            one_beat_later > 0.1,
            "{name}: a two-beat sweep should sound different a beat later, differs by {one_beat_later}"
        );
        assert!(
            two_beats_later < 0.01,
            "{name}: a two-beat sweep should repeat every two beats, differs by {two_beats_later}"
        );
    }
}

#[test]
fn brake_slows_the_music_to_a_stop_and_lets_it_back_in() {
    let mut brake = Brake::new(&EffectConfig::default());
    brake.set_beat(BeatLength::ONE);
    let frames = seconds_to_frames(2.0);
    let mut audio = Stereo::mono(sine(1_000.0, 0.5, frames));
    let mut deck = Deck::new(120.0);
    let trigger_at = seconds_to_frames(0.5);
    let release_at = seconds_to_frames(1.5);
    deck.play_range(&mut brake, &mut audio, 0, trigger_at);
    brake.set_enabled(true);
    deck.play_range(&mut brake, &mut audio, trigger_at, release_at);
    brake.set_enabled(false);
    deck.play_range(&mut brake, &mut audio, release_at, frames);

    let crossings = |range: std::ops::Range<usize>| {
        audio.left[range]
            .windows(2)
            .filter(|pair| pair[0] <= 0.0 && pair[1] > 0.0)
            .count()
    };
    let early = crossings(trigger_at..trigger_at + 4_800);
    let late = crossings(trigger_at + 14_400..trigger_at + 19_200);
    assert!(
        early >= 90,
        "{early} cycles in the first 100 ms of the brake"
    );
    assert!(
        late < early / 2,
        "pitch should drop: {late} cycles late against {early}"
    );
    let stopped = peak(&audio.left[trigger_at + 24_500..release_at]);
    assert!(stopped < 1e-6, "silent once stopped, got {stopped}");
    let back = rms(&audio.left[release_at + 2_400..]);
    assert!(
        (back - 0.5 / 2.0_f32.sqrt()).abs() < 0.01,
        "live music back at {back}"
    );
    let kink = largest_kink(&audio.left[release_at - 100..release_at + 2_400]);
    assert!(kink < 0.05, "release kink {kink}");
}

#[test]
fn backspin_plays_the_music_backwards_then_stops() {
    let mut backspin = Backspin::new(&EffectConfig::default());
    backspin.set_beat(BeatLength::TWO);
    let frames = seconds_to_frames(3.0);
    let ramp: Vec<f32> = (0..frames)
        .map(|frame| frame as f32 / frames as f32)
        .collect();
    let mut audio = Stereo::mono(ramp);
    let mut deck = Deck::new(120.0);
    let trigger_at = seconds_to_frames(1.5);
    deck.play_range(&mut backspin, &mut audio, 0, trigger_at);
    backspin.set_enabled(true);
    deck.play_range(&mut backspin, &mut audio, trigger_at, frames);

    let spin = seconds_to_frames(1.0);
    let readings: Vec<f32> = (15..45)
        .map(|percent| audio.left[trigger_at + spin * percent / 100])
        .collect();
    assert!(
        readings.windows(2).all(|pair| pair[1] < pair[0]),
        "the record should run backwards: {readings:?}"
    );
    assert!(readings[0] < 0.5, "the spin starts in the past");
    assert!(
        peak(&audio.left[trigger_at + spin + 100..]) < 1e-6,
        "silent after the spin"
    );
}
