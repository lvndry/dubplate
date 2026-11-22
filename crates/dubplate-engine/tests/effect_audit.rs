//! Every effect, played through an effect unit the way the DJ plays it, is heard on every
//! target.
//!
//! Each check renders two engines side by side, one with the unit on and one without, and
//! measures what the unit changed.

mod common;

use std::f64::consts::TAU;

use common::{SAMPLE_RATE, left_channel, render_frames, right_channel, wait_until_decoded};
use dubplate_engine::{
    BeatGrid, Controller, DeckId, EffectKind, EffectTarget, EffectUnitId, Renderer, Track,
    decode_file, offline_engine,
};

const BLOCK: usize = 256;
const TEMPO_BPM: f64 = 120.0;
const UNIT: EffectUnitId = EffectUnitId::ALL[0];
const LEAD_IN_SECONDS: f64 = 1.0;
const ON_SECONDS: f64 = 4.0;
const TRAIL_SECONDS: f64 = 1.0;

/// A four-to-the-floor loop: a kick on every beat, a hi-hat on every off-beat, a shaker on the
/// sixteenths and a chord, peaking around -3 dBFS.
fn club_loop(seconds: f64) -> Track {
    let frames = (seconds * f64::from(SAMPLE_RATE)) as usize;
    let beat_seconds = 60.0 / TEMPO_BPM;
    let mut noise_state: u64 = 0x2545_f491_4f6c_dd1d;
    let mut kick_phase = 0.0;
    let samples: Vec<f32> = (0..frames)
        .flat_map(|frame| {
            let seconds = frame as f64 / f64::from(SAMPLE_RATE);
            let beats = seconds / beat_seconds;
            let since_beat = beats.fract() * beat_seconds;
            let kick_hz = 50.0 + 90.0 * (-since_beat / 0.03).exp();
            kick_phase += kick_hz / f64::from(SAMPLE_RATE);
            let kick = (TAU * kick_phase).sin() * (-since_beat / 0.2).exp() * 0.5;
            noise_state ^= noise_state << 13;
            noise_state ^= noise_state >> 7;
            noise_state ^= noise_state << 17;
            let noise = (noise_state >> 40) as f64 / (1_u64 << 24) as f64 * 2.0 - 1.0;
            let since_off_beat = (beats + 0.5).fract() * beat_seconds;
            let hat = noise * 0.2 * (-since_off_beat / 0.03).exp();
            let since_sixteenth = (beats * 4.0).fract() * beat_seconds / 4.0;
            let shaker = noise * 0.06 * (-since_sixteenth / 0.02).exp();
            let chord: f64 = [220.0, 261.63, 329.63]
                .iter()
                .map(|hertz| (TAU * hertz * seconds).sin())
                .sum::<f64>()
                * 0.07;
            let value = (kick + hat + shaker + chord) as f32;
            [value, value]
        })
        .collect();
    Track::from_interleaved_stereo(&samples, SAMPLE_RATE).expect("valid test track")
}

/// `deck` plays `track` alone with a beat grid, the crossfader in the middle.
fn playing(track: &Track, deck: DeckId) -> (Controller, Renderer) {
    let (mut controller, renderer) = offline_engine(SAMPLE_RATE);
    controller
        .load_track(deck, track.clone())
        .expect("load the track");
    controller
        .set_beat_grid(deck, Some(BeatGrid::new(TEMPO_BPM, 0.0).expect("grid")))
        .expect("set the grid");
    controller.play(deck).expect("play");
    (controller, renderer)
}

/// The deck that plays while a unit sits on `target`: the target's own deck, or deck B, the
/// deck the DJ reported effects missing on.
fn deck_for(target: EffectTarget) -> DeckId {
    match target {
        EffectTarget::Deck(deck) => deck,
        _ => DeckId::B,
    }
}

fn frames(seconds: f64) -> usize {
    (seconds * f64::from(SAMPLE_RATE)) as usize
}

fn rms(samples: &[f32]) -> f64 {
    let energy: f64 = samples
        .iter()
        .map(|sample| f64::from(*sample).powi(2))
        .sum();
    (energy / samples.len().max(1) as f64).sqrt()
}

fn decibels(ratio: f64) -> f64 {
    20.0 * ratio.max(1e-12).log10()
}

/// What a unit did to the music.
#[derive(Debug, Clone, Copy)]
struct Change {
    /// The loudness of the difference from the dry mix, against the dry mix, in dB.
    difference_decibels: f64,
    /// The change in level, in dB.
    level_decibels: f64,
    /// The loudness of the difference in the second after the unit is switched off, in dB.
    trail_decibels: f64,
}

/// Plays `track` with the unit set to `kind` at `depth` on `target`, switched on for a few
/// seconds and then off.
fn change(track: &Track, kind: EffectKind, target: EffectTarget, depth: Option<f32>) -> Change {
    let (_reference_controller, mut reference) = playing(track, deck_for(target));
    let (mut controller, mut renderer) = playing(track, deck_for(target));
    controller
        .set_effect_kind(UNIT, kind)
        .expect("choose the effect");
    controller
        .set_effect_target(UNIT, target)
        .expect("choose the target");
    if let Some(depth) = depth {
        controller.set_effect_depth(UNIT, depth).expect("depth");
    }
    let lead_in = frames(LEAD_IN_SECONDS);
    render_frames(&mut reference, lead_in, BLOCK);
    render_frames(&mut renderer, lead_in, BLOCK);
    controller
        .set_effect_enabled(UNIT, true)
        .expect("switch on");
    let dry = render_frames(&mut reference, frames(ON_SECONDS), BLOCK);
    let wet = render_frames(&mut renderer, frames(ON_SECONDS), BLOCK);
    controller
        .set_effect_enabled(UNIT, false)
        .expect("switch off");
    let dry_trail = render_frames(&mut reference, frames(TRAIL_SECONDS), BLOCK);
    let wet_trail = render_frames(&mut renderer, frames(TRAIL_SECONDS), BLOCK);

    let difference = |wet: &[f32], dry: &[f32]| -> Vec<f32> {
        wet.iter().zip(dry).map(|(wet, dry)| wet - dry).collect()
    };
    let both = |interleaved: &[f32]| {
        rms(&left_channel(interleaved)).hypot(rms(&right_channel(interleaved)))
    };
    let dry_level = both(&dry);
    Change {
        difference_decibels: decibels(both(&difference(&wet, &dry)) / dry_level),
        level_decibels: decibels(both(&wet) / dry_level),
        trail_decibels: decibels(both(&difference(&wet_trail, &dry_trail)) / both(&dry_trail)),
    }
}

/// The quietest change a DJ hears clearly on a busy club track, in dB under the music.
const AUDIBLE_DECIBELS: f64 = -14.0;

#[test]
fn every_effect_is_clearly_heard_on_every_target_from_a_quarter_of_the_knob() {
    let track = club_loop(LEAD_IN_SECONDS + ON_SECONDS + TRAIL_SECONDS + 1.0);
    let mut quiet = Vec::new();
    for kind in EffectKind::ALL {
        for target in EffectTarget::ALL {
            for depth in [None, Some(0.25), Some(0.5), Some(1.0)] {
                let change = change(&track, kind, target, depth);
                if change.difference_decibels < AUDIBLE_DECIBELS {
                    quiet.push(format!(
                        "{kind} on {target:?} at depth {depth:?}: {:.1} dB",
                        change.difference_decibels
                    ));
                }
            }
        }
    }
    assert!(quiet.is_empty(), "barely heard:\n{}", quiet.join("\n"));
}

/// Prints what every effect does to the tracks named in `DUBPLATE_AUDIT_TRACKS`, separated by
/// `:`. Run with `--ignored --nocapture`.
#[test]
#[ignore = "reads the DJ's own tracks"]
fn audit_on_real_tracks() {
    let paths = std::env::var("DUBPLATE_AUDIT_TRACKS").expect("DUBPLATE_AUDIT_TRACKS");
    let seek = std::env::var("DUBPLATE_AUDIT_START")
        .ok()
        .and_then(|text| text.parse::<f64>().ok())
        .unwrap_or(60.0);
    for path in paths.split(':') {
        let full = decode_file(path).expect("decode");
        wait_until_decoded(&full);
        let track_frames = |seconds: f64| (seconds * f64::from(full.sample_rate())) as usize;
        let start = track_frames(seek);
        let length = track_frames(LEAD_IN_SECONDS + ON_SECONDS + TRAIL_SECONDS + 1.0);
        let interleaved: Vec<f32> = (start..start + length)
            .filter_map(|index| full.frame(index))
            .flatten()
            .collect();
        let track =
            Track::from_interleaved_stereo(&interleaved, full.sample_rate()).expect("section");
        println!("{path}");
        println!(
            "{:<10} {:<8} {:>6} {:>10} {:>8} {:>8}",
            "effect", "target", "depth", "difference", "level", "trail"
        );
        for kind in EffectKind::ALL {
            for target in EffectTarget::ALL {
                for depth in [None, Some(0.25), Some(1.0)] {
                    let change = change(&track, kind, target, depth);
                    println!(
                        "{:<10} {:<8} {:>6} {:>10.1} {:>8.1} {:>8.1}",
                        kind.name(),
                        format!("{target:?}").replace("Deck(", "").replace(')', ""),
                        depth.map_or_else(|| "default".to_owned(), |depth| depth.to_string()),
                        change.difference_decibels,
                        change.level_decibels,
                        change.trail_decibels
                    );
                }
            }
        }
    }
}
