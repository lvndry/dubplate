//! Checks that echo, ping-pong and reverb never push a full-level track into clipping.
//!
//! Turning one of these effects on over a track should sound clearly present without jumping
//! the level, the way the beat effects of a club mixer do. Every check here plays a track that
//! peaks at -1 dBFS, the level of a loud club master.

mod common;

use std::f64::consts::TAU;

use common::{Deck, SAMPLE_RATE, Stereo, decibels, peak, rms, seconds_to_frames};
use dubplate_fx::{Echo, Effect, EffectConfig, EffectKind, PingPong, Reverb, create};

const TRAIL_EFFECTS: [EffectKind; 3] = [EffectKind::Echo, EffectKind::PingPong, EffectKind::Reverb];
const TRACK_PEAK: f32 = 0.891;
const FULL_SCALE: f32 = 1.0;
const TEMPO_BPM: f64 = 118.0;
const ALLOWED_PEAK_RISE_DECIBELS: f32 = 1.0;
const ALLOWED_LOUDNESS_RISE_DECIBELS: f32 = 1.0;
const WARM_UP_SECONDS: f32 = 1.0;

/// Independent white noise on each channel, peaking at [`TRACK_PEAK`].
fn loud_noise(frames: usize) -> Stereo {
    Stereo {
        left: common::noise(TRACK_PEAK, frames, 21),
        right: common::noise(TRACK_PEAK, frames, 22),
    }
}

/// A four-to-the-floor loop at [`TEMPO_BPM`]: a pitched-down kick on every beat, a hi-hat on
/// every off-beat and a minor chord progression that changes every bar, normalised to peak at
/// [`TRACK_PEAK`].
fn club_loop(frames: usize) -> Stereo {
    const CHORDS_HZ: [[f64; 3]; 4] = [
        [220.0, 261.63, 329.63],
        [174.61, 220.0, 261.63],
        [196.0, 246.94, 293.66],
        [164.81, 207.65, 246.94],
    ];
    let sample_rate = f64::from(SAMPLE_RATE);
    let beat_seconds = 60.0 / TEMPO_BPM;
    let hat_noise = common::noise(1.0, frames, 5);
    let mut kick_phase = 0.0;
    let mut samples: Vec<f32> = (0..frames)
        .map(|frame| {
            let seconds = frame as f64 / sample_rate;
            let beats = seconds / beat_seconds;
            let since_beat = beats.fract() * beat_seconds;
            let kick_hz = 50.0 + 90.0 * (-since_beat / 0.03).exp();
            kick_phase = if since_beat * sample_rate < 1.0 {
                0.0
            } else {
                kick_phase + kick_hz / sample_rate
            };
            let kick = (TAU * kick_phase).sin() * (-since_beat / 0.2).exp();

            let since_off_beat = (beats + 0.5).fract() * beat_seconds;
            let hat = f64::from(hat_noise[frame]) * 0.25 * (-since_off_beat / 0.02).exp();

            let bar = (beats / 4.0) as usize % CHORDS_HZ.len();
            let swell = 1.0 - (-((beats % 4.0) * beat_seconds) / 0.05).exp();
            let chord: f64 = CHORDS_HZ[bar]
                .iter()
                .map(|hertz| (TAU * hertz * seconds).sin())
                .sum::<f64>()
                * 0.12
                * swell;

            (kick + hat + chord) as f32
        })
        .collect();
    let loudest = peak(&samples);
    for sample in &mut samples {
        *sample *= TRACK_PEAK / loudest;
    }
    Stereo::mono(samples)
}

/// What an effect added to a channel.
fn difference(wet: &[f32], dry: &[f32]) -> Vec<f32> {
    wet.iter()
        .zip(dry)
        .map(|(wet_sample, dry_sample)| wet_sample - dry_sample)
        .collect()
}

/// What happened to a track played through an effect.
struct LevelChange {
    dry_peak: f32,
    wet_peak: f32,
    loudness_rise_decibels: f32,
    trail_loudness_decibels: f32,
}

impl LevelChange {
    fn peak_rise_decibels(&self) -> f32 {
        decibels(self.wet_peak / self.dry_peak)
    }
}

fn play(effect: &mut dyn Effect, dry: &Stereo) -> LevelChange {
    let mut wet = dry.clone();
    effect.set_enabled(true);
    Deck::new(TEMPO_BPM).play(effect, &mut wet);
    let settled = seconds_to_frames(WARM_UP_SECONDS);
    let loudness = |audio: &Stereo| {
        rms(&audio.left[settled..]).hypot(rms(&audio.right[settled..])) / 2.0_f32.sqrt()
    };
    let trail = Stereo {
        left: difference(&wet.left, &dry.left),
        right: difference(&wet.right, &dry.right),
    };
    LevelChange {
        dry_peak: peak(&dry.left).max(peak(&dry.right)),
        wet_peak: peak(&wet.left).max(peak(&wet.right)),
        loudness_rise_decibels: decibels(loudness(&wet) / loudness(dry)),
        trail_loudness_decibels: decibels(loudness(&trail) / loudness(dry)),
    }
}

/// Each trail effect as built, and with the performance knob at 0.6, its usual spot.
fn at_default_settings(kind: EffectKind) -> [Box<dyn Effect>; 2] {
    let config = EffectConfig::default();
    let as_built = create(kind, &config);
    let mut usual_depth = create(kind, &config);
    usual_depth.set_depth(0.6);
    [as_built, usual_depth]
}

#[test]
fn default_settings_are_clearly_heard_without_jumping_the_level() {
    let club_loop = club_loop(seconds_to_frames(8.0));
    let noise = loud_noise(seconds_to_frames(4.0));
    for kind in TRAIL_EFFECTS {
        for (name, track) in [("club loop", &club_loop), ("noise", &noise)] {
            for mut effect in at_default_settings(kind) {
                let change = play(effect.as_mut(), track);
                let label = format!("{kind} at mix {} on the {name}", effect.mix());
                assert!(
                    change.peak_rise_decibels() <= ALLOWED_PEAK_RISE_DECIBELS,
                    "{label} raised the peak by {} dB",
                    change.peak_rise_decibels()
                );
                assert!(
                    change.loudness_rise_decibels <= ALLOWED_LOUDNESS_RISE_DECIBELS,
                    "{label} raised the loudness by {} dB",
                    change.loudness_rise_decibels
                );
            }
        }
        for mut effect in at_default_settings(kind) {
            let change = play(effect.as_mut(), &club_loop);
            assert!(
                (-20.0..=-4.0).contains(&change.trail_loudness_decibels),
                "{kind} at mix {} added a trail at {} dB against the club loop",
                effect.mix(),
                change.trail_loudness_decibels
            );
        }
    }
}

/// A trail effect with its performance knob, mix and the control that sets its length all
/// turned as far as they go.
fn at_maximum(kind: EffectKind, mix: f32) -> Box<dyn Effect> {
    let config = EffectConfig::default();
    let mut effect: Box<dyn Effect> = match kind {
        EffectKind::Echo => {
            let mut echo = Echo::new(&config);
            echo.set_depth(1.0);
            echo.set_feedback(1.0);
            Box::new(echo)
        }
        EffectKind::PingPong => {
            let mut delay = PingPong::new(&config);
            delay.set_depth(1.0);
            delay.set_feedback(1.0);
            Box::new(delay)
        }
        EffectKind::Reverb => {
            let mut reverb = Reverb::new(&config);
            reverb.set_depth(1.0);
            reverb.set_decay_seconds(20.0);
            reverb.set_size(1.0);
            reverb.set_damping(0.0);
            reverb.set_low_cut_hz(20.0);
            Box::new(reverb)
        }
        other => panic!("{other} leaves no trail"),
    };
    effect.set_mix(mix);
    effect
}

#[test]
fn no_knob_position_takes_a_minus_one_decibel_track_past_full_scale() {
    let tracks = [
        ("club loop", club_loop(seconds_to_frames(4.0))),
        ("noise", loud_noise(seconds_to_frames(4.0))),
    ];
    for kind in TRAIL_EFFECTS {
        for (name, track) in &tracks {
            for depth in [0.0, 0.5, 1.0] {
                for mix in [0.25, 0.5, 0.75, 1.0] {
                    let mut effect = create(kind, &EffectConfig::default());
                    effect.set_depth(depth);
                    effect.set_mix(mix);
                    let change = play(effect.as_mut(), track);
                    assert!(
                        change.wet_peak <= FULL_SCALE,
                        "{kind} at depth {depth} and mix {mix} took the {name} to {}",
                        change.wet_peak
                    );
                }
            }
        }
    }
}

#[test]
fn maximum_feedback_held_for_thirty_seconds_stays_under_full_scale() {
    let frames = seconds_to_frames(30.0);
    let tracks = [
        ("club loop", club_loop(frames)),
        ("noise", loud_noise(frames)),
    ];
    for kind in TRAIL_EFFECTS {
        for (name, track) in &tracks {
            for mix in [0.5, 1.0] {
                let mut effect = at_maximum(kind, mix);
                let change = play(effect.as_mut(), track);
                assert!(
                    change.wet_peak <= FULL_SCALE,
                    "{kind} at maximum and mix {mix} took the {name} to {}",
                    change.wet_peak
                );
                assert!(
                    change.loudness_rise_decibels <= 3.0,
                    "{kind} at maximum and mix {mix} raised the {name} by {} dB",
                    change.loudness_rise_decibels
                );
            }
        }
    }
}

#[test]
fn full_feedback_never_builds_past_the_music_that_went_in() {
    let frames = seconds_to_frames(30.0);
    let quiet_track = {
        let mut track = club_loop(frames);
        for sample in track.left.iter_mut().chain(&mut track.right) {
            *sample *= 0.25;
        }
        track
    };
    for kind in [EffectKind::Echo, EffectKind::PingPong] {
        let mut effect = at_maximum(kind, 1.0);
        let change = play(effect.as_mut(), &quiet_track);
        assert!(
            change.peak_rise_decibels() <= ALLOWED_PEAK_RISE_DECIBELS,
            "{kind} at full feedback built a -13 dBFS track up by {} dB",
            change.peak_rise_decibels()
        );
        assert!(
            change.loudness_rise_decibels <= ALLOWED_LOUDNESS_RISE_DECIBELS,
            "{kind} at full feedback made a -13 dBFS track {} dB louder",
            change.loudness_rise_decibels
        );
    }
}
