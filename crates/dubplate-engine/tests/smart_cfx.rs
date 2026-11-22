//! Smart CFX: the filter knob drives the filter and a second effect together.

mod common;

use common::{
    SAMPLE_RATE, club_loop_track, left_channel, mixer_latency_frames, render_frames, sine_track,
};
use dubplate_engine::{BeatGrid, Controller, DeckId, Renderer, SmartCfx, Track, offline_engine};

const BLOCK: usize = 256;

fn deck(track: Track, preset: Option<SmartCfx>) -> (Controller, Renderer) {
    let (mut controller, renderer) = offline_engine(SAMPLE_RATE);
    controller.load_track(DeckId::A, track).expect("load");
    controller
        .set_beat_grid(DeckId::A, Some(BeatGrid::new(124.0, 0.0).expect("grid")))
        .expect("grid");
    controller.set_crossfader(0.0).expect("crossfader");
    controller.set_smart_cfx(DeckId::A, preset).expect("preset");
    controller.play(DeckId::A).expect("play");
    (controller, renderer)
}

fn seconds(renderer: &mut Renderer, seconds: f64) -> Vec<f32> {
    left_channel(&render_frames(
        renderer,
        (seconds * f64::from(SAMPLE_RATE)) as usize,
        BLOCK,
    ))
}

#[test]
fn every_preset_leaves_the_music_untouched_at_the_centre() {
    let track = sine_track(523.0, 0.5, 0.5, 4.0, SAMPLE_RATE);
    for preset in SmartCfx::ALL {
        let (controller, mut renderer) = deck(track.clone(), Some(preset));
        let output = seconds(&mut renderer, 1.0);
        let latency = mixer_latency_frames(&controller);
        for frame in [10_000, 30_000] {
            let [expected, _] = track.frame(frame - latency).unwrap();
            assert_eq!(output[frame], expected, "{preset} changed the centre");
        }
    }
}

#[test]
fn every_preset_changes_the_sound_off_centre_and_stays_bounded() {
    let track = sine_track(330.0, 0.6, 0.6, 6.0, SAMPLE_RATE);
    for knob in [0.15, 0.85] {
        let (mut plain_controller, mut plain_renderer) = deck(track.clone(), None);
        plain_controller.set_filter(DeckId::A, knob).unwrap();
        let plain = seconds(&mut plain_renderer, 2.0);
        let plain_peak = plain[48_000..]
            .iter()
            .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
        for preset in SmartCfx::ALL {
            let (mut controller, mut renderer) = deck(track.clone(), Some(preset));
            controller.set_filter(DeckId::A, knob).unwrap();
            let output = seconds(&mut renderer, 2.0);
            assert!(
                output
                    .iter()
                    .all(|sample| sample.is_finite() && sample.abs() < 1.0)
            );
            let biggest_difference = output[48_000..]
                .iter()
                .zip(&plain[48_000..])
                .map(|(effected, filtered)| (effected - filtered).abs())
                .fold(0.0_f32, f32::max);
            assert!(
                biggest_difference > plain_peak * 0.05,
                "{preset} at {knob} adds nothing to the filter"
            );
        }
    }
}

#[test]
fn back_at_the_centre_the_effect_rings_out_and_the_music_returns_exactly() {
    let track = sine_track(440.0, 0.5, 0.5, 20.0, SAMPLE_RATE);
    let (mut controller, mut renderer) = deck(track.clone(), Some(SmartCfx::DubEcho));
    controller.set_filter(DeckId::A, 0.85).unwrap();
    seconds(&mut renderer, 1.0);
    controller.set_filter(DeckId::A, 0.5).unwrap();
    seconds(&mut renderer, 16.0);
    let output = seconds(&mut renderer, 0.5);
    let latency = mixer_latency_frames(&controller);
    let start_frame = (17.0 * f64::from(SAMPLE_RATE)) as usize;
    for (index, sample) in output.iter().enumerate().step_by(997) {
        let [expected, _] = track.frame(start_frame + index - latency).unwrap();
        assert_eq!(*sample, expected);
    }
    assert_eq!(controller.smart_cfx(DeckId::A), Some(SmartCfx::DubEcho));
    assert_eq!(SmartCfx::Phase.next(), SmartCfx::Space);
}

fn rms(samples: &[f32]) -> f32 {
    let energy: f64 = samples
        .iter()
        .map(|sample| f64::from(*sample).powi(2))
        .sum();
    (energy / samples.len().max(1) as f64).sqrt() as f32
}

/// The knob positions a quarter, half and three quarters of the way to each side.
const KNOB_POSITIONS: [f32; 6] = [0.375, 0.25, 0.125, 0.625, 0.75, 0.875];

/// The quietest difference from the plain filter, in dB of RMS under the filtered music, that
/// a DJ hears clearly on a club loop.
const AUDIBLE_DECIBELS: f32 = -20.0;

#[test]
fn every_preset_is_clearly_heard_over_the_plain_filter_from_a_quarter_of_the_knob() {
    let track = club_loop_track(124.0, 5.0, SAMPLE_RATE);
    let settled = SAMPLE_RATE as usize;
    let mut quiet = Vec::new();
    for knob in KNOB_POSITIONS {
        let (mut plain_controller, mut plain_renderer) = deck(track.clone(), None);
        plain_controller.set_filter(DeckId::A, knob).unwrap();
        let plain = seconds(&mut plain_renderer, 3.0);
        for preset in SmartCfx::ALL {
            let (mut controller, mut renderer) = deck(track.clone(), Some(preset));
            controller.set_filter(DeckId::A, knob).unwrap();
            let output = seconds(&mut renderer, 3.0);
            let difference: Vec<f32> = output[settled..]
                .iter()
                .zip(&plain[settled..])
                .map(|(effected, filtered)| effected - filtered)
                .collect();
            let decibels = 20.0 * (rms(&difference) / rms(&plain[settled..])).log10();
            if decibels < AUDIBLE_DECIBELS {
                quiet.push(format!("{preset} at knob {knob}: {decibels:.1} dB"));
            }
        }
    }
    assert!(quiet.is_empty(), "barely heard:\n{}", quiet.join("\n"));
}

#[test]
fn turning_smart_cfx_on_after_the_knob_moved_takes_the_knob_where_it_is() {
    let track = club_loop_track(124.0, 4.0, SAMPLE_RATE);
    let (mut plain_controller, mut plain_renderer) = deck(track.clone(), None);
    plain_controller.set_filter(DeckId::A, 0.25).unwrap();
    let plain = seconds(&mut plain_renderer, 2.0);
    let (mut controller, mut renderer) = deck(track, None);
    controller.set_filter(DeckId::A, 0.25).unwrap();
    controller
        .set_smart_cfx(DeckId::A, Some(SmartCfx::Chop))
        .unwrap();
    let output = seconds(&mut renderer, 2.0);
    let settled = SAMPLE_RATE as usize / 2;
    let changed = output[settled..]
        .iter()
        .zip(&plain[settled..])
        .filter(|(effected, filtered)| (*effected - *filtered).abs() > 1e-3)
        .count();
    assert!(changed > settled / 4, "the chop never came in");
}
