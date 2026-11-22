//! Beat-based actions: quantize, loops, beat jumps, slip, rolls and pad effects.

use dubplate_fx::BeatLength;

use crate::looping::LoopRegion;
use crate::pad_effects::PadEffect;
use crate::retire::RetireQueue;

use super::{Deck, FALLBACK_BEATS_PER_SECOND};

impl Deck {
    pub(crate) fn set_quantize(&mut self, enabled: bool) {
        self.quantize = enabled;
    }

    /// The beat grid's beat at `frame`, or the beat of a steady 120 BPM from the start of the
    /// track when the deck has no grid.
    pub(super) fn beat_at_frame(&self, frame: f64) -> f64 {
        let seconds = frame / self.source_rate();
        match &self.beat_grid {
            Some(grid) => grid.beat_at(seconds),
            None => seconds * FALLBACK_BEATS_PER_SECOND,
        }
    }

    pub(super) fn frame_at_beat(&self, beat: f64) -> f64 {
        let seconds = match &self.beat_grid {
            Some(grid) => grid.seconds_at(beat),
            None => beat / FALLBACK_BEATS_PER_SECOND,
        };
        seconds * self.source_rate()
    }

    pub(super) fn quantizes(&self) -> bool {
        self.quantize && self.beat_grid.is_some()
    }

    /// The nearest beat to `frame` with quantize on and a grid, otherwise `frame` itself.
    pub(super) fn snap_to_beat(&self, frame: f64) -> f64 {
        if self.quantizes() {
            self.clamp_position(self.frame_at_beat(self.beat_at_frame(frame).round()))
        } else {
            frame
        }
    }

    /// Where a jump to `frame` lands. With quantize on and a grid, the deck keeps its place in
    /// the beat, so the music carries on in rhythm: it lands as far past `frame` as the
    /// playhead is past its last beat.
    pub(super) fn in_phase(&self, frame: f64) -> f64 {
        if !self.quantizes() {
            return frame;
        }
        let target_beat = self.beat_at_frame(frame);
        let playing_beat = self.beat_at_frame(self.voice.position);
        self.frame_at_beat(target_beat + (playing_beat - target_beat).rem_euclid(1.0))
    }

    /// Jumps by `beats` beats, forwards or backwards, keeping the place in the beat. A loop
    /// that is on moves with the playhead.
    pub(crate) fn beat_jump(&mut self, beats: f64, retire: &mut RetireQueue) {
        let target = self.frame_at_beat(self.beat_at_frame(self.voice.position) + beats);
        if let Some(region) = self.looping.active_loop() {
            let start = self.frame_at_beat(self.beat_at_frame(region.start) + beats);
            let end = self.frame_at_beat(self.beat_at_frame(region.end) + beats);
            self.looping.region = Some(LoopRegion {
                start,
                end,
                ..region
            });
        }
        self.jump_to_frame(target, retire);
    }

    /// Sets the loop in point where the deck is, on the nearest beat with quantize.
    pub(crate) fn loop_in(&mut self) {
        self.looping.pending_start = Some(self.snap_to_beat(self.voice.position));
    }

    /// Sets the loop out point where the deck is, on the nearest beat with quantize, and turns
    /// the loop on from the loop in point. Without a loop in point it moves the end of the
    /// current loop.
    pub(crate) fn loop_out(&mut self, retire: &mut RetireQueue) {
        if self.looping.pending_start.is_some() {
            self.start_slipping_loop();
        }
        let start = match (self.looping.pending_start.take(), self.looping.region) {
            (Some(start), _) => start,
            (None, Some(region)) => region.start,
            (None, None) => return,
        };
        let mut end = self.snap_to_beat(self.voice.position);
        if end <= start {
            end = self.frame_at_beat(self.beat_at_frame(start) + 1.0);
        }
        self.set_loop(
            LoopRegion {
                start,
                end,
                active: true,
                length: None,
            },
            retire,
        );
    }

    /// A beat loop of `length` from the beat the deck just passed with quantize, or from where
    /// it is without. With a loop on, changes its length from its start instead.
    pub(crate) fn beat_loop(&mut self, length: BeatLength, retire: &mut RetireQueue) {
        self.start_slipping_loop();
        self.start_beat_loop(length, retire);
    }

    pub(super) fn start_beat_loop(&mut self, length: BeatLength, retire: &mut RetireQueue) {
        let start = match self.looping.active_loop() {
            Some(region) => region.start,
            None if self.quantizes() => {
                let grain = length.beats().min(1.0);
                let beat = self.beat_at_frame(self.voice.position);
                self.frame_at_beat((beat / grain).floor() * grain)
            }
            None => self.voice.position,
        };
        let end = self.frame_at_beat(self.beat_at_frame(start) + length.beats());
        self.set_loop(
            LoopRegion {
                start,
                end,
                active: true,
                length: Some(length),
            },
            retire,
        );
    }

    /// Halves the loop from its start, down to a sixteenth of a beat.
    pub(crate) fn halve_loop(&mut self, retire: &mut RetireQueue) {
        self.resize_loop(BeatLength::halved, 0.5, retire);
    }

    /// Doubles the loop from its start, up to thirty-two beats.
    pub(crate) fn double_loop(&mut self, retire: &mut RetireQueue) {
        self.resize_loop(BeatLength::doubled, 2.0, retire);
    }

    pub(super) fn resize_loop(
        &mut self,
        resize: fn(BeatLength) -> Option<BeatLength>,
        factor: f64,
        retire: &mut RetireQueue,
    ) {
        let Some(region) = self.looping.region else {
            return;
        };
        let start_beat = self.beat_at_frame(region.start);
        let (length, beats) = match region.length {
            Some(length) => match resize(length) {
                Some(resized) => (Some(resized), resized.beats()),
                None => return,
            },
            None => (None, (self.beat_at_frame(region.end) - start_beat) * factor),
        };
        let end = self.frame_at_beat(start_beat + beats);
        self.set_loop(
            LoopRegion {
                end,
                length,
                ..region
            },
            retire,
        );
    }

    /// RELOOP/EXIT: leaves the loop that is on, or turns the last loop back on and jumps to
    /// its start, in rhythm with quantize.
    pub(crate) fn reloop_or_exit(&mut self, retire: &mut RetireQueue) {
        let Some(region) = self.looping.region.as_mut() else {
            return;
        };
        region.active = !region.active;
        if !region.active {
            if std::mem::take(&mut self.slipping_loop) {
                self.land(retire);
            }
            return;
        }
        {
            let start = region.start;
            let target = if self.playing {
                self.in_phase(start)
            } else {
                start
            };
            self.jump_to_frame(target, retire);
        }
    }

    /// Puts a loop on the deck and brings a playhead past its end back inside it.
    pub(super) fn set_loop(&mut self, region: LoopRegion, retire: &mut RetireQueue) {
        self.looping.region = Some(region);
        let position = self.voice.position;
        if region.active && position >= region.start && position >= region.end {
            self.jump_to_frame(region.fold(position), retire);
        }
    }

    /// With SLIP on, a loop starts a slip: leaving it lands where the track would have been.
    pub(super) fn start_slipping_loop(&mut self) {
        if self.slip.enabled && !self.slipping_loop && self.roll.is_none() && self.playing {
            self.slip.begin(self.voice.position);
            self.slipping_loop = true;
        }
    }

    /// Lands on the slip shadow when the last slipping action ends.
    pub(super) fn land(&mut self, retire: &mut RetireQueue) {
        if let Some(shadow) = self.slip.end() {
            let target = match self.looping.active_loop() {
                Some(region) => region.fold(shadow),
                None => shadow,
            };
            self.jump_to_frame(target, retire);
        }
    }

    /// A loop roll of `length` while a pad is held: the deck loops on the beat while the track
    /// keeps playing underneath, and `None` lets go, landing in rhythm where the track would
    /// have been. Pressing another length while rolling changes the roll's length.
    pub(crate) fn beat_roll(&mut self, length: Option<BeatLength>, retire: &mut RetireQueue) {
        match length {
            Some(length) => {
                if self.roll.is_none() {
                    self.roll = Some(self.looping.region.take());
                    self.slip.begin(self.voice.position);
                }
                self.start_beat_loop(length, retire);
            }
            None => {
                if let Some(previous) = self.roll.take() {
                    self.looping.region = previous;
                    self.land(retire);
                }
            }
        }
    }

    /// The SLIP button.
    pub(crate) fn set_slip(&mut self, enabled: bool) {
        self.slip.enabled = enabled;
    }

    pub(crate) fn hold_pad_effect(&mut self, pad: PadEffect) {
        self.pad_effects.hold(pad);
    }

    pub(crate) fn release_pad_effect(&mut self) {
        self.pad_effects.release();
    }
}
