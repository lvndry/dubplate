use std::sync::Arc;

use crate::curves::Ramp;
use crate::mixer::RetireQueue;
use crate::track::TrackBuffer;

/// Fades that can overlap on one deck, for jumps that come faster than a fade lasts.
const MAX_TAILS: usize = 8;

/// A short fade that lets the old playback position die away after a jump, a pause or a load,
/// so the waveform never steps from one value to another between two samples.
struct Tail {
    track: Arc<TrackBuffer>,
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

    /// Mixes the next frame of the playhead at `gain` and of every tail, and moves the tails
    /// on. Sets `starved` when a frame needed was not decoded yet.
    #[inline]
    pub(crate) fn next_frame(
        &mut self,
        gain: f32,
        retire: &mut RetireQueue,
        starved: &mut bool,
    ) -> (f32, f32) {
        let (left, right) = if gain > 0.0 {
            self.read_playhead(starved)
        } else {
            (0.0, 0.0)
        };
        let (tail_left, tail_right) = self.next_tails(retire, starved);
        (left * gain + tail_left, right * gain + tail_right)
    }

    /// The track's frame under the playhead, interpolated.
    #[inline]
    pub(crate) fn read_playhead(&self, starved: &mut bool) -> (f32, f32) {
        match &self.track {
            Some(track) => {
                let (left, right, missing) = read_interpolated(track, self.position);
                *starved |= missing;
                (left, right)
            }
            None => (0.0, 0.0),
        }
    }

    /// Mixes the next frame of every tail and moves them on.
    #[inline]
    pub(crate) fn next_tails(
        &mut self,
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
            let (left, right, missing) = read_interpolated(&tail.track, tail.position);
            *starved |= missing;
            left_sum += left * tail_gain;
            right_sum += right * tail_gain;
            tail.position += tail.step;
            tail.remaining_frames -= 1;
            if tail.remaining_frames == 0
                && let Some(finished) = slot.take()
            {
                retire.track(finished.track);
            }
        }
        (left_sum, right_sum)
    }
}

/// Reads a stereo frame between two decoded frames with Catmull-Rom cubic interpolation, which
/// reproduces straight lines exactly and keeps tempo changes free of the dull, aliased sound of
/// linear interpolation. The flag is true when a frame needed was not decoded yet.
pub(crate) fn read_interpolated(track: &TrackBuffer, position: f64) -> (f32, f32, bool) {
    let base = position.floor();
    let fraction = (position - base) as f32;
    let window = track.read_four((base as i64).saturating_sub(1));
    (
        catmull_rom(window.left, fraction),
        catmull_rom(window.right, fraction),
        window.missing,
    )
}

fn catmull_rom(points: [f32; 4], fraction: f32) -> f32 {
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
