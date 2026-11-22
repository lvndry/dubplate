//! Drives every effect to its limits and checks the output stays safe and click-free.

mod common;

use common::{Deck, Stereo, largest_kink, peak, seconds_to_frames, sine};
use dubplate_fx::{
    Backspin, BeatFilter, BeatLength, Brake, Crusher, Echo, Effect, EffectConfig, EffectKind, Filter,
    FilterMode, FilterSlope, Flanger, Gate, Phaser, PingPong, Reverb, create,
};

const OUTPUT_CEILING: f32 = 4.0;

/// Every effect with every control turned as far as it goes.
fn at_maximum(kind: EffectKind) -> Box<dyn Effect> {
    let config = EffectConfig::default();
    let mut effect: Box<dyn Effect> = match kind {
        EffectKind::Echo => {
            let mut echo = Echo::new(&config);
            echo.set_feedback(1.0);
            Box::new(echo)
        }
        EffectKind::PingPong => {
            let mut delay = PingPong::new(&config);
            delay.set_feedback(1.0);
            Box::new(delay)
        }
        EffectKind::Reverb => {
            let mut reverb = Reverb::new(&config);
            reverb.set_decay_seconds(20.0);
            reverb.set_size(1.0);
            reverb.set_damping(0.0);
            reverb.set_low_cut_hz(20.0);
            Box::new(reverb)
        }
        EffectKind::LowPass | EffectKind::HighPass | EffectKind::BandPass => {
            let mode = match kind {
                EffectKind::LowPass => FilterMode::LowPass,
                EffectKind::HighPass => FilterMode::HighPass,
                _ => FilterMode::BandPass,
            };
            let mut filter = Filter::new(mode);
            filter.set_resonance(1.0);
            filter.set_slope(FilterSlope::TwentyFourDecibels);
            filter.set_cutoff_hz(55.0);
            Box::new(filter)
        }
        EffectKind::Filter => {
            let mut filter = BeatFilter::new();
            filter.set_depth(1.0);
            filter.set_beat(BeatLength::QUARTER);
            Box::new(filter)
        }
        EffectKind::Flanger => {
            let mut flanger = Flanger::new(&config);
            flanger.set_feedback(0.95);
            Box::new(flanger)
        }
        EffectKind::Phaser => {
            let mut phaser = Phaser::new();
            phaser.set_feedback(0.9);
            Box::new(phaser)
        }
        EffectKind::Gate => Box::new(Gate::new()),
        EffectKind::Crusher => {
            let mut crusher = Crusher::new();
            crusher.set_bits(1.0);
            crusher.set_rate_hz(400.0);
            Box::new(crusher)
        }
        EffectKind::Brake => Box::new(Brake::new(&config)),
        EffectKind::Backspin => Box::new(Backspin::new(&config)),
    };
    effect.set_mix(1.0);
    if effect.beat().is_some() {
        effect.set_beat(BeatLength::SIXTEENTH);
    }
    effect.set_enabled(true);
    effect
}

/// Full-scale square waves and noise, the harshest thing a deck can send, with a few absurdly
/// loud samples such as a broken decoder might produce.
fn full_scale(frames: usize) -> Stereo {
    let noise = common::noise(1.0, frames, 13);
    let left = (0..frames)
        .map(|frame| if (frame / 400) % 2 == 0 { 1.0 } else { -1.0 })
        .collect();
    let right = noise
        .iter()
        .enumerate()
        .map(|(frame, sample)| {
            if frame % 96_000 < 48_000 {
                sample.signum()
            } else {
                *sample
            }
        })
        .collect();
    let mut audio = Stereo { left, right };
    for spike in [frames / 3, frames / 2, frames * 2 / 3] {
        audio.left[spike] = f32::MAX;
        audio.right[spike + 1] = -f32::MAX;
    }
    audio
}

#[test]
fn every_effect_stays_finite_and_bounded_at_maximum_settings() {
    for kind in EffectKind::ALL {
        let mut effect = at_maximum(kind);
        let mut audio = full_scale(seconds_to_frames(6.0));
        Deck::new(174.0).play(effect.as_mut(), &mut audio);
        let loudest = peak(&audio.left).max(peak(&audio.right));
        assert!(
            audio
                .left
                .iter()
                .chain(&audio.right)
                .all(|sample| sample.is_finite()),
            "{kind} produced NaN or infinity"
        );
        assert!(loudest <= OUTPUT_CEILING, "{kind} peaked at {loudest}");
    }
}

/// Plays one second of a clean sine, then one second of a sine with NaN and infinity in it,
/// then one more clean second, and returns the last second.
fn play_through_bad_samples(kind: EffectKind, poison: bool) -> Stereo {
    let mut effect = create(kind, &EffectConfig::default());
    effect.set_depth(0.8);
    effect.set_enabled(true);
    let frames = seconds_to_frames(1.0);
    let mut deck = Deck::new(126.0);
    let mut audio = Stereo::mono(sine(330.0, 0.5, frames));
    if poison {
        for (offset, bad) in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY]
            .into_iter()
            .enumerate()
        {
            audio.left[frames / 2 + offset * 17] = bad;
            audio.right[frames / 2 + offset * 29] = bad;
        }
    }
    deck.play(effect.as_mut(), &mut audio);
    assert!(
        audio
            .left
            .iter()
            .chain(&audio.right)
            .all(|sample| sample.is_finite()),
        "{kind} let a bad sample through"
    );
    let mut after = Stereo::mono(sine(330.0, 0.5, frames));
    deck.play(effect.as_mut(), &mut after);
    after
}

#[test]
fn nan_and_infinity_never_reach_the_output_and_the_effect_recovers() {
    for kind in EffectKind::ALL {
        let recovered = play_through_bad_samples(kind, true);
        let clean = play_through_bad_samples(kind, false);
        let half = recovered.frames() / 2;
        let recovered_level = common::rms(&recovered.left[half..]);
        let clean_level = common::rms(&clean.left[half..]);
        assert!(
            (decibels_or_silence(recovered_level) - decibels_or_silence(clean_level)).abs() < 1.0,
            "{kind} plays at {recovered_level} after bad samples, {clean_level} without"
        );
        assert!(
            recovered
                .left
                .iter()
                .all(|sample| sample.abs() <= OUTPUT_CEILING)
        );
    }
}

fn decibels_or_silence(level: f32) -> f32 {
    common::decibels(level.max(1e-6))
}

#[test]
fn turning_knobs_and_switching_never_clicks() {
    let smooth_effects = EffectKind::ALL.into_iter().filter(|kind| {
        !matches!(
            kind,
            EffectKind::Gate | EffectKind::Crusher | EffectKind::Brake | EffectKind::Backspin
        )
    });
    for kind in smooth_effects {
        let mut effect = create(kind, &EffectConfig::default());
        effect.set_depth(0.0);
        let frames = seconds_to_frames(3.0);
        let mut audio = Stereo::mono(sine(220.0, 0.5, frames));
        let mut deck = Deck::new(124.0);
        let moments = [0.2, 0.6, 1.0, 1.4, 1.8, 2.2].map(seconds_to_frames);
        let mut start = 0;
        for (step, moment) in moments.into_iter().enumerate() {
            deck.play_range(effect.as_mut(), &mut audio, start, moment);
            match step {
                0 => effect.set_enabled(true),
                1 => effect.set_depth(1.0),
                2 => effect.set_mix(0.2),
                3 => {
                    if let Some(beat) = effect.beat() {
                        effect.set_beat(beat.longer());
                    }
                    effect.set_depth(0.3);
                }
                4 => effect.set_mix(1.0),
                _ => effect.set_enabled(false),
            }
            start = moment;
        }
        deck.play_range(effect.as_mut(), &mut audio, start, frames);

        let kink = largest_kink(&audio.left).max(largest_kink(&audio.right));
        assert!(kink < 0.01, "{kind} clicked: largest kink {kink}");
    }
}
