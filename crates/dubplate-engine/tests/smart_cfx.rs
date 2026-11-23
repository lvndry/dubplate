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
    assert_eq!(SmartCfx::Phase.next(), SmartCfx::Pitch);
    assert_eq!(SmartCfx::Pitch.next(), SmartCfx::Space);
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

#[test]
fn sweep_plays_without_the_filter_gating_to_the_left_and_narrowing_to_the_right() {
    let track = sine_track(5_000.0, 0.5, 0.5, 4.0, SAMPLE_RATE);
    let level = |preset, knob| {
        let (mut controller, mut renderer) = deck(track.clone(), preset);
        controller.set_filter(DeckId::A, knob).unwrap();
        let output = seconds(&mut renderer, 2.0);
        rms(&output[48_000..])
    };
    let filtered = level(None, 0.05);
    let gated = level(Some(SmartCfx::Sweep), 0.05);
    assert!(
        filtered < 0.03,
        "the filter takes 5 kHz out far left: {filtered}"
    );
    assert!(
        gated > 0.08 && gated < 0.3,
        "Sweep far left gates the 5 kHz instead of filtering it: {gated}"
    );
    let narrowed = level(Some(SmartCfx::Sweep), 0.95);
    let open = level(Some(SmartCfx::Sweep), 0.5);
    assert!(
        narrowed < open * 0.5,
        "Sweep far right narrows on the mids, away from 5 kHz: {narrowed} against {open}"
    );
}

#[test]
fn noise_lays_noise_over_silence_either_way() {
    let track = sine_track(440.0, 0.0, 0.0, 4.0, SAMPLE_RATE);
    for knob in [0.1, 0.9] {
        let (mut controller, mut renderer) = deck(track.clone(), Some(SmartCfx::Noise));
        controller.set_filter(DeckId::A, knob).unwrap();
        let output = seconds(&mut renderer, 2.0);
        let level = rms(&output[48_000..]);
        assert!(level > 0.01, "Noise at {knob} is silent: {level}");
    }
}

#[test]
fn plain_cfx_filter_passes_lows_left_highs_right_and_returns_flat() {
    let track = club_loop_track(124.0, 8.0, SAMPLE_RATE);
    let (mut controller, mut renderer) = deck(track.clone(), None);
    let (mut reference_controller, mut reference) = deck(track, None);
    for position in [0.51, 0.0, 0.5, 1.0, 0.5] {
        controller.set_filter(DeckId::A, position).unwrap();
        reference_controller.set_filter(DeckId::A, 0.5).unwrap();
        let audio = seconds(&mut renderer, 1.25);
        let dry = seconds(&mut reference, 1.25);
        let start = SAMPLE_RATE as usize / 4;
        let difference: Vec<_> = audio[start..]
            .iter()
            .zip(&dry[start..])
            .map(|(actual, dry)| actual - dry)
            .collect();
        if position == 0.5 || position == 0.51 {
            assert!(
                rms(&difference) < 1e-5,
                "{position} detent did not restore dry"
            );
        } else {
            assert!(
                rms(&difference) > rms(&dry[start..]) * 0.20,
                "{position} did not engage the filter"
            );
        }
    }

    for (knob, pass, cut) in [(0.1, 80.0, 5_000.0), (0.9, 5_000.0, 80.0)] {
        let level = |frequency| {
            let track = sine_track(frequency, 0.5, 0.5, 2.0, SAMPLE_RATE);
            let (mut controller, mut renderer) = deck(track, None);
            controller.set_filter(DeckId::A, knob).unwrap();
            let audio = seconds(&mut renderer, 1.0);
            rms(&audio[SAMPLE_RATE as usize / 2..])
        };
        assert!(level(pass) > 0.25, "knob {knob} blocked its passband");
        assert!(level(cut) < 0.01, "knob {knob} did not reject its stopband");
    }
}

#[test]
fn every_cfx_preset_stays_on_its_assigned_decks_and_returns_dry() {
    let track = club_loop_track(124.0, 20.0, SAMPLE_RATE);
    let fixed_position = 0.75;
    let reference = |deck_id: DeckId| {
        let (mut controller, renderer) = offline_engine(SAMPLE_RATE);
        for id in DeckId::ALL {
            controller.load_track(id, track.clone()).unwrap();
            controller
                .set_beat_grid(id, Some(BeatGrid::new(124.0, 0.0).unwrap()))
                .unwrap();
            controller.play(id).unwrap();
        }
        let crossfader = if deck_id == DeckId::A { 0.0 } else { 1.0 };
        controller.set_crossfader(crossfader).unwrap();
        controller.set_filter(deck_id, fixed_position).unwrap();
        (controller, renderer)
    };
    let settled = SAMPLE_RATE as usize / 2;

    for preset in SmartCfx::ALL {
        for audible in DeckId::ALL {
            let other = if audible == DeckId::A {
                DeckId::B
            } else {
                DeckId::A
            };
            let (mut controller, mut renderer) = reference(audible);
            let (mut dry_controller, mut dry_renderer) = reference(audible);
            seconds(&mut renderer, 0.25);
            seconds(&mut dry_renderer, 0.25);

            // Normal CFX permits one assigned deck. It must not leak into the other deck.
            controller.set_smart_cfx(other, Some(preset)).unwrap();
            controller.set_filter(other, 0.25).unwrap();
            assert!(
                seconds(&mut renderer, 0.5) == seconds(&mut dry_renderer, 0.5),
                "{preset} leaked from {other:?} into {audible:?}"
            );

            // Smart CFX sends the shared colour to both decks, using each deck's CFX knob.
            controller.set_smart_cfx(audible, Some(preset)).unwrap();
            let effected = seconds(&mut renderer, 1.5);
            let dry = seconds(&mut dry_renderer, 1.5);
            let difference: Vec<_> = effected[settled..]
                .iter()
                .zip(&dry[settled..])
                .map(|(actual, dry)| actual - dry)
                .collect();
            assert!(
                rms(&difference) > 0.001,
                "{preset} never reached {audible:?}"
            );
            assert!(effected.iter().all(|sample| sample.is_finite()));

            controller.set_filter(audible, 0.5).unwrap();
            dry_controller.set_filter(audible, 0.5).unwrap();
            // Dub Echo and Space have deliberately long feedback tails. Once they decay,
            // the exact dry signal should be back, like every non-trailing effect.
            seconds(&mut renderer, 16.0);
            seconds(&mut dry_renderer, 16.0);
            assert!(
                seconds(&mut renderer, 0.25) == seconds(&mut dry_renderer, 0.25),
                "{preset} did not return to dry on {audible:?}"
            );
        }
    }
}

#[test]
fn pitch_bends_the_test_tone_down_left_and_up_right() {
    let source = 523.25;
    let track = sine_track(source, 0.3, 0.3, 2.0, SAMPLE_RATE);
    for (knob, direction) in [(0.3, -1.0), (0.7, 1.0)] {
        let (mut controller, mut renderer) = deck(track.clone(), Some(SmartCfx::Pitch));
        controller.set_filter(DeckId::A, knob).unwrap();
        let output = seconds(&mut renderer, 1.75);
        let signal = &output[SAMPLE_RATE as usize..];
        let power_at = |frequency: f64| {
            let phase_step = std::f64::consts::TAU * frequency / f64::from(SAMPLE_RATE);
            let (sine, cosine) =
                signal
                    .iter()
                    .enumerate()
                    .fold((0.0, 0.0), |(sine, cosine), (index, sample)| {
                        let (sin, cos) = (phase_step * index as f64).sin_cos();
                        (
                            sine + f64::from(*sample) * sin,
                            cosine + f64::from(*sample) * cos,
                        )
                    });
            sine.hypot(cosine)
        };
        let offset = ((0.4_f64 - 0.02) / 0.98).powf(0.6);
        let band_peak = |sign: f64| {
            let expected = source * (sign * offset).exp2();
            (-20..=20)
                .map(|step| power_at(expected + f64::from(step)))
                .fold(0.0_f64, f64::max)
        };
        let right_direction = band_peak(direction);
        let wrong_direction = band_peak(-direction);
        assert!(
            right_direction > 20.0 && right_direction > 5.0 * wrong_direction,
            "knob {knob}: pitch energy {right_direction} in intended band and {wrong_direction} in opposite band"
        );
    }
}
