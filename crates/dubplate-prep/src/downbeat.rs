//! Dubplate's pass over Euphoria's beat grid for dance music: it puts the beats on the kicks
//! rather than between them, and bar 1 on the kick a track opens with.
//!
//! Euphoria fits the tempo and the beats well, but on the owner's afro house and amapiano its
//! downbeat matched rekordbox's on about half the tracks, and about one grid in nine sat half
//! a beat off, on the hi-hats. Two things a DJ relies on settle both:
//!
//! - The kick of four-on-the-floor music lands on the beat and the hi-hats between beats. A
//!   kick's attack is a click across the whole spectrum, so onsets of the whole signal and of
//!   the kick band that are stronger half a beat after the grid's beats than on them, with the
//!   hi-hat band's onsets weaker there, put the grid on the off-beat. Where the track's first
//!   kick lands, on a beat or between two, counts too.
//! - DJ tracks start on a downbeat, so the first kick of a track is beat 1 of bar 1.
//!
//! The weights below were set on the 308 tracks of the owner's library that rekordbox also
//! analysed at the same tempo, with rekordbox's grids as the reference.

use euphoria_core::AudioBuffer;

use crate::analysis::AnalysedGrid;
use crate::grid::BEATS_PER_BAR;

/// The kick band's upper edge, in hertz.
const KICK_BAND_HERTZ: f64 = 120.0;
/// The hi-hat band's lower edge, in hertz.
const HI_HAT_BAND_HERTZ: f64 = 4_000.0;
/// The energy taken for silence, so its logarithm stays finite.
const SILENCE_ENERGY: f64 = 1e-10;
/// The length of one frame of a band's energy, in seconds.
const FRAME_SECONDS: f64 = 0.005;
/// An onset counts for a beat from this long before the beat to `ONSET_AFTER_SECONDS` after
/// it. Euphoria puts its beats a little after the onsets, where a listener taps.
const ONSET_BEFORE_SECONDS: f64 = 0.04;
const ONSET_AFTER_SECONDS: f64 = 0.02;
/// How far into the track its opening kick is looked for, in seconds.
const OPENING_SEARCH_SECONDS: f64 = 30.0;
/// The opening kick is the first frame of the kick band within this much of the loudest one
/// in the opening search, as a natural logarithm of energy: 4.6 is 20 dB.
const OPENING_BELOW_LOUDEST: f64 = 4.6;
/// How much each piece of evidence counts toward the grid being on the off-beat: the whole
/// signal's onsets, the kick band's and the hi-hat band's, each as the natural logarithm of
/// how much stronger they are half a beat after the beats than on them, and how far the
/// opening kick is from the nearest beat, as a share of a beat beyond a quarter.
const WHOLE_SIGNAL_WEIGHT: f64 = 1.0;
const KICK_WEIGHT: f64 = 0.5;
const HI_HAT_WEIGHT: f64 = -0.5;
const OPENING_WEIGHT: f64 = 0.5;
/// The evidence the off-beat needs before the grid moves half a beat.
const OFF_BEAT_THRESHOLD: f64 = 0.25;
/// An opening kick within this share of a beat of a beat of the grid is bar 1.
const OPENING_ON_A_BEAT: f64 = 0.2;

/// Euphoria's `grid` of `audio` with its beats moved onto the kicks when it sits on the
/// off-beat, and its bars counted from the kick the track opens with when that kick lands on
/// a beat. Otherwise Euphoria's downbeat stays.
pub(crate) fn settle_grid(audio: &AudioBuffer, grid: AnalysedGrid) -> AnalysedGrid {
    let sample_rate = f64::from(audio.sample_rate().get());
    let band = |filters: &[Filter]| Band::of_audio(audio, sample_rate, filters);
    let (Some(kick), Some(whole), Some(hi_hat)) = (
        band(&[
            Filter::LowPass(KICK_BAND_HERTZ),
            Filter::LowPass(KICK_BAND_HERTZ),
        ]),
        band(&[]),
        band(&[
            Filter::HighPass(HI_HAT_BAND_HERTZ),
            Filter::HighPass(HI_HAT_BAND_HERTZ),
        ]),
    ) else {
        return grid;
    };
    let beat_seconds = grid.beat_seconds();
    let bar_seconds = beat_seconds * f64::from(BEATS_PER_BAR);
    let first_beat = grid.first_downbeat_seconds.rem_euclid(beat_seconds);
    let opening_in_beats = kick
        .opening_seconds()
        .map(|seconds| (seconds - first_beat) / beat_seconds);
    let opening_evidence = opening_in_beats.map_or(0.0, |beats| {
        OPENING_WEIGHT * (distance_to_whole(beats) - 0.25)
    });
    let off_beat_evidence = WHOLE_SIGNAL_WEIGHT * whole.off_beat_evidence(first_beat, beat_seconds)
        + KICK_WEIGHT * kick.off_beat_evidence(first_beat, beat_seconds)
        + HI_HAT_WEIGHT * hi_hat.off_beat_evidence(first_beat, beat_seconds)
        + opening_evidence;
    let shift_beats = if off_beat_evidence > OFF_BEAT_THRESHOLD {
        0.5
    } else {
        0.0
    };
    let downbeat = opening_in_beats
        .map(|beats| beats - shift_beats)
        .filter(|beats| distance_to_whole(*beats) < OPENING_ON_A_BEAT)
        .map(|beats| first_beat + (shift_beats + beats.round()) * beat_seconds)
        .unwrap_or(grid.first_downbeat_seconds + shift_beats * beat_seconds);
    AnalysedGrid {
        bpm: grid.bpm,
        first_downbeat_seconds: downbeat.rem_euclid(bar_seconds),
    }
}

/// How far `value` is from the nearest whole number.
fn distance_to_whole(value: f64) -> f64 {
    (value - value.round()).abs()
}

/// The energy of one band of a track, frame by frame.
struct Band {
    /// The natural logarithm of each frame's energy.
    log_energy: Vec<f64>,
    /// How much each frame's energy rose over the two frames before it, or 0 where it fell.
    onsets: Vec<f64>,
    frame_seconds: f64,
}

impl Band {
    /// The band `filters` leave of `audio`, or None for audio too short to hold a frame.
    fn of_audio(audio: &AudioBuffer, sample_rate: f64, filters: &[Filter]) -> Option<Self> {
        let frame_length = (sample_rate * FRAME_SECONDS).round().max(1.0) as usize;
        let mut biquads: Vec<Biquad> = filters
            .iter()
            .map(|filter| Biquad::new(*filter, sample_rate))
            .collect();
        let log_energy: Vec<f64> = audio
            .samples()
            .chunks_exact(frame_length)
            .map(|frame| {
                let energy = frame
                    .iter()
                    .map(|sample| {
                        let filtered = biquads
                            .iter_mut()
                            .fold(f64::from(*sample), |value, biquad| biquad.run(value));
                        filtered * filtered
                    })
                    .sum::<f64>()
                    / frame_length as f64;
                (energy + SILENCE_ENERGY).ln()
            })
            .collect();
        if log_energy.is_empty() {
            return None;
        }
        let onsets = (0..log_energy.len())
            .map(|frame| match frame.checked_sub(2) {
                Some(earlier) => (log_energy[frame] - log_energy[earlier]).max(0.0),
                None => 0.0,
            })
            .collect();
        Some(Self {
            log_energy,
            onsets,
            frame_seconds: frame_length as f64 / sample_rate,
        })
    }

    /// When the band first sounds: the first frame of the opening search within 20 dB of its
    /// loudest frame. None when the opening search is silent.
    fn opening_seconds(&self) -> Option<f64> {
        let searched =
            ((OPENING_SEARCH_SECONDS / self.frame_seconds) as usize).min(self.log_energy.len());
        let opening = &self.log_energy[..searched];
        let loudest = opening.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        if loudest < SILENCE_ENERGY.ln() + OPENING_BELOW_LOUDEST {
            return None;
        }
        opening
            .iter()
            .position(|energy| *energy > loudest - OPENING_BELOW_LOUDEST)
            .map(|frame| frame as f64 * self.frame_seconds)
    }

    /// How much stronger, as a natural logarithm, the band's onsets are half a beat after the
    /// beats of a grid starting at `first_beat` than on them, across the whole track.
    fn off_beat_evidence(&self, first_beat: f64, beat_seconds: f64) -> f64 {
        let duration = self.onsets.len() as f64 * self.frame_seconds;
        let beats = ((duration - first_beat) / beat_seconds).floor().max(0.0) as usize;
        let (on_beat, off_beat) = (0..beats)
            .map(|beat| first_beat + beat as f64 * beat_seconds)
            .fold((0.0, 0.0), |(on_beat, off_beat), seconds| {
                (
                    on_beat + self.strongest_onset_near(seconds),
                    off_beat + self.strongest_onset_near(seconds + beat_seconds / 2.0),
                )
            });
        if on_beat > 0.0 && off_beat > 0.0 {
            (off_beat / on_beat).ln()
        } else {
            0.0
        }
    }

    /// The strongest onset counted for a beat at `seconds`.
    fn strongest_onset_near(&self, seconds: f64) -> f64 {
        let frame_at = |seconds: f64| (seconds / self.frame_seconds).max(0.0) as usize;
        let start = frame_at(seconds - ONSET_BEFORE_SECONDS).min(self.onsets.len());
        let end = frame_at(seconds + ONSET_AFTER_SECONDS).min(self.onsets.len());
        self.onsets[start..end.max(start)]
            .iter()
            .copied()
            .fold(0.0, f64::max)
    }
}

/// A second-order filter, with its corner in hertz.
#[derive(Debug, Clone, Copy)]
enum Filter {
    LowPass(f64),
    HighPass(f64),
}

/// A second-order filter as the Audio EQ Cookbook gives it, with a Butterworth Q.
struct Biquad {
    feedforward: [f64; 3],
    feedback: [f64; 2],
    inputs: [f64; 2],
    outputs: [f64; 2],
}

impl Biquad {
    fn new(filter: Filter, sample_rate: f64) -> Self {
        let corner = match filter {
            Filter::LowPass(hertz) | Filter::HighPass(hertz) => hertz,
        };
        let omega = std::f64::consts::TAU * corner / sample_rate;
        let alpha = omega.sin() * std::f64::consts::FRAC_1_SQRT_2;
        let cosine = omega.cos();
        let normal = 1.0 + alpha;
        let feedforward = match filter {
            Filter::LowPass(_) => {
                let side = (1.0 - cosine) / 2.0 / normal;
                [side, 2.0 * side, side]
            }
            Filter::HighPass(_) => {
                let side = (1.0 + cosine) / 2.0 / normal;
                [side, -2.0 * side, side]
            }
        };
        Self {
            feedforward,
            feedback: [-2.0 * cosine / normal, (1.0 - alpha) / normal],
            inputs: [0.0; 2],
            outputs: [0.0; 2],
        }
    }

    fn run(&mut self, input: f64) -> f64 {
        let output = self.feedforward[0] * input
            + self.feedforward[1] * self.inputs[0]
            + self.feedforward[2] * self.inputs[1]
            - self.feedback[0] * self.outputs[0]
            - self.feedback[1] * self.outputs[1];
        self.inputs = [input, self.inputs[0]];
        self.outputs = [output, self.outputs[0]];
        output
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::TAU;
    use std::num::NonZeroU32;

    use super::*;

    const SAMPLE_RATE: u32 = 44_100;
    const BPM: f64 = 122.0;
    const SECONDS: f64 = 40.0;

    /// Four-on-the-floor from `first_kick_seconds`: a low kick on every beat and a bright
    /// hi-hat half a beat after each one, the way afro house and amapiano sit.
    fn dance_track(first_kick_seconds: f64) -> AudioBuffer {
        let beat = 60.0 / BPM;
        let samples = (0..(SECONDS * f64::from(SAMPLE_RATE)) as usize)
            .map(|index| {
                let seconds = index as f64 / f64::from(SAMPLE_RATE);
                let since_first = seconds - first_kick_seconds;
                if since_first < 0.0 {
                    return 0.0;
                }
                let since_kick = since_first.rem_euclid(beat);
                let kick = 0.8 * (TAU * 55.0 * since_kick).sin() * (-since_kick * 18.0).exp();
                let since_hat = (since_first - beat / 2.0).rem_euclid(beat);
                let hat = 0.4 * (TAU * 7_000.0 * since_hat).sin() * (-since_hat * 90.0).exp();
                (kick + hat) as f32
            })
            .collect();
        AudioBuffer::new(
            samples,
            NonZeroU32::new(SAMPLE_RATE).expect("a sample rate"),
        )
    }

    fn grid(first_downbeat_seconds: f64) -> AnalysedGrid {
        AnalysedGrid {
            bpm: BPM,
            first_downbeat_seconds,
        }
    }

    fn assert_downbeat_at(settled: AnalysedGrid, seconds: f64) {
        let bar = 4.0 * 60.0 / BPM;
        let off = (settled.first_downbeat_seconds - seconds).rem_euclid(bar);
        let off = off.min(bar - off);
        assert!(off < 0.03, "downbeat at {settled:?}, wanted {seconds}");
        assert_eq!(settled.bpm, BPM);
    }

    #[test]
    fn a_track_that_opens_on_a_kick_has_its_first_bar_there() {
        let beat = 60.0 / BPM;
        let one_beat_late = settle_grid(&dance_track(0.0), grid(beat + 0.015));
        assert_downbeat_at(one_beat_late, 0.015);
    }

    #[test]
    fn a_grid_on_the_hi_hats_moves_onto_the_kicks() {
        let beat = 60.0 / BPM;
        let on_the_hats = settle_grid(&dance_track(0.0), grid(beat * 2.5 + 0.015));
        assert_downbeat_at(on_the_hats, 0.015);
    }

    #[test]
    fn a_silent_intro_puts_bar_one_on_the_first_kick() {
        let beat = 60.0 / BPM;
        let first_kick = 3.0 * beat + 0.4;
        let settled = settle_grid(&dance_track(first_kick), grid(first_kick + 0.015 + beat));
        assert_downbeat_at(settled, first_kick + 0.015);
    }

    #[test]
    fn silence_keeps_the_grid_it_was_given() {
        let silence = AudioBuffer::new(
            vec![0.0; SAMPLE_RATE as usize * 4],
            NonZeroU32::new(SAMPLE_RATE).expect("a sample rate"),
        );
        let given = grid(0.3);
        let settled = settle_grid(&silence, given);
        assert_eq!(settled, given);
    }
}
