//! Each of rekordbox's Beat FX that Dubplate builds, checked for what defines it: a delay
//! repeats once on the beat, an Up Echo climbs, a Pan alternates, a Roll repeats what it
//! recorded, and so on.

mod common;

use std::f32::consts::TAU;

use common::{Deck, SAMPLE_RATE, Stereo, peak, rms, sine};
use dubplate_fx::{
    BeatLength, Effect, EffectConfig, EffectKind, ParameterValue, SignedBeats, create,
};

/// 120 BPM, so one beat is half a second.
const TEMPO_BPM: f64 = 120.0;
const BEAT_FRAMES: usize = 24_000;

fn effect(kind: EffectKind, depth: f32) -> Box<dyn Effect> {
    let mut effect = create(kind, &EffectConfig::default());
    effect.set_depth(depth);
    effect.set_enabled(true);
    effect
}

fn play(effect: &mut dyn Effect, audio: &mut Stereo) {
    Deck::new(TEMPO_BPM).play(effect, audio);
}

/// A short burst of a sine, faded in and out over a millisecond, starting at `at`.
fn burst(frequency: f32, frames: usize, at: usize, length: usize) -> Stereo {
    let mut audio = Stereo::silence(frames);
    let fade = 48;
    for offset in 0..length {
        let envelope = (offset.min(length - 1 - offset) as f32 / fade as f32).min(1.0);
        let value = 0.5 * envelope * (TAU * frequency * offset as f32 / SAMPLE_RATE).sin();
        audio.left[at + offset] = value;
        audio.right[at + offset] = value;
    }
    audio
}

/// The strength of `frequency` in `samples`, as the magnitude of one DFT bin.
fn strength(samples: &[f32], frequency: f32) -> f32 {
    let step = TAU * frequency / SAMPLE_RATE;
    let (real, imaginary) = samples.iter().enumerate().fold(
        (0.0_f64, 0.0_f64),
        |(real, imaginary), (index, sample)| {
            let angle = f64::from(step) * index as f64;
            (
                real + f64::from(*sample) * angle.cos(),
                imaginary + f64::from(*sample) * angle.sin(),
            )
        },
    );
    (real.hypot(imaginary) / samples.len().max(1) as f64) as f32
}

/// The frequency among `candidates` that is strongest in `samples`.
fn strongest(samples: &[f32], candidates: &[f32]) -> f32 {
    candidates
        .iter()
        .copied()
        .fold((0.0, 0.0), |(best, best_strength), candidate| {
            let candidate_strength = strength(samples, candidate);
            if candidate_strength > best_strength {
                (candidate, candidate_strength)
            } else {
                (best, best_strength)
            }
        })
        .0
}

fn semitones_from(frequency: f32, steps: f32) -> f32 {
    frequency * (steps / 12.0).exp2()
}

#[test]
fn delay_repeats_once_on_the_beat() {
    let mut delay = effect(EffectKind::Delay, 0.5);
    let mut audio = burst(1_000.0, BEAT_FRAMES * 3, 4_800, 2_400);
    play(delay.as_mut(), &mut audio);
    let half_beat = BEAT_FRAMES / 2;
    let first = rms(&audio.left[4_800 + half_beat..4_800 + half_beat + 2_400]);
    let second = rms(&audio.left[4_800 + 2 * half_beat..4_800 + 2 * half_beat + 2_400]);
    assert!(first > 0.1, "no repeat half a beat later: {first}");
    assert!(second < first * 0.01, "a second repeat at {second}");
}

#[test]
fn up_echo_climbs_and_down_echo_falls_with_every_repeat() {
    let frequency = 440.0;
    for (kind, direction) in [(EffectKind::UpEcho, 1.0), (EffectKind::DownEcho, -1.0)] {
        let mut echo = effect(kind, 0.5);
        echo.set_mix(1.0);
        let mut audio = burst(frequency, BEAT_FRAMES * 3, 0, 4_800);
        play(echo.as_mut(), &mut audio);
        let half_beat = BEAT_FRAMES / 2;
        let candidates: Vec<f32> = (-14..=14)
            .map(|step| semitones_from(frequency, step as f32))
            .collect();
        let first = strongest(&audio.left[half_beat + 600..half_beat + 4_200], &candidates);
        let second = strongest(
            &audio.left[2 * half_beat + 600..2 * half_beat + 4_200],
            &candidates,
        );
        assert!(
            (first - frequency) * direction > 0.0 && (second - first) * direction > 0.0,
            "{kind}: {frequency} Hz came back at {first} Hz, then {second} Hz"
        );
    }
}

#[test]
fn spiral_up_and_down_shift_their_repeats() {
    let frequency = 440.0;
    for (kind, direction) in [(EffectKind::SpiralUp, 1.0), (EffectKind::SpiralDown, -1.0)] {
        let mut spiral = effect(kind, 0.5);
        spiral.set_mix(1.0);
        let mut audio = burst(frequency, BEAT_FRAMES * 2, 0, 4_800);
        play(spiral.as_mut(), &mut audio);
        let half_beat = BEAT_FRAMES / 2;
        let candidates: Vec<f32> = (-6..=6)
            .map(|step| semitones_from(frequency, step as f32))
            .collect();
        let repeat = strongest(&audio.left[half_beat + 600..half_beat + 4_200], &candidates);
        assert!(
            (repeat - frequency) * direction > 0.0,
            "{kind}: {frequency} Hz came back at {repeat} Hz"
        );
    }
}

#[test]
fn spiral_bends_its_repeats_when_the_beat_changes() {
    let frequency = 440.0;
    let mut spiral = effect(EffectKind::Spiral, 1.0);
    spiral.set_mix(1.0);
    let frames = BEAT_FRAMES * 6;
    let mut audio = Stereo::mono(sine(frequency, 0.3, frames));
    for sample in audio.left[BEAT_FRAMES * 2..]
        .iter_mut()
        .chain(&mut audio.right[BEAT_FRAMES * 2..])
    {
        *sample = 0.0;
    }
    let mut deck = Deck::new(TEMPO_BPM);
    let change_at = BEAT_FRAMES * 3;
    deck.play_range(spiral.as_mut(), &mut audio, 0, change_at);
    spiral.set_beat(BeatLength::ONE);
    deck.play_range(spiral.as_mut(), &mut audio, change_at, frames);
    let candidates: Vec<f32> = (-12..=12)
        .map(|step| semitones_from(frequency, step as f32 * 0.5))
        .collect();
    let gliding = strongest(&audio.left[change_at + 600..change_at + 3_000], &candidates);
    assert!(
        gliding < frequency * 0.97,
        "the trail stayed at {gliding} Hz while the delay lengthened"
    );
}

#[test]
fn multi_tap_delay_repeats_every_eighth_of_a_beat_up_to_the_beat_length() {
    let mut delay = effect(EffectKind::MtDelay, 1.0);
    delay.set_beat(BeatLength::HALF);
    let mut audio = Stereo::silence(BEAT_FRAMES * 2);
    audio.left[1_000] = 0.5;
    audio.right[1_000] = 0.5;
    play(delay.as_mut(), &mut audio);
    let eighth = BEAT_FRAMES / 8;
    for tap in 1..=4 {
        let at = 1_000 + tap * eighth;
        assert!(
            peak(&audio.left[at - 2..at + 3]) > 0.1,
            "no repeat {tap} at {at}"
        );
    }
    let fifth = 1_000 + 5 * eighth;
    assert!(
        peak(&audio.left[fifth - 2..fifth + 3]) < 1e-3,
        "a repeat past the beat length"
    );
}

#[test]
fn rev_delay_plays_each_beat_length_backwards() {
    let mut delay = effect(EffectKind::RevDelay, 1.0);
    delay.set_beat(BeatLength::HALF);
    let frames = BEAT_FRAMES * 4;
    let ramp: Vec<f32> = (0..frames)
        .map(|frame| 0.5 * ((frame % (BEAT_FRAMES / 2)) as f32 / (BEAT_FRAMES / 2) as f32))
        .collect();
    let mut audio = Stereo::mono(ramp);
    play(delay.as_mut(), &mut audio);
    let window = &audio.left[BEAT_FRAMES * 2..BEAT_FRAMES * 3];
    let falling = window
        .windows(2)
        .filter(|pair| pair[1] < pair[0] - 1e-6)
        .count();
    let rising = window
        .windows(2)
        .filter(|pair| pair[1] > pair[0] + 1e-6)
        .count();
    assert!(
        falling > rising * 2,
        "rising ramps came back rising: {rising} rising, {falling} falling"
    );
}

#[test]
fn low_cut_echo_takes_the_lows_out_of_its_repeats() {
    let mut echo = effect(EffectKind::LowCutEcho, 0.5);
    echo.set_mix(1.0);
    let mut audio = burst(60.0, BEAT_FRAMES * 2, 0, 9_600);
    play(echo.as_mut(), &mut audio);
    let half_beat = BEAT_FRAMES / 2;
    let repeat = rms(&audio.left[half_beat + 1_000..half_beat + 8_600]);
    assert!(repeat < 0.05, "60 Hz came back at {repeat}");
}

#[test]
fn lpf_echo_darkens_and_hpf_echo_thins_as_the_knob_turns() {
    for (kind, kept, cut) in [
        (EffectKind::LpfEcho, 200.0, 6_000.0),
        (EffectKind::HpfEcho, 6_000.0, 200.0),
    ] {
        let mut echo = effect(kind, 1.0);
        let frames = BEAT_FRAMES * 2;
        let mixed: Vec<f32> = sine(kept, 0.25, frames)
            .iter()
            .zip(sine(cut, 0.25, frames))
            .map(|(kept_sample, cut_sample)| kept_sample + cut_sample)
            .collect();
        let mut audio = Stereo::mono(mixed.clone());
        for sample in audio.left[BEAT_FRAMES / 2..]
            .iter_mut()
            .chain(&mut audio.right[BEAT_FRAMES / 2..])
        {
            *sample = 0.0;
        }
        play(echo.as_mut(), &mut audio);
        let tail = &audio.left[BEAT_FRAMES / 2 + 2_000..BEAT_FRAMES];
        assert!(
            strength(tail, kept) > 4.0 * strength(tail, cut),
            "{kind} kept {} of {kept} Hz and {} of {cut} Hz",
            strength(tail, kept),
            strength(tail, cut)
        );
    }
}

#[test]
fn crush_echo_crushes_its_repeats() {
    let mut echo = effect(EffectKind::CrushEcho, 1.0);
    echo.set_mix(1.0);
    let mut audio = burst(220.0, BEAT_FRAMES * 2, 0, 9_600);
    play(echo.as_mut(), &mut audio);
    let half_beat = BEAT_FRAMES / 2;
    let repeat = &audio.left[half_beat + 1_000..half_beat + 8_000];
    let mut levels: Vec<i32> = repeat
        .iter()
        .map(|sample| (sample * 10_000.0).round() as i32)
        .collect();
    levels.sort_unstable();
    levels.dedup();
    assert!(
        levels.len() < 40,
        "the repeat held {} levels, not a crushed few",
        levels.len()
    );
}

#[test]
fn noise_rises_over_silence() {
    let mut noise = effect(EffectKind::Noise, 1.0);
    let mut audio = Stereo::silence(BEAT_FRAMES * 2);
    play(noise.as_mut(), &mut audio);
    assert!(rms(&audio.left[BEAT_FRAMES..]) > 0.01, "no noise");
}

#[test]
fn noise_ring_out_plays_from_the_delay_then_goes_idle() {
    let mut noise = effect(EffectKind::Noise, 0.85);
    let mut deck = Deck::new(TEMPO_BPM);
    let mut pressed = Stereo::silence(BEAT_FRAMES / 4);
    deck.play(noise.as_mut(), &mut pressed);
    assert!(
        peak(&pressed.left) < 1e-6,
        "noise reached the output before the delay"
    );

    noise.set_enabled(false);
    let mut released = Stereo::silence(BEAT_FRAMES * 16);
    deck.play(noise.as_mut(), &mut released);
    assert!(
        rms(&released.left[BEAT_FRAMES / 4..BEAT_FRAMES / 2]) > 0.01,
        "the recorded noise did not ring out"
    );
    assert!(
        peak(&released.left[BEAT_FRAMES * 15..]) < 1e-5,
        "the noise kept playing after release"
    );
    assert!(!noise.is_active(), "the noise tail never went idle");
}

#[test]
fn noise_back_to_dry_mutes_and_goes_idle() {
    let mut noise = effect(EffectKind::Noise, 0.85);
    let mut deck = Deck::new(TEMPO_BPM);
    let mut pressed = Stereo::silence(BEAT_FRAMES * 2);
    deck.play(noise.as_mut(), &mut pressed);
    assert!(rms(&pressed.left[BEAT_FRAMES..]) > 0.01);

    noise.switch_off_dry();
    noise.set_depth(1.0);
    let mut released = Stereo::silence(BEAT_FRAMES * 16);
    deck.play(noise.as_mut(), &mut released);
    assert!(
        peak(&released.left[BEAT_FRAMES / 4..]) < 1e-5,
        "the noise was still audible after switching to dry"
    );
    assert!(!noise.is_active(), "the muted noise tail never went idle");
}

#[test]
fn helix_fully_right_holds_its_loop_without_fading() {
    let mut helix = effect(EffectKind::Helix, 1.0);
    let frames = BEAT_FRAMES * 12;
    // 80 cycles fit the half-beat loop, so every pass lands in phase with the last.
    let mut audio = Stereo::mono(sine(320.0, 0.3, frames));
    let mut deck = Deck::new(TEMPO_BPM);
    let freeze_at = BEAT_FRAMES * 2;
    helix.set_depth(0.5);
    deck.play_range(helix.as_mut(), &mut audio, 0, freeze_at);
    helix.set_depth(1.0);
    for sample in audio.left[freeze_at..]
        .iter_mut()
        .chain(&mut audio.right[freeze_at..])
    {
        *sample = 0.0;
    }
    deck.play_range(helix.as_mut(), &mut audio, freeze_at, frames);
    let early = rms(&audio.left[BEAT_FRAMES * 4..BEAT_FRAMES * 5]);
    let late = rms(&audio.left[BEAT_FRAMES * 11..BEAT_FRAMES * 12]);
    let before = rms(&audio.left[BEAT_FRAMES..BEAT_FRAMES * 2]);
    assert!(
        early > 0.05,
        "the frozen loop is silent: {early}, {before} before freezing"
    );
    assert!(
        late > early * 0.9,
        "the frozen loop faded from {early} to {late}"
    );
}

#[test]
fn reverb_up_and_down_shift_their_tails_an_octave() {
    let frequency = 400.0;
    for (kind, shifted) in [
        (EffectKind::ReverbUp, frequency * 2.0),
        (EffectKind::ReverbDown, frequency / 2.0),
    ] {
        let mut reverb = effect(kind, 1.0);
        let mut audio = burst(frequency, BEAT_FRAMES * 4, 0, BEAT_FRAMES);
        play(reverb.as_mut(), &mut audio);
        let tail = &audio.left[BEAT_FRAMES * 2..BEAT_FRAMES * 3];
        assert!(
            strength(tail, shifted) > 0.0005,
            "{kind} left no tail at {shifted} Hz: {}",
            strength(tail, shifted)
        );
    }
}

#[test]
fn pan_alternates_left_and_right_on_the_beat() {
    let mut pan = effect(EffectKind::Pan, 1.0);
    pan.set_beat(BeatLength::ONE);
    let mut audio = Stereo::mono(vec![0.5; BEAT_FRAMES * 4]);
    play(pan.as_mut(), &mut audio);
    for beat in 0..4 {
        let middle = beat * BEAT_FRAMES + BEAT_FRAMES / 2;
        let left = audio.left[middle].abs();
        let right = audio.right[middle].abs();
        let (louder, quieter) = if beat % 2 == 0 {
            (left, right)
        } else {
            (right, left)
        };
        assert!(
            louder > 0.5 && quieter < 0.05,
            "beat {beat}: left {left}, right {right}"
        );
    }
}

#[test]
fn rolls_repeat_what_they_recorded_and_rev_roll_plays_it_backwards() {
    let frames = BEAT_FRAMES * 3;
    let input = common::noise(0.5, frames, 3);
    let length = BEAT_FRAMES / 2;
    for kind in [EffectKind::Roll, EffectKind::SlipRoll, EffectKind::RevRoll] {
        let mut roll = effect(kind, 1.0);
        roll.set_beat(BeatLength::HALF);
        let mut audio = Stereo::mono(input.clone());
        play(roll.as_mut(), &mut audio);
        let check = |offset: usize| {
            let recorded = match kind {
                EffectKind::RevRoll => input[length - 1 - offset],
                _ => input[offset],
            };
            (audio.left[2 * length + offset] - recorded).abs() < 1e-4
        };
        let matching = (200..length - 200)
            .step_by(97)
            .filter(|offset| check(*offset))
            .count();
        let checked = (200..length - 200).step_by(97).count();
        assert_eq!(matching, checked, "{kind} did not repeat its recording");
    }
}

#[test]
fn slip_roll_records_again_when_its_length_changes() {
    let frames = BEAT_FRAMES * 4;
    let input = common::noise(0.5, frames, 9);
    let mut roll = effect(EffectKind::SlipRoll, 1.0);
    roll.set_beat(BeatLength::HALF);
    let mut audio = Stereo::mono(input.clone());
    let mut deck = Deck::new(TEMPO_BPM);
    let change_at = BEAT_FRAMES * 2;
    deck.play_range(roll.as_mut(), &mut audio, 0, change_at);
    roll.set_beat(BeatLength::QUARTER);
    deck.play_range(roll.as_mut(), &mut audio, change_at, frames);
    let quarter = BEAT_FRAMES / 4;
    let offset = quarter / 2;
    let repeat_at = change_at + 2 * quarter + offset;
    assert!(
        (audio.left[repeat_at] - input[change_at + offset]).abs() < 1e-4,
        "the slip roll kept its first recording"
    );
}

#[test]
fn pitch_shifts_by_its_parameter_at_full_depth() {
    let frequency = 440.0;
    let mut pitch = effect(EffectKind::Pitch, 1.0);
    pitch.set_parameter(ParameterValue::Semitones(7));
    let mut audio = Stereo::mono(sine(frequency, 0.4, BEAT_FRAMES * 2));
    play(pitch.as_mut(), &mut audio);
    let candidates: Vec<f32> = (-12..=12)
        .map(|step| semitones_from(frequency, step as f32))
        .collect();
    let heard = strongest(&audio.left[BEAT_FRAMES..], &candidates);
    let expected = semitones_from(frequency, 7.0);
    assert!(
        (heard - expected).abs() < 1.0,
        "a fifth up came out at {heard} Hz instead of {expected} Hz"
    );
}

#[test]
fn robot_rings_at_its_tone() {
    let mut robot = effect(EffectKind::Robot, 1.0);
    robot.set_parameter(ParameterValue::Percent(0));
    let mut audio = Stereo::mono(common::noise(0.3, BEAT_FRAMES * 2, 5));
    play(robot.as_mut(), &mut audio);
    let output = &audio.left[BEAT_FRAMES..];
    let at_tone = strength(output, 150.0) + strength(output, 300.0);
    let between = strength(output, 225.0) + strength(output, 375.0);
    assert!(
        at_tone > between * 3.0,
        "the robot does not ring at 150 Hz: {at_tone} against {between}"
    );
}

#[test]
fn mobius_sounds_over_silence_and_enigma_jet_moves_the_sound() {
    for kind in [EffectKind::MobiusSaw, EffectKind::MobiusTriangle] {
        let mut mobius = effect(kind, 1.0);
        let mut audio = Stereo::silence(BEAT_FRAMES * 2);
        play(mobius.as_mut(), &mut audio);
        assert!(rms(&audio.left[BEAT_FRAMES..]) > 0.02, "{kind} is silent");
        assert!(peak(&audio.left) < 1.0, "{kind} clips");
    }
    for falling in [false, true] {
        let mut jet = effect(EffectKind::EnigmaJet, 1.0);
        let cycle = SignedBeats {
            length: BeatLength::ONE,
            falling,
        };
        jet.set_parameter(ParameterValue::SignedBeats(cycle));
        let input = common::noise(0.3, BEAT_FRAMES * 3, 8);
        let mut audio = Stereo::mono(input.clone());
        play(jet.as_mut(), &mut audio);
        let changed: f32 = audio.left[BEAT_FRAMES..]
            .iter()
            .zip(&input[BEAT_FRAMES..])
            .map(|(output, dry)| (output - dry).abs())
            .sum::<f32>()
            / (BEAT_FRAMES * 2) as f32;
        assert!(
            changed > 0.02,
            "the enigma jet barely changed the noise: {changed}"
        );
    }
}

#[test]
fn vinyl_brake_slows_the_music_as_far_as_the_knob_turns() {
    let frequency = 440.0;
    let mut brake = effect(EffectKind::VinylBrake, 0.5);
    brake.set_beat(BeatLength::QUARTER);
    let mut audio = Stereo::mono(sine(frequency, 0.4, BEAT_FRAMES * 3));
    play(brake.as_mut(), &mut audio);
    let candidates: Vec<f32> = (-14..=2)
        .map(|step| semitones_from(frequency, step as f32))
        .collect();
    let heard = strongest(&audio.left[BEAT_FRAMES..BEAT_FRAMES * 2], &candidates);
    assert!(
        (heard - frequency / 2.0).abs() < 15.0,
        "half way the record played at {heard} Hz instead of {} Hz",
        frequency / 2.0
    );

    let mut stop = effect(EffectKind::VinylBrake, 1.0);
    stop.set_beat(BeatLength::QUARTER);
    let mut audio = Stereo::mono(sine(frequency, 0.4, BEAT_FRAMES * 3));
    play(stop.as_mut(), &mut audio);
    assert!(
        rms(&audio.left[BEAT_FRAMES * 2..]) < 0.01,
        "fully right the record did not stop"
    );
}

#[test]
fn stretch_slows_the_music_and_keeps_its_pitch() {
    let frequency = 440.0;
    let mut stretch = effect(EffectKind::Stretch, 1.0);
    stretch.set_beat(BeatLength::TWO);
    let input = sine(frequency, 0.4, BEAT_FRAMES * 3);
    let mut audio = Stereo::mono(input.clone());
    play(stretch.as_mut(), &mut audio);
    let output = &audio.left[BEAT_FRAMES / 2..BEAT_FRAMES * 2];
    let candidates: Vec<f32> = (-12..=12)
        .map(|step| semitones_from(frequency, step as f32))
        .collect();
    let heard = strongest(output, &candidates);
    assert!(
        (heard - frequency).abs() < 1.0,
        "the stretch moved the pitch to {heard} Hz"
    );
}
