//! Offline regressions for effect timing, pitch controls and spectral state.

mod common;

use std::f64::consts::TAU;

use common::{Deck, SAMPLE_RATE, Stereo, largest_kink, rms, sine};
use dubplate_fx::{
    BeatLength, Effect, EffectConfig, EffectKind, ParameterValue, ProcessContext, SignedBeats,
    create,
};

const TEMPO_BPM: f64 = 120.0;
const BEAT_FRAMES: usize = 24_000;

fn enabled(kind: EffectKind, parameter: ParameterValue) -> Box<dyn Effect> {
    let mut effect = create(kind, &EffectConfig::default());
    effect.set_depth(1.0);
    effect.set_parameter(parameter);
    effect.set_enabled(true);
    effect
}

#[test]
fn mobius_oscillators_keep_their_phase_when_voices_wrap_in_either_direction() {
    let voice_frames = BEAT_FRAMES / 16;
    // Sawtooth edges naturally produce a larger slope change than triangle peaks.
    for (kind, max_kink) in [
        (EffectKind::MobiusSaw, 0.2),
        (EffectKind::MobiusTriangle, 0.05),
    ] {
        for cycle in [
            SignedBeats::rising(BeatLength::SIXTEENTH),
            SignedBeats::falling(BeatLength::SIXTEENTH),
        ] {
            let mut effect = enabled(kind, ParameterValue::SignedBeats(cycle));
            let mut audio = Stereo::silence(voice_frames * 15);
            Deck::new(TEMPO_BPM).play(effect.as_mut(), &mut audio);
            for voice in 2..15 {
                let boundary = voice * voice_frames;
                let kink = largest_kink(&audio.left[boundary - 2..boundary + 3]);
                assert!(
                    kink < max_kink,
                    "{kind} {cycle:?} voice {voice} discontinuity: {kink}"
                );
            }
        }
    }
}

fn tone_strength(samples: &[f32], frequency: f64) -> f64 {
    let step = frequency * TAU / f64::from(SAMPLE_RATE);
    let (real, imaginary) =
        samples
            .iter()
            .enumerate()
            .fold((0.0, 0.0), |(real, imaginary), (frame, &sample)| {
                let (sine, cosine) = (step * frame as f64).sin_cos();
                (
                    real + cosine * f64::from(sample),
                    imaginary + sine * f64::from(sample),
                )
            });
    real.hypot(imaginary) / samples.len() as f64
}

#[test]
fn mobius_moves_an_octave_per_signed_beat_length() {
    let centre_tone = 440.0;
    let amount = 0.25_f64; // A quarter octave after one of the configured four beats.
    for kind in [EffectKind::MobiusSaw, EffectKind::MobiusTriangle] {
        for (cycle, direction) in [
            (SignedBeats::rising(BeatLength::FOUR), 1.0),
            (SignedBeats::falling(BeatLength::FOUR), -1.0),
        ] {
            let mut mobius = enabled(kind, ParameterValue::SignedBeats(cycle));
            let mut audio = Stereo::silence(BEAT_FRAMES * 2);
            Deck::new(TEMPO_BPM).play(mobius.as_mut(), &mut audio);
            let window = &audio.left[BEAT_FRAMES - 1_500..BEAT_FRAMES + 1_500];
            let expected = tone_strength(window, centre_tone * (amount * direction).exp2());
            let too_slow = tone_strength(window, centre_tone * (amount * direction / 6.0).exp2());
            assert!(
                expected > 2.0 * too_slow,
                "{kind} {cycle:?} missed the expected pitch: {expected} vs {too_slow}"
            );
        }
    }
}

#[test]
fn enigma_jet_moves_one_voice_per_signed_beat_length() {
    let voice_frames = BEAT_FRAMES / 4;
    let render = |cycle| {
        let mut effect = enabled(EffectKind::EnigmaJet, ParameterValue::SignedBeats(cycle));
        // The tone completes an integer number of oscillations within every voice period.
        let mut audio = Stereo::mono(sine(800.0, 0.3, voice_frames * 8));
        Deck::new(TEMPO_BPM).play(effect.as_mut(), &mut audio);
        audio.left
    };
    let rising = render(SignedBeats::rising(BeatLength::QUARTER));
    let falling = render(SignedBeats::falling(BeatLength::QUARTER));
    for samples in [&rising, &falling] {
        let residual: Vec<f32> = samples[voice_frames * 5..voice_frames * 6]
            .iter()
            .zip(&samples[voice_frames * 6..voice_frames * 7])
            .map(|(first, second)| first - second)
            .collect();
        assert!(
            rms(&residual) < 0.0002,
            "one voice did not repeat after 1/4 beat: {}",
            rms(&residual)
        );
    }
    let direction_difference: Vec<f32> = rising[voice_frames * 5..voice_frames * 6]
        .iter()
        .zip(&falling[voice_frames * 5..voice_frames * 6])
        .map(|(up, down)| up - down)
        .collect();
    assert!(rms(&direction_difference) > 0.01);
}

#[test]
fn zero_semitones_preserve_the_live_signal_instead_of_delaying_it() {
    let mut pitch = enabled(EffectKind::Pitch, ParameterValue::Semitones(0));
    let dry = sine(419.0, 0.3, BEAT_FRAMES);
    let mut audio = Stereo::mono(dry.clone());
    Deck::new(TEMPO_BPM).play(pitch.as_mut(), &mut audio);
    let error = audio.left[BEAT_FRAMES / 2..]
        .iter()
        .zip(&dry[BEAT_FRAMES / 2..])
        .map(|(output, input)| (output - input).abs())
        .fold(0.0, f32::max);
    assert_eq!(error, 0.0, "neutral pitch changed the dry signal");
}

#[test]
fn robot_saturates_out_of_range_tones_at_the_supported_endpoints() {
    for (invalid, valid) in [(i16::MIN, -100), (i16::MAX, 100)] {
        let mut clamped = enabled(EffectKind::Robot, ParameterValue::Percent(invalid));
        let mut endpoint = enabled(EffectKind::Robot, ParameterValue::Percent(valid));
        assert_eq!(clamped.parameter(), Some(ParameterValue::Percent(valid)));
        let mut actual = Stereo::mono(sine(351.0, 0.4, BEAT_FRAMES));
        let mut expected = actual.clone();
        Deck::new(TEMPO_BPM).play(clamped.as_mut(), &mut actual);
        Deck::new(TEMPO_BPM).play(endpoint.as_mut(), &mut expected);
        assert_eq!(actual.left, expected.left);
    }
}

// The adjacent-sample difference is a simple high-frequency measure for broadband noise.
fn roughness(samples: &[f32]) -> f32 {
    let differences: Vec<f32> = samples
        .windows(2)
        .map(|sample| sample[1] - sample[0])
        .collect();
    rms(&differences) / rms(samples)
}

#[test]
fn noise_builds_the_current_band_after_reset() {
    let measure = |depth| {
        let mut noise = enabled(
            EffectKind::Noise,
            ParameterValue::Beats(BeatLength::QUARTER),
        );
        noise.set_depth(depth);
        noise.reset();
        let mut audio = Stereo::silence(BEAT_FRAMES * 4);
        Deck::new(TEMPO_BPM).play(noise.as_mut(), &mut audio);
        roughness(&audio.left[BEAT_FRAMES * 2..])
    };
    let lower_band = measure(0.2);
    let higher_band = measure(1.0);
    assert!(
        higher_band > 2.0 * lower_band,
        "noise did not move up from its lower band: {lower_band} vs {higher_band}"
    );
}

#[test]
fn noise_recomputes_its_filter_when_the_sample_rate_changes() {
    let mut effect = enabled(
        EffectKind::Noise,
        ParameterValue::Beats(BeatLength::QUARTER),
    );
    let mut initial = Stereo::silence(BEAT_FRAMES * 4);
    let mut deck = Deck::new(TEMPO_BPM);
    deck.play(effect.as_mut(), &mut initial);

    // At twice the sample rate, the same physical band changes more slowly per sample.
    let mut higher_rate = Stereo::silence(BEAT_FRAMES * 8);
    effect.process(
        &mut higher_rate.left,
        &mut higher_rate.right,
        &ProcessContext::new(2.0 * SAMPLE_RATE, TEMPO_BPM, deck.beat_position),
    );
    let before = roughness(&initial.left[BEAT_FRAMES * 2..]);
    let after = roughness(&higher_rate.left[BEAT_FRAMES * 4..]);
    assert!(
        after < 0.8 * before,
        "noise shifted its physical band with the sample rate: {before} vs {after}"
    );
}
