use std::sync::{Arc, OnceLock};

use crate::curves::Ramp;
use crate::mixer::RetireQueue;
use crate::stems::{PART_CHANNELS, PartFrame, Stem, all_full, mix_parts};
use crate::track::{FrameRead, TrackBuffer};

/// The gain of each stem for one output frame, in the order of [`Stem::ALL`].
pub(crate) type StemGains = [f32; Stem::COUNT];

/// Every stem at its level in the track.
pub(crate) const UNITY_STEM_GAINS: StemGains = [1.0; Stem::COUNT];

/// Audio the resampling kernel reads, a left and a right channel at a time: a track, or one stem
/// of its stems.
pub(crate) trait StereoFrames {
    /// One frame, without waiting.
    fn read_frame(&self, index: i64) -> FrameRead;

    /// Hands `visit` the `N` frames from `first` on with their offset in the window, and
    /// returns whether any of them is not decoded yet.
    fn visit_frames<const N: usize>(&self, first: i64, visit: impl FnMut(usize, f32, f32)) -> bool;
}

impl StereoFrames for TrackBuffer {
    #[inline]
    fn read_frame(&self, index: i64) -> FrameRead {
        TrackBuffer::read_frame(self, index)
    }

    #[inline]
    fn visit_frames<const N: usize>(&self, first: i64, visit: impl FnMut(usize, f32, f32)) -> bool {
        TrackBuffer::visit_frames::<N>(self, first, visit)
    }
}

/// Fades that can overlap on one deck, for jumps that come faster than a fade lasts.
const MAX_TAILS: usize = 8;

/// A short fade that lets the old playback position die away after a jump, a pause or a load,
/// so the waveform never steps from one value to another between two samples.
struct Tail {
    track: Arc<TrackBuffer>,
    reads_stems: bool,
    position: f64,
    step: f64,
    start_gain: f32,
    remaining_frames: u32,
    wrap_frames: Option<usize>,
}

impl Tail {
    /// The tail's gain on its next frame. It falls one step before the first frame, so with
    /// the new position fading in from one step the two always add up to the level before the
    /// jump.
    fn gain(&self, declick_frames: u32) -> f32 {
        self.start_gain * self.remaining_frames.saturating_sub(1) as f32 / declick_frames as f32
    }
}

/// What a deck plays: the track, the playhead, its fade in and out, and the tails of the
/// positions it just left.
pub(crate) struct Voice {
    pub(crate) track: Option<Arc<TrackBuffer>>,
    /// The voice plays the track's stems, each at its gain, instead of the track.
    reads_stems: bool,
    /// The playhead, in track frames.
    pub(crate) position: f64,
    pub(crate) transport: Ramp,
    tails: [Option<Tail>; MAX_TAILS],
    declick_frames: u32,
    kernel: &'static ResamplingKernel,
}

impl Voice {
    pub(crate) fn new(declick_frames: u32) -> Self {
        Self {
            track: None,
            reads_stems: false,
            position: 0.0,
            transport: Ramp::new(0.0, declick_frames),
            tails: std::array::from_fn(|_| None),
            declick_frames,
            // Voice construction happens before the renderer reaches the audio thread.
            kernel: ResamplingKernel::shared(),
        }
    }

    /// Whether a tail from an earlier position is still fading out.
    pub(crate) fn has_tails(&self) -> bool {
        self.tails.iter().any(Option::is_some)
    }

    /// The last frame of the track, once it is fully decoded.
    pub(crate) fn end_frame(&self) -> Option<f64> {
        self.track
            .as_ref()
            .and_then(|track| track.total_frames())
            .map(|frames| frames as f64)
    }

    /// Hands what the voice is playing over to a fading tail that keeps moving by `step` per
    /// frame, and silences the playhead. Tails from earlier jumps keep fading; when every slot
    /// is busy the quietest one is dropped.
    pub(crate) fn start_tail(&mut self, step: f64, retire: &mut RetireQueue) {
        self.start_tail_with_wrap(step, false, retire);
    }

    /// Preserves a sampler loop's periodic filter extension while its old position fades.
    pub(crate) fn start_tail_with_wrap(
        &mut self,
        step: f64,
        looped: bool,
        retire: &mut RetireQueue,
    ) {
        let gain = self.transport.current();
        self.transport.jump_to(0.0);
        if gain <= 0.0 {
            return;
        }
        let Some(track) = self.track.clone() else {
            return;
        };
        let wrap_frames = looped
            .then(|| track.total_frames())
            .flatten()
            .filter(|frames| *frames > 0);
        let tail = Tail {
            track,
            reads_stems: self.reads_stems,
            position: self.position,
            step,
            start_gain: gain,
            remaining_frames: self.declick_frames,
            wrap_frames,
        };
        let declick_frames = self.declick_frames;
        let slot = match self.tails.iter().position(Option::is_none) {
            Some(free) => free,
            None => self
                .tails
                .iter()
                .enumerate()
                .min_by(|(_, first), (_, second)| {
                    let first_gain = first.as_ref().map_or(0.0, |tail| tail.gain(declick_frames));
                    let second_gain = second
                        .as_ref()
                        .map_or(0.0, |tail| tail.gain(declick_frames));
                    first_gain.total_cmp(&second_gain)
                })
                .map_or(0, |(index, _)| index),
        };
        if let Some(evicted) = self.tails[slot].replace(tail) {
            retire.track(evicted.track);
        }
    }

    /// Moves the playhead to `frame` with a crossfade: the old position fades out as a tail
    /// while the new one fades in to the level the playhead was heading for.
    pub(crate) fn jump(&mut self, frame: f64, step: f64, retire: &mut RetireQueue) {
        self.jump_with_wrap(frame, step, false, retire);
    }

    pub(crate) fn jump_with_wrap(
        &mut self,
        frame: f64,
        step: f64,
        looped: bool,
        retire: &mut RetireQueue,
    ) {
        let target = self.transport.target();
        self.start_tail_with_wrap(step, looped, retire);
        self.position = frame;
        if target > 0.0 {
            self.transport.set_target(target);
        }
    }

    /// Whether the voice plays the track's stems instead of the track.
    pub(crate) fn reads_stems(&self) -> bool {
        self.reads_stems
    }

    /// Switches between the track and its stems with a crossfade at the same position, as a
    /// jump to where the playhead already is: what was playing fades out as a tail while the
    /// other fades in.
    pub(crate) fn set_reads_stems(
        &mut self,
        reads_stems: bool,
        step: f64,
        retire: &mut RetireQueue,
    ) {
        if self.reads_stems == reads_stems {
            return;
        }
        let target = self.transport.target();
        self.start_tail(step, retire);
        self.reads_stems = reads_stems;
        if target > 0.0 {
            self.transport.set_target(target);
        }
    }

    /// Mixes the next frame of the playhead at `gain` and of every tail, and moves the tails
    /// on. Sets `starved` when a frame needed was not decoded yet.
    #[inline]
    pub(crate) fn next_frame(
        &mut self,
        gain: f32,
        step: f64,
        looped: bool,
        retire: &mut RetireQueue,
        starved: &mut bool,
    ) -> (f32, f32) {
        let (left, right) = if gain > 0.0 {
            self.read_playhead_with_wrap(step, looped, &UNITY_STEM_GAINS, starved)
        } else {
            (0.0, 0.0)
        };
        let (tail_left, tail_right) = self.next_tails(&UNITY_STEM_GAINS, retire, starved);
        (left * gain + tail_left, right * gain + tail_right)
    }

    /// Every stem and the track at the next frame of the playhead and of every tail, unmixed,
    /// and moves the tails on: what key lock stretches when the voice reads stems.
    #[inline]
    pub(crate) fn next_part_frame(
        &mut self,
        step: f64,
        retire: &mut RetireQueue,
        starved: &mut bool,
    ) -> PartFrame {
        let mut frame = match &self.track {
            Some(track) => self.kernel.read_parts(track, self.position, step, starved),
            None => [0.0; PART_CHANNELS],
        };
        for slot in &mut self.tails {
            let Some(tail) = slot else {
                continue;
            };
            let tail_gain = tail.gain(self.declick_frames);
            let tail_frame = self
                .kernel
                .read_parts(&tail.track, tail.position, tail.step, starved);
            for (sample, tail_sample) in frame.iter_mut().zip(tail_frame) {
                *sample += tail_sample * tail_gain;
            }
            if tail.advance()
                && let Some(finished) = slot.take()
            {
                retire.track(finished.track);
            }
        }
        frame
    }

    /// The frame under the playhead, interpolated: the track's, or its stems mixed at
    /// `stem_gains` with what they left out.
    #[inline]
    pub(crate) fn read_playhead(
        &self,
        step: f64,
        stem_gains: &StemGains,
        starved: &mut bool,
    ) -> (f32, f32) {
        self.read_playhead_with_wrap(step, false, stem_gains, starved)
    }

    fn read_playhead_with_wrap(
        &self,
        step: f64,
        looped: bool,
        stem_gains: &StemGains,
        starved: &mut bool,
    ) -> (f32, f32) {
        match &self.track {
            Some(track) => {
                let wrap_frames = looped
                    .then(|| track.total_frames())
                    .flatten()
                    .filter(|frames| *frames > 0);
                self.kernel.read_source(
                    track,
                    self.reads_stems,
                    self.position,
                    step,
                    wrap_frames,
                    stem_gains,
                    starved,
                )
            }
            None => (0.0, 0.0),
        }
    }

    /// Mixes the next frame of every tail and moves them on.
    #[inline]
    pub(crate) fn next_tails(
        &mut self,
        stem_gains: &StemGains,
        retire: &mut RetireQueue,
        starved: &mut bool,
    ) -> (f32, f32) {
        let mut left_sum = 0.0;
        let mut right_sum = 0.0;
        for slot in &mut self.tails {
            let Some(tail) = slot else {
                continue;
            };
            let tail_gain = tail.gain(self.declick_frames);
            let (left, right) = self.kernel.read_source(
                &tail.track,
                tail.reads_stems,
                tail.position,
                tail.step,
                tail.wrap_frames,
                stem_gains,
                starved,
            );
            left_sum += left * tail_gain;
            right_sum += right * tail_gain;
            if tail.advance()
                && let Some(finished) = slot.take()
            {
                retire.track(finished.track);
            }
        }
        (left_sum, right_sum)
    }
}

impl Tail {
    /// Moves the tail on by a frame. Returns whether it has faded out.
    #[inline]
    fn advance(&mut self) -> bool {
        self.position += self.step;
        self.remaining_frames -= 1;
        self.remaining_frames == 0
    }
}

/// A rate-aware windowed-sinc interpolator, shared by decks, tails, sampler and key-lock
/// feeders. Normal playback uses 64 taps; downsampling above 2x fades to 128 taps by 3x,
/// narrowing the transition band for high-rate files and fast platter motion. The Blackman
/// window limits ringing; the cutoff falls smoothly as the
/// source advances faster than one frame per output frame, leaving a transition band below
/// the new Nyquist frequency. At unity rate integer frames pass through exactly.
///
/// The phase/cutoff coefficient bank is initialized by `Voice::new`, never by a read: the
/// callback performs no trigonometry, locking or allocation. Bilinear interpolation avoids
/// zipper noise when the platter or sample-rate ratio changes. All voices share ~12 MiB.
/// See Julius O. Smith, "Windowed Sinc Interpolation", Physical Audio Signal Processing.
struct ResamplingKernel {
    normal: CoefficientBank<64>,
    wide: CoefficientBank<128>,
}

const PHASES: usize = 256;
const CUTOFFS: usize = 64;

impl ResamplingKernel {
    fn shared() -> &'static Self {
        static KERNEL: OnceLock<ResamplingKernel> = OnceLock::new();
        KERNEL.get_or_init(|| Self {
            normal: CoefficientBank::new(),
            wide: CoefficientBank::new(),
        })
    }

    #[cfg(test)]
    fn read(&self, track: &TrackBuffer, position: f64, step: f64) -> (f32, f32, bool) {
        self.read_wrapped(track, position, step, None)
    }

    /// The frame of `track` at `position`: the track itself, or, when `reads_stems` and the
    /// track has stems, its stems mixed at `stem_gains` with what they left out. Every gain
    /// full plays the track itself without reading the stems.
    #[inline]
    #[allow(clippy::too_many_arguments)]
    fn read_source(
        &self,
        track: &TrackBuffer,
        reads_stems: bool,
        position: f64,
        step: f64,
        wrap_frames: Option<usize>,
        stem_gains: &StemGains,
        starved: &mut bool,
    ) -> (f32, f32) {
        let (left, right, missing) = self.read_wrapped(track, position, step, wrap_frames);
        *starved |= missing;
        match track.stems() {
            Some(stems) if reads_stems && !all_full(stem_gains) => {
                let mut stem_frame = [0.0; PART_CHANNELS - 2];
                for stem in Stem::ALL {
                    let (stem_left, stem_right, _) =
                        self.read_wrapped(&stems.channel(stem), position, step, wrap_frames);
                    stem_frame[stem.index() * 2] = stem_left;
                    stem_frame[stem.index() * 2 + 1] = stem_right;
                }
                mix_parts((left, right), &stem_frame, stem_gains)
            }
            _ => (left, right),
        }
    }

    /// Every stem of `track` at `position`, then the track itself; silent stems when it has
    /// none.
    #[inline]
    fn read_parts(
        &self,
        track: &TrackBuffer,
        position: f64,
        step: f64,
        starved: &mut bool,
    ) -> PartFrame {
        let mut frame = [0.0; PART_CHANNELS];
        if let Some(stems) = track.stems() {
            for stem in Stem::ALL {
                let (left, right, _) =
                    self.read_wrapped(&stems.channel(stem), position, step, None);
                frame[stem.index() * 2] = left;
                frame[stem.index() * 2 + 1] = right;
            }
        }
        let (left, right, missing) = self.read_wrapped(track, position, step, None);
        *starved |= missing;
        frame[PART_CHANNELS - 2] = left;
        frame[PART_CHANNELS - 1] = right;
        frame
    }

    fn read_wrapped(
        &self,
        track: &impl StereoFrames,
        position: f64,
        step: f64,
        wrap_frames: Option<usize>,
    ) -> (f32, f32, bool) {
        let speed = step.abs();
        if speed <= 2.0 {
            return self.normal.read(track, position, step, wrap_frames);
        }
        if speed >= 3.0 {
            return self.wide.read(track, position, step, wrap_frames);
        }
        let (left, right, missing) = self.normal.read(track, position, step, wrap_frames);
        let (wide_left, wide_right, wide_missing) =
            self.wide.read(track, position, step, wrap_frames);
        let fraction = (speed - 2.0) as f32;
        let mix = fraction * fraction * (3.0 - 2.0 * fraction);
        (
            left + (wide_left - left) * mix,
            right + (wide_right - right) * mix,
            missing || wide_missing,
        )
    }
}

struct CoefficientBank<const TAPS: usize> {
    coefficients: Vec<[f32; TAPS]>,
}

impl<const TAPS: usize> CoefficientBank<TAPS> {
    fn new() -> Self {
        let coefficients = (0..=CUTOFFS)
            .flat_map(|cutoff_index| {
                let cutoff = cutoff_index as f64 / CUTOFFS as f64;
                (0..=PHASES).map(move |phase_index| {
                    let fraction = phase_index as f64 / PHASES as f64;
                    let mut weights = std::array::from_fn(|index| {
                        let distance = index as f64 + 1.0 - (TAPS / 2) as f64 - fraction;
                        let angle = std::f64::consts::PI * distance * cutoff;
                        let sinc = if angle.abs() < 1e-12 {
                            1.0
                        } else {
                            angle.sin() / angle
                        };
                        let phase = std::f64::consts::PI * distance / (TAPS / 2) as f64;
                        (sinc * (0.42 + 0.5 * phase.cos() + 0.08 * (2.0 * phase).cos())) as f32
                    });
                    let sum: f32 = weights.iter().sum();
                    for weight in &mut weights {
                        *weight /= sum;
                    }
                    weights
                })
            })
            .collect();
        Self { coefficients }
    }

    fn read(
        &self,
        track: &impl StereoFrames,
        position: f64,
        step: f64,
        wrap_frames: Option<usize>,
    ) -> (f32, f32, bool) {
        let base = position.floor();
        let fraction = (position - base) as f32;
        let base_index = base as i64;
        // Preserve the exact, inexpensive path for unpitched matching-rate audio and for
        // integer-frame reads while upsampling. Reverse playback uses the same bandwidth.
        if step.abs() <= 1.0 && fraction == 0.0 {
            return match track.read_frame(base_index) {
                FrameRead::Frame(left, right) => (left, right, false),
                FrameRead::Outside => (0.0, 0.0, false),
                FrameRead::Missing => (0.0, 0.0, true),
            };
        }
        let cutoff = (1.0 / (1.0 + 1.1 * (step.abs() - 1.0).max(0.0))) as f32;
        let phase = fraction * PHASES as f32;
        let phase_index = (phase as usize).min(PHASES - 1);
        let phase_mix = phase - phase_index as f32;
        let cutoff_position = cutoff * CUTOFFS as f32;
        let cutoff_index = (cutoff_position as usize).min(CUTOFFS - 1);
        let cutoff_mix = cutoff_position - cutoff_index as f32;
        let first = cutoff_index * (PHASES + 1) + phase_index;
        let low_before = &self.coefficients[first];
        let low_after = &self.coefficients[first + 1];
        let high_before = &self.coefficients[first + PHASES + 1];
        let high_after = &self.coefficients[first + PHASES + 2];
        let mut left = 0.0;
        let mut right = 0.0;
        let first_frame = base_index.saturating_add(1 - (TAPS / 2) as i64);
        let mut accumulate = |index: usize, sample_left: f32, sample_right: f32| {
            let low = low_before[index] + (low_after[index] - low_before[index]) * phase_mix;
            let high = high_before[index] + (high_after[index] - high_before[index]) * phase_mix;
            let weight = low + (high - low) * cutoff_mix;
            left += sample_left * weight;
            right += sample_right * weight;
        };
        let missing = match wrap_frames.filter(|frames| {
            first_frame < 0 || first_frame.saturating_add(TAPS as i64) > *frames as i64
        }) {
            Some(frames) => {
                let mut missing = false;
                for index in 0..TAPS {
                    let wrapped = first_frame
                        .saturating_add(index as i64)
                        .rem_euclid(frames as i64);
                    match track.read_frame(wrapped) {
                        FrameRead::Frame(left, right) => accumulate(index, left, right),
                        FrameRead::Missing => missing = true,
                        FrameRead::Outside => {}
                    }
                }
                missing
            }
            None => track.visit_frames::<TAPS>(first_frame, accumulate),
        };
        (left, right, missing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::Track;

    fn tone(frequency: f64) -> Track {
        let samples: Vec<_> = (0..48_000)
            .flat_map(|frame| {
                let value =
                    (std::f64::consts::TAU * frequency * frame as f64 / 48_000.0).sin() as f32;
                [value, value]
            })
            .collect();
        Track::from_interleaved_stereo(&samples, 48_000).unwrap()
    }

    fn rms_at_rate(track: &Track, step: f64) -> f32 {
        rms_at_rate_offset(track, step, 0.0)
    }

    fn rms_at_rate_offset(track: &Track, step: f64, fraction: f64) -> f32 {
        let kernel = ResamplingKernel::shared();
        let power: f32 = (0..4_000)
            .map(|index| {
                let position =
                    if step > 0.0 { 1_000.0 } else { 40_000.0 } + index as f64 * step + fraction;
                let (left, right, missing) = kernel.read(track.buffer(), position, step);
                assert!(!missing);
                assert_eq!(left, right);
                left * left
            })
            .sum();
        (power / 4_000.0).sqrt()
    }

    #[test]
    fn speeding_up_suppresses_out_of_band_audio_in_both_directions() {
        for step in [1.5, 2.0, 4.0, -1.5, -2.0, -4.0] {
            assert!(rms_at_rate(&tone(20_000.0), step) < 0.001, "rate {step}");
            let passband = rms_at_rate(&tone(1_000.0), step);
            assert!(
                (passband - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.002,
                "rate {step}, RMS {passband}"
            );
        }
    }

    #[test]
    fn high_rate_files_reject_aliases_in_the_audible_band() {
        // At 192k -> 48k, source 28k aliases to output 20k; at 384k -> 48k,
        // source 32k aliases to output 16k. Frequencies below are normalized to 48k.
        for (step, frequency) in [(4.0, 7_000.0), (8.0, 4_000.0), (-8.0, 4_000.0)] {
            let track = tone(frequency);
            for fraction in [0.0, 0.125, 0.5, 0.875] {
                let rms = rms_at_rate_offset(&track, step, fraction);
                let attenuation_db = 20.0 * (rms / std::f32::consts::FRAC_1_SQRT_2).log10();
                assert!(
                    attenuation_db < -80.0,
                    "rate {step}, phase {fraction}: {attenuation_db}dB"
                );
            }
        }
    }

    #[test]
    fn widening_the_filter_is_continuous_and_preserves_dc() {
        let kernel = ResamplingKernel::shared();
        let track = tone(10_000.0);
        for boundary in [2.0, 3.0, -2.0, -3.0] {
            let (before, _, _) = kernel.read(track.buffer(), 1_000.375, boundary - 1e-6);
            let (after, _, _) = kernel.read(track.buffer(), 1_000.375, boundary + 1e-6);
            assert!((after - before).abs() < 1e-4, "boundary {boundary}");
        }
        let dc = Track::from_interleaved_stereo(&[0.3; 2_000], 48_000).unwrap();
        for index in 0..=100 {
            let step = 2.0 + index as f64 / 100.0;
            let (left, right, missing) = kernel.read(dc.buffer(), 100.375, step);
            assert!((left - 0.3).abs() < 1e-6);
            assert_eq!(left, right);
            assert!(!missing);
        }
    }

    #[test]
    fn sampler_loop_filter_extension_preserves_the_wrap_seam() {
        let kernel = ResamplingKernel::shared();
        // Include loops shorter than the filter so the extension may wrap multiple times.
        for frames in [8, 1_000] {
            let track = Track::from_interleaved_stereo(&vec![0.3; frames * 2], 48_000).unwrap();
            for step in [0.91875, 1.5, 2.0, 4.0, 8.0, -8.0] {
                for position in [0.0, 0.375, frames as f64 - 0.625] {
                    let (left, right, missing) =
                        kernel.read_wrapped(track.buffer().as_ref(), position, step, Some(frames));
                    assert!(
                        (left - 0.3).abs() < 1e-6,
                        "length {frames}, position {position}, rate {step}: {left}"
                    );
                    assert_eq!(left, right);
                    assert!(!missing);
                }
            }
        }
    }

    #[test]
    fn matching_rate_integer_frames_are_unchanged() {
        let track = tone(20_000.0);
        let kernel = ResamplingKernel::shared();
        for index in 0..1_000 {
            let expected = track.frame(index).unwrap();
            assert_eq!(
                kernel.read(track.buffer(), index as f64, 1.0),
                (expected[0], expected[1], false)
            );
        }
    }

    #[test]
    #[ignore = "manual optimized-build resampler throughput measurement"]
    fn resampler_throughput() {
        let track = tone(1_000.0);
        let kernel = ResamplingKernel::shared();
        for step in [1.0_f64, 1.01, 2.0, -2.0, 8.0] {
            let started = std::time::Instant::now();
            for frame in 0..480_000 {
                let position = 1_000.0 + (frame as f64 * step.abs()) % 40_000.0;
                std::hint::black_box(kernel.read(track.buffer(), position, step));
            }
            let elapsed = started.elapsed();
            eprintln!(
                "resampler rate {step}: {:.1} ns/output frame, {:.3}% of one core at 48kHz",
                elapsed.as_secs_f64() * 1e9 / 480_000.0,
                elapsed.as_secs_f64() * 10.0
            );
        }
    }

    #[test]
    fn interpolation_preserves_dc_and_handles_track_edges() {
        let track = Track::from_interleaved_stereo(&[0.3; 2_000], 48_000).unwrap();
        let kernel = ResamplingKernel::shared();
        for step in [0.25, 0.99, 1.01, 2.0, 8.0, -8.0] {
            let (left, right, missing) = kernel.read(track.buffer(), 100.375, step);
            assert!((left - 0.3).abs() < 1e-6);
            assert_eq!(left, right);
            assert!(!missing);
            for position in [0.0, 0.25, 999.75, 1_000.0] {
                let (left, right, missing) = kernel.read(track.buffer(), position, step);
                assert!(left.is_finite() && right.is_finite());
                assert!(!missing);
            }
        }
    }
}
