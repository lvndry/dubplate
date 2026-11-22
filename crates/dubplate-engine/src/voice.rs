use std::sync::Arc;

use crate::curves::Ramp;
use crate::mixer::RetireQueue;
use crate::stems::{PART_CHANNELS, PartFrame, Stem, all_full, mix_parts};
use crate::track::{FrameRead, TrackBuffer};

/// The gain of each stem for one output frame, in the order of [`Stem::ALL`].
pub(crate) type StemGains = [f32; Stem::COUNT];

/// Every stem at its level in the track.
pub(crate) const UNITY_STEM_GAINS: StemGains = [1.0; Stem::COUNT];

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
        let gain = self.transport.current();
        self.transport.jump_to(0.0);
        if gain <= 0.0 {
            return;
        }
        let Some(track) = self.track.clone() else {
            return;
        };
        let tail = Tail {
            track,
            reads_stems: self.reads_stems,
            position: self.position,
            step,
            start_gain: gain,
            remaining_frames: self.declick_frames,
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
        let target = self.transport.target();
        self.start_tail(step, retire);
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
        stem_gains: &StemGains,
        retire: &mut RetireQueue,
        starved: &mut bool,
    ) -> (f32, f32) {
        let (left, right) = if gain > 0.0 {
            self.read_playhead(stem_gains, starved)
        } else {
            (0.0, 0.0)
        };
        let (tail_left, tail_right) = self.next_tails(stem_gains, retire, starved);
        (left * gain + tail_left, right * gain + tail_right)
    }

    /// Every stem and the track at the next frame of the playhead and of every tail, unmixed,
    /// and moves the tails on: what key lock stretches when the voice reads stems.
    #[inline]
    pub(crate) fn next_part_frame(
        &mut self,
        retire: &mut RetireQueue,
        starved: &mut bool,
    ) -> PartFrame {
        let mut frame = match &self.track {
            Some(track) => read_parts(track, self.position, starved),
            None => [0.0; PART_CHANNELS],
        };
        for slot in &mut self.tails {
            let Some(tail) = slot else {
                continue;
            };
            let tail_gain = tail.gain(self.declick_frames);
            let tail_frame = read_parts(&tail.track, tail.position, starved);
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
    /// `stem_gains`.
    #[inline]
    pub(crate) fn read_playhead(&self, stem_gains: &StemGains, starved: &mut bool) -> (f32, f32) {
        match &self.track {
            Some(track) => read_source(track, self.reads_stems, self.position, stem_gains, starved),
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
            let (left, right) = read_source(
                &tail.track,
                tail.reads_stems,
                tail.position,
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

/// The frame of `track` at `position`: the track itself, or, when `reads_stems` and the track
/// has stems, its stems mixed at `stem_gains` with what they left out. Every gain full plays the
/// track itself without reading the stems.
#[inline]
fn read_source(
    track: &TrackBuffer,
    reads_stems: bool,
    position: f64,
    stem_gains: &StemGains,
    starved: &mut bool,
) -> (f32, f32) {
    let (left, right, missing) = read_interpolated(track, position);
    *starved |= missing;
    match track.stems() {
        Some(stems) if reads_stems && !all_full(stem_gains) => {
            mix_parts((left, right), &stems.read(position), stem_gains)
        }
        _ => (left, right),
    }
}

/// Every stem of `track` at `position`, then the track itself; silent stems when it has none.
#[inline]
fn read_parts(track: &TrackBuffer, position: f64, starved: &mut bool) -> PartFrame {
    let mut frame = [0.0; PART_CHANNELS];
    if let Some(stems) = track.stems() {
        let stem_frame = stems.read(position);
        frame[..stem_frame.len()].copy_from_slice(&stem_frame);
    }
    let (left, right, missing) = read_interpolated(track, position);
    *starved |= missing;
    frame[PART_CHANNELS - 2] = left;
    frame[PART_CHANNELS - 1] = right;
    frame
}

/// Reads a stereo frame between two decoded frames with Catmull-Rom cubic interpolation, which
/// reproduces straight lines exactly and keeps tempo changes free of the dull, aliased sound of
/// linear interpolation. The flag is true when a frame needed was not decoded yet.
pub(crate) fn read_interpolated(track: &TrackBuffer, position: f64) -> (f32, f32, bool) {
    let base = position.floor();
    let fraction = (position - base) as f32;
    let base_index = base as i64;
    let mut left = [0.0_f32; 4];
    let mut right = [0.0_f32; 4];
    let mut missing = false;
    for offset in 0..4 {
        match track.read_frame(base_index.saturating_add(offset as i64 - 1)) {
            FrameRead::Frame(sample_left, sample_right) => {
                left[offset] = sample_left;
                right[offset] = sample_right;
            }
            FrameRead::Outside => {}
            FrameRead::Missing => missing = true,
        }
    }
    (
        catmull_rom(left, fraction),
        catmull_rom(right, fraction),
        missing,
    )
}

#[inline]
pub(crate) fn catmull_rom(points: [f32; 4], fraction: f32) -> f32 {
    let [before, start, end, after] = points;
    start
        + 0.5
            * fraction
            * (end - before
                + fraction
                    * (2.0 * before - 5.0 * start + 4.0 * end - after
                        + fraction * (3.0 * (start - end) + after - before)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catmull_rom_is_exact_on_straight_lines() {
        let points = [1.0, 2.0, 3.0, 4.0];
        for step in 0..=10 {
            let fraction = step as f32 / 10.0;
            assert!((catmull_rom(points, fraction) - (2.0 + fraction)).abs() < 1e-6);
        }
    }

    #[test]
    fn catmull_rom_passes_through_the_frames() {
        let points = [0.3, -0.7, 0.9, 0.1];
        assert_eq!(catmull_rom(points, 0.0), -0.7);
        assert!((catmull_rom(points, 1.0) - 0.9).abs() < 1e-6);
    }
}
