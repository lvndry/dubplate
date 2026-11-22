//! Loading, playing, cueing and moving the playhead, and the jog wheel.

use std::sync::Arc;

use crate::jog::JogInput;
use crate::retire::RetireQueue;
use crate::track::TrackBuffer;

use super::Deck;

impl Deck {
    /// Puts a track on the deck, stopped at its start, and fades out whatever was playing.
    pub(crate) fn load(&mut self, track: Option<Arc<TrackBuffer>>, retire: &mut RetireQueue) {
        self.voice.start_tail(self.platter.last_step(), retire);
        if let Some(previous) = std::mem::replace(&mut self.voice.track, track) {
            retire.track(previous);
        }
        self.playing = false;
        self.voice.position = 0.0;
        self.cue_point = 0.0;
        self.looping.clear();
        self.slip.clear();
        self.slipping_loop = false;
        self.slipping_touch = false;
        self.roll = None;
        self.pad_effects.release();
        self.key_lock.stop(retire);
        if let Some(previous) = self.beat_grid.take() {
            retire.beat_grid(previous);
        }
        self.sync.multiple = None;
        self.platter.stop();
        self.update_rate();
    }

    pub(crate) fn play(&mut self) {
        if self.voice.track.is_none() || self.at_end() {
            return;
        }
        self.playing = true;
        self.voice.transport.set_target(1.0);
    }

    /// Stops the motor. A hand on the record keeps the music under it.
    pub(crate) fn pause(&mut self, retire: &mut RetireQueue) {
        if self.playing {
            self.playing = false;
            if !self.platter.is_holding() {
                self.voice.start_tail(self.platter.last_step(), retire);
            }
        }
    }

    /// CDJ cue button: while playing, returns to the cue point and stops; while stopped, sets
    /// the cue point where the deck is, on the nearest beat with quantize.
    pub(crate) fn cue(&mut self, retire: &mut RetireQueue) {
        if self.playing {
            self.playing = false;
            self.voice.start_tail(self.platter.last_step(), retire);
            self.voice.position = self.cue_point;
        } else {
            self.cue_point = self.snap_to_beat(self.voice.position);
            self.voice.position = self.cue_point;
        }
    }

    pub(crate) fn set_cue_point(&mut self, seconds: f64) {
        self.cue_point = self.clamp_position(seconds * self.source_rate());
    }

    pub(crate) fn seek(&mut self, seconds: f64, retire: &mut RetireQueue) {
        self.jump_to_frame(seconds * self.source_rate(), retire);
    }

    /// Jumps to a frame of the track, fading the old position out while the new one fades in.
    pub(super) fn jump_to_frame(&mut self, frame: f64, retire: &mut RetireQueue) {
        self.voice.start_tail(self.platter.last_step(), retire);
        self.key_lock.jumped();
        self.voice.position = self.clamp_position(frame);
        if self.playing {
            if self.at_end() {
                self.playing = false;
            } else {
                self.voice.transport.set_target(1.0);
            }
        }
    }

    /// A hot cue: jumps to `seconds` in rhythm while playing; on a stopped deck, jumps there
    /// and starts playing, as on a CDJ.
    pub(crate) fn hot_cue(&mut self, seconds: f64, retire: &mut RetireQueue) {
        let frame = seconds * self.source_rate();
        if self.playing {
            let target = self.in_phase(frame);
            self.jump_to_frame(target, retire);
        } else {
            self.jump_to_frame(frame, retire);
            self.play();
        }
    }

    /// Key lock, the master tempo button: tempo changes keep the track's pitch.
    pub(crate) fn set_key_lock(&mut self, enabled: bool) {
        self.key_lock_enabled = enabled;
    }

    /// Shifts the key by `semitones` through the key lock stretcher, whatever the tempo.
    pub(crate) fn set_key_shift(&mut self, semitones: f32) {
        self.key_shift = semitones;
    }

    pub(crate) fn jog(&mut self, input: JogInput) {
        if self.playing
            && matches!(
                input,
                JogInput::Nudge(_) | JogInput::Scratch(_) | JogInput::Touch(true)
            )
        {
            self.loosen_beat_sync();
        }
        self.platter.apply(input, self.playing);
        match input {
            JogInput::Touch(true)
                if self.slip.enabled && self.playing && self.platter.is_touched() =>
            {
                if !self.slipping_touch {
                    self.slip.begin(self.voice.position);
                    self.slipping_touch = true;
                }
            }
            JogInput::Touch(false) if self.slipping_touch => {
                self.slipping_touch = false;
                self.pending_landing = true;
            }
            _ => {}
        }
    }

    pub(crate) fn set_jog_ticks_per_revolution(&mut self, ticks: u32) {
        self.platter.set_ticks_per_revolution(ticks);
    }

    pub(crate) fn set_vinyl_mode(&mut self, enabled: bool) {
        self.platter.set_vinyl_mode(enabled);
    }
}
