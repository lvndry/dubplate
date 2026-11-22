use std::sync::Arc;

use dubplate_fx::{BeatLength, EqBand};

use crate::INSERT_SLOTS;
use crate::channel::ChannelStrip;
use crate::crossfader::ChannelFaderCurve;
use crate::curves::Ramp;
use crate::grid::BeatGrid;
use crate::jog::{BlockStart, JogInput, Platter};
use crate::keylock::{KeyLock, KeyLockBlock};
use crate::looping::{LoopRegion, Looping};
use crate::mixer::RetireQueue;
use crate::pad_effects::{PadEffect, PadEffects};
use crate::processor::{AudioProcessor, ProcessContext, StereoBlock};
use crate::slip::Slip;
use crate::smart_cfx::SmartCfx;
use crate::stems::{StemMix, StemSettings};
use crate::sync::{MasterBeat, PhaseAction, SyncFollow, SyncMode, phase_action, sync_multiple};
use crate::tempo::{BeatClock, TempoFader, TempoRange};
use crate::track::{MAX_TRACK_SECONDS, TrackBuffer};
use crate::voice::Voice;

/// One of the decks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeckId {
    /// Deck A, on the left side of the crossfader.
    A,
    /// Deck B, on the right side of the crossfader.
    B,
}

impl DeckId {
    /// Every deck, in order.
    pub const ALL: [DeckId; crate::DECK_COUNT] = [DeckId::A, DeckId::B];

    /// Position of the deck in [`DeckId::ALL`].
    pub fn index(self) -> usize {
        match self {
            DeckId::A => 0,
            DeckId::B => 1,
        }
    }
}

/// Below this share of its own speed the record is too slow to be heard; the level follows the
/// speed down to silence, as a stylus does, so a record held still is silent instead of
/// holding a constant offset.
const AUDIBLE_SPEED: f64 = 0.05;

/// Key lock leaves a deck within this much of its own tempo to the plain resampler, which is
/// exact there.
const KEY_LOCK_TEMPO_TOLERANCE: f64 = 1e-6;

/// Beats per second loops and beat jumps count on a deck without a beat grid: 120 BPM.
const FALLBACK_BEATS_PER_SECOND: f64 = 2.0;

/// What rendering one block of a deck found.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DeckRender {
    pub(crate) starved: bool,
}

/// Which beat a beat loop that is not already on starts from.
#[derive(Debug, Clone, Copy)]
enum LoopSnap {
    /// The beat nearest to what the DJ hears, as a beat loop on a CDJ.
    Nearest,
    /// The beat just passed, so a loop roll repeats what was just heard.
    Previous,
}

/// The audio-thread side of a deck.
pub(crate) struct Deck {
    voice: Voice,
    tempo: f64,
    rate: f64,
    playing: bool,
    cue_point: f64,
    looping: Looping,
    quantize: bool,
    slip: Slip,
    slipping_loop: bool,
    slipping_touch: bool,
    roll: Option<Option<LoopRegion>>,
    pad_effects: PadEffects,
    key_shift: f32,
    pending_landing: bool,
    smart_tempo: bool,
    beat_grid: Option<BeatGrid>,
    fader: TempoFader,
    sync: SyncFollow,
    phase_bend: f64,
    trim: Ramp,
    channel_gain: Ramp,
    fader_position: f32,
    platter: Platter,
    steps: Vec<f64>,
    key_lock: KeyLock,
    key_lock_enabled: bool,
    stem_mix: StemMix,
    strip: ChannelStrip,
    pub(crate) inserts: [Option<Box<dyn AudioProcessor>>; INSERT_SLOTS],
    pub(crate) block: StereoBlock,
    output_rate: f64,
    output_lag_frames: f64,
}

impl Deck {
    pub(crate) fn new(output_rate: u32, max_block_frames: usize, declick_frames: u32) -> Self {
        Self {
            voice: Voice::new(declick_frames),
            tempo: 1.0,
            rate: 1.0,
            playing: false,
            cue_point: 0.0,
            looping: Looping::default(),
            quantize: true,
            slip: Slip::default(),
            slipping_loop: false,
            slipping_touch: false,
            roll: None,
            pad_effects: PadEffects::new(output_rate),
            key_shift: 0.0,
            pending_landing: false,
            smart_tempo: false,
            beat_grid: None,
            fader: TempoFader::default(),
            sync: SyncFollow::default(),
            phase_bend: 0.0,
            trim: Ramp::new(1.0, declick_frames),
            channel_gain: Ramp::new(1.0, declick_frames),
            fader_position: 1.0,
            platter: Platter::new(output_rate),
            steps: vec![0.0; max_block_frames],
            key_lock: KeyLock::with_stems(output_rate, declick_frames),
            key_lock_enabled: false,
            stem_mix: StemMix::new(output_rate),
            strip: ChannelStrip::new(output_rate),
            inserts: std::array::from_fn(|_| None),
            block: StereoBlock::with_capacity(max_block_frames),
            output_rate: f64::from(output_rate.max(1)),
            output_lag_frames: 0.0,
        }
    }

    pub(crate) fn is_loaded(&self) -> bool {
        self.voice.track.is_some()
    }

    pub(crate) fn is_playing(&self) -> bool {
        self.playing
    }

    pub(crate) fn tempo(&self) -> f64 {
        self.tempo
    }

    /// The speed the record turns at, where 1.0 is the track's own tempo: the tempo, bent by a
    /// nudge, or the hand's speed while scratching.
    pub(crate) fn speed(&self) -> f64 {
        self.platter.last_step() * self.output_rate / self.source_rate()
    }

    pub(crate) fn is_touched(&self) -> bool {
        self.platter.is_touched()
    }

    pub(crate) fn vinyl_mode(&self) -> bool {
        self.platter.vinyl_mode()
    }

    pub(crate) fn mixer_latency_frames(&self) -> usize {
        self.strip.latency_frames()
    }

    pub(crate) fn fader_position(&self) -> f32 {
        self.fader_position
    }

    pub(crate) fn has_beat_grid(&self) -> bool {
        self.beat_grid.is_some()
    }

    pub(crate) fn set_beat_grid(&mut self, beat_grid: Option<BeatGrid>, retire: &mut RetireQueue) {
        if let Some(previous) = std::mem::replace(&mut self.beat_grid, beat_grid) {
            retire.beat_grid(previous);
        }
        self.sync.multiple = None;
    }

    /// The tempo as it plays, in beats per minute, when the deck has a beat grid.
    pub(crate) fn bpm(&self) -> Option<f64> {
        self.beat_grid
            .as_ref()
            .map(|grid| grid.bpm_at(self.position_seconds()) * self.tempo)
    }

    /// The tempo and beat position at the next frame the deck plays, when it has a beat grid.
    pub(crate) fn beat_clock(&self) -> Option<BeatClock> {
        self.beat_grid.as_ref().map(|grid| BeatClock {
            bpm: grid.bpm_at(self.position_seconds()) * self.tempo,
            beat_position: grid.beat_at(self.position_seconds()),
        })
    }

    /// The deck's gain to the master while it plays, 0 when it is stopped. Counts the trim,
    /// the channel fader on `curve` and the crossfader gain `crossfader_gain`, the same way the
    /// louder-deck rule hears the deck. The channel fader is not read from `channel_gain`
    /// because that gain already carries the crossfader.
    pub(crate) fn level(&self, curve: ChannelFaderCurve, crossfader_gain: f32) -> f32 {
        if self.playing {
            self.trim.target() * curve.gain(self.fader_position()) * crossfader_gain
        } else {
            0.0
        }
    }

    fn source_rate(&self) -> f64 {
        self.voice
            .track
            .as_ref()
            .map_or(self.output_rate, |track| f64::from(track.sample_rate()))
    }

    pub(crate) fn position_seconds(&self) -> f64 {
        self.voice.position / self.source_rate()
    }

    pub(crate) fn cue_point_seconds(&self) -> f64 {
        self.cue_point / self.source_rate()
    }

    pub(crate) fn playback_snapshot(&self) -> crate::controller::DeckPlayback {
        crate::controller::DeckPlayback {
            position_seconds: self.position_seconds(),
            cue_seconds: self.cue_point_seconds(),
            tempo: self.tempo,
            fader_position: self.fader.position(),
            range: self.fader.range(),
            waits_for_pickup: self.fader_waits_for_pickup(),
            sync: self.sync.mode,
            key_lock: self.key_lock_enabled,
            key_shift: self.key_shift,
            slip: self.slip.enabled,
            vinyl_mode: self.platter.vinyl_mode(),
            playing: self.playing,
        }
    }

    pub(crate) fn track_handle(&self) -> Option<Arc<TrackBuffer>> {
        self.voice.track.clone()
    }

    pub(crate) fn grid_handle(&self) -> Option<BeatGrid> {
        self.beat_grid.clone()
    }

    fn update_rate(&mut self) {
        self.rate = self.tempo * (1.0 + self.phase_bend) * self.source_rate() / self.output_rate;
        self.platter.set_track_rate(self.source_rate());
    }

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
        self.stem_mix.settle();
        self.platter.stop();
        self.update_rate();
    }

    /// Sets the level of each stem and which are muted. Each stem glides to its new gain over
    /// 20 ms; a track without stems plays as it is until they are attached.
    pub(crate) fn set_stem_settings(&mut self, settings: StemSettings) {
        self.stem_mix.set(settings);
    }

    /// Plays the track's stems instead of the track once they are attached, with a crossfade.
    /// While key lock stretches an untouched track, the switch waits for the stretcher to
    /// start again anyway, at the next jump, pause or stem move, so the stretched sound never
    /// restarts for nothing.
    fn engage_stems(&mut self, retire: &mut RetireQueue) {
        let has_stems = self
            .voice
            .track
            .as_ref()
            .is_some_and(|track| track.stems().is_some());
        if has_stems == self.voice.reads_stems() {
            return;
        }
        let stretcher_busy = self.key_lock.is_engaged() && self.voice.transport.current() > 0.0;
        if has_stems && stretcher_busy && self.stem_mix.settings().is_untouched() {
            return;
        }
        self.voice
            .set_reads_stems(has_stems, self.platter.last_step(), retire);
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
    /// the cue point where the deck is.
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

    /// Key lock, the master tempo button: tempo changes keep the track's pitch.
    pub(crate) fn set_key_lock(&mut self, enabled: bool) {
        self.key_lock_enabled = enabled;
    }

    pub(crate) fn set_quantize(&mut self, enabled: bool) {
        self.quantize = enabled;
    }

    /// The beat grid's beat at `frame`, or the beat of a steady 120 BPM from the start of the
    /// track when the deck has no grid.
    fn beat_at_frame(&self, frame: f64) -> f64 {
        let seconds = frame / self.source_rate();
        match &self.beat_grid {
            Some(grid) => grid.beat_at(seconds),
            None => seconds * FALLBACK_BEATS_PER_SECOND,
        }
    }

    fn frame_at_beat(&self, beat: f64) -> f64 {
        let seconds = match &self.beat_grid {
            Some(grid) => grid.seconds_at(beat),
            None => beat / FALLBACK_BEATS_PER_SECOND,
        };
        seconds * self.source_rate()
    }

    fn quantizes(&self) -> bool {
        self.quantize && self.beat_grid.is_some()
    }

    /// How many output frames the sound card and the mixer still hold of what the deck has
    /// rendered: the delay between the playhead and what the DJ hears.
    pub(crate) fn set_output_lag(&mut self, frames: f64) {
        self.output_lag_frames = frames.max(0.0);
    }

    /// The frame of the track the DJ hears now, which is behind the playhead by the output lag.
    fn heard_position(&self) -> f64 {
        if !self.playing {
            return self.voice.position;
        }
        self.clamp_position(self.voice.position - self.output_lag_frames * self.platter.last_step())
    }

    /// The nearest beat to `frame` with quantize on and a grid, otherwise `frame` itself.
    fn snap_to_beat(&self, frame: f64) -> f64 {
        self.snap_to_grain(frame, 1.0)
    }

    /// The last multiple of `grain` beats at or before `frame` with quantize on and a grid,
    /// otherwise `frame` itself.
    fn previous_grain(&self, frame: f64, grain: f64) -> f64 {
        if self.quantizes() {
            let beat = (self.beat_at_frame(frame) / grain).floor() * grain;
            self.clamp_position(self.frame_at_beat(beat))
        } else {
            frame
        }
    }

    /// The nearest multiple of `grain` beats to `frame` with quantize on and a grid, otherwise
    /// `frame` itself.
    fn snap_to_grain(&self, frame: f64, grain: f64) -> f64 {
        if self.quantizes() {
            let beat = (self.beat_at_frame(frame) / grain).round() * grain;
            self.clamp_position(self.frame_at_beat(beat))
        } else {
            frame
        }
    }

    /// Where a jump to `frame` lands. With quantize on and a grid, the deck keeps its place in
    /// the beat, so the music carries on in rhythm: it lands as far past `frame` as the
    /// playhead is past its last beat.
    fn in_phase(&self, frame: f64) -> f64 {
        if !self.quantizes() {
            return frame;
        }
        let target_beat = self.beat_at_frame(frame);
        let playing_beat = self.beat_at_frame(self.voice.position);
        self.frame_at_beat(target_beat + (playing_beat - target_beat).rem_euclid(1.0))
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

    /// A hot loop: the loop from `start_seconds` to `end_seconds` goes on and the deck goes to
    /// its start as a hot cue does, in rhythm while playing and playing from it when stopped.
    pub(crate) fn hot_loop(
        &mut self,
        start_seconds: f64,
        end_seconds: f64,
        retire: &mut RetireQueue,
    ) {
        let rate = self.source_rate();
        let region = LoopRegion {
            start: self.clamp_position(start_seconds * rate),
            end: self.clamp_position(end_seconds * rate),
            active: true,
            length: None,
        };
        if region.length_frames() <= 0.0 {
            self.hot_cue(start_seconds, retire);
            return;
        }
        self.start_slipping_loop();
        self.looping.pending_start = None;
        self.looping.region = Some(region);
        if self.playing {
            let target = region.fold(self.in_phase(region.start));
            self.jump_to_frame(target, retire);
        } else {
            self.jump_to_frame(region.start, retire);
            self.play();
        }
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
        self.looping.pending_start = Some(self.snap_to_beat(self.heard_position()));
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
        let mut end = self.snap_to_beat(self.heard_position());
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

    /// A beat loop of `length` from the beat nearest to what the DJ hears with quantize, which
    /// may be just ahead of the playhead, or from exactly there without. With a loop on, changes its length from its start instead.
    pub(crate) fn beat_loop(&mut self, length: BeatLength, retire: &mut RetireQueue) {
        self.start_slipping_loop();
        self.start_beat_loop(length, LoopSnap::Nearest, retire);
    }

    fn start_beat_loop(&mut self, length: BeatLength, snap: LoopSnap, retire: &mut RetireQueue) {
        let start = match self.looping.active_loop() {
            Some(region) => region.start,
            None => {
                let grain = length.beats().min(1.0);
                let heard = self.heard_position();
                match snap {
                    LoopSnap::Nearest => self.snap_to_grain(heard, grain),
                    LoopSnap::Previous => self.previous_grain(heard, grain),
                }
            }
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

    fn resize_loop(
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
            self.start_slipping_loop();
            let target = if self.playing {
                self.in_phase(start)
            } else {
                start
            };
            self.jump_to_frame(target, retire);
        }
    }

    /// With SLIP on, a loop starts a slip: leaving it lands where the track would have been.
    fn start_slipping_loop(&mut self) {
        if self.slip.enabled && !self.slipping_loop && self.roll.is_none() && self.playing {
            self.slip.begin(self.voice.position);
            self.slipping_loop = true;
        }
    }

    /// Lands on the slip shadow when the last slipping action ends.
    fn land(&mut self, retire: &mut RetireQueue) {
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
                self.start_beat_loop(length, LoopSnap::Previous, retire);
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

    pub(crate) fn slip_enabled(&self) -> bool {
        self.slip.enabled
    }

    pub(crate) fn slip_shadow(&self) -> Option<f64> {
        self.slip.shadow()
    }

    pub(crate) fn hold_pad_effect(&mut self, pad: PadEffect) {
        self.pad_effects.hold(pad);
    }

    pub(crate) fn release_pad_effect(&mut self) {
        self.pad_effects.release();
    }

    /// Shifts the key by `semitones` through the key lock stretcher, whatever the tempo.
    pub(crate) fn set_key_shift(&mut self, semitones: f32) {
        self.key_shift = semitones;
    }

    /// Puts a loop on the deck and brings a playhead past its end back inside it.
    fn set_loop(&mut self, region: LoopRegion, retire: &mut RetireQueue) {
        self.looping.region = Some(region);
        let position = self.voice.position;
        if region.active && position >= region.start && position >= region.end {
            self.jump_to_frame(region.fold(position), retire);
        }
    }

    pub(crate) fn loop_region(&self) -> Option<LoopRegion> {
        self.looping.region
    }

    pub(crate) fn pending_loop_start(&self) -> Option<f64> {
        self.looping.pending_start
    }

    pub(crate) fn frames_to_seconds(&self, frames: f64) -> f64 {
        frames / self.source_rate()
    }

    pub(crate) fn set_cue_point(&mut self, seconds: f64) {
        self.cue_point = self.clamp_position(seconds * self.source_rate());
    }

    pub(crate) fn seek(&mut self, seconds: f64, retire: &mut RetireQueue) {
        self.jump_to_frame(seconds * self.source_rate(), retire);
    }

    /// Jumps to a frame of the track, fading the old position out while the new one fades in.
    fn jump_to_frame(&mut self, frame: f64, retire: &mut RetireQueue) {
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

    fn apply_tempo(&mut self, tempo: f64) {
        self.tempo = tempo;
        self.phase_bend = 0.0;
        self.update_rate();
    }

    /// Restores effective tempo separately from the physical fader and its pickup state.
    pub(crate) fn restore_tempo(
        &mut self,
        tempo: f64,
        position: f64,
        range: TempoRange,
        waits_for_pickup: bool,
    ) {
        self.stop_following();
        self.fader = TempoFader::restored(position, range, waits_for_pickup);
        self.apply_tempo(tempo);
    }

    pub(crate) fn tempo_fader_position(&self) -> f64 {
        self.fader.position()
    }

    /// Sets the tempo the fader asks for directly, where 1.0 is the track's own tempo.
    pub(crate) fn set_tempo(&mut self, tempo: f64) {
        if let Some(tempo) = self.fader.move_to_tempo(tempo, self.tempo) {
            self.play_fader_tempo(tempo);
        }
    }

    /// Moves the tempo fader, from -1 (slowest) to 1 (fastest).
    pub(crate) fn set_tempo_fader(&mut self, position: f64) {
        if let Some(tempo) = self.fader.move_to(position, self.tempo) {
            self.play_fader_tempo(tempo);
        }
    }

    pub(crate) fn set_tempo_range(&mut self, range: TempoRange) {
        if let Some(tempo) = self.fader.set_range(range) {
            self.play_fader_tempo(tempo);
        }
    }

    fn play_fader_tempo(&mut self, tempo: f64) {
        if !self.sync.following && !self.smart_tempo {
            self.apply_tempo(tempo);
        }
    }

    pub(crate) fn tempo_range(&self) -> TempoRange {
        self.fader.range()
    }

    /// Whether the tempo fader waits to be moved through the playing tempo before it takes
    /// over again.
    pub(crate) fn fader_waits_for_pickup(&self) -> bool {
        !self.fader.is_in_control()
    }

    pub(crate) fn set_pitch_bend(&mut self, bend: f64) {
        if bend != 0.0 {
            self.loosen_beat_sync();
        }
        self.platter.set_held_bend(bend);
    }

    pub(crate) fn sync_mode(&self) -> SyncMode {
        self.sync.mode
    }

    pub(crate) fn is_following(&self) -> bool {
        self.sync.following
    }

    pub(crate) fn set_sync(&mut self, mode: SyncMode) {
        self.sync.mode = mode;
        self.sync.multiple = None;
        self.sync.locked = false;
    }

    /// A hand on a following deck drops beat sync to tempo sync, as on a CDJ-3000.
    fn loosen_beat_sync(&mut self) {
        if self.sync.following && self.sync.mode == SyncMode::Beat {
            self.sync.mode = SyncMode::Tempo;
        }
    }

    /// What a following deck needs from this deck as the tempo master.
    pub(crate) fn master_beat(&self, deck: DeckId) -> Option<MasterBeat> {
        let grid = self.beat_grid.as_ref()?;
        let seconds = self.position_seconds();
        Some(MasterBeat {
            deck,
            bpm: grid.bpm_at(seconds) * self.tempo,
            beat: grid.beat_at(seconds),
            running: self.playing && !self.platter.is_holding(),
        })
    }

    /// Stops following the tempo master; the tempo fader takes over once it reaches the
    /// playing tempo.
    pub(crate) fn stop_following(&mut self) {
        self.sync.locked = false;
        if self.sync.following {
            self.sync.following = false;
            self.phase_bend = 0.0;
            self.update_rate();
            self.fader.take_back(self.tempo);
        }
    }

    /// Follows the tempo master for the next block: its tempo, and with beat sync its beats.
    pub(crate) fn follow(
        &mut self,
        master: MasterBeat,
        allow_half_and_double: bool,
        force_beat_lock: bool,
        retire: &mut RetireQueue,
    ) {
        let Some(grid) = &self.beat_grid else {
            self.stop_following();
            return;
        };
        if !self.sync.following {
            self.sync.following = true;
            self.fader.hand_over();
        }
        let seconds = self.position_seconds();
        let own_bpm = grid.bpm_at(seconds);
        let multiple = match self.sync.multiple {
            Some((deck, multiple)) if deck == master.deck => multiple,
            _ => {
                let multiple =
                    sync_multiple(master.bpm, own_bpm * self.tempo, allow_half_and_double);
                self.sync.multiple = Some((master.deck, multiple));
                multiple
            }
        };
        let tempo = master.bpm * multiple / own_bpm;
        let lock_beats = (self.sync.mode == SyncMode::Beat || force_beat_lock)
            && master.running
            && self.playing
            && !self.platter.is_holding()
            && self.slip.shadow().is_none();
        let mut phase_bend = 0.0;
        if lock_beats {
            let beat = grid.beat_at(seconds);
            let beats_per_second = own_bpm * tempo / 60.0;
            match phase_action(
                master.beat * multiple,
                beat,
                beats_per_second,
                self.sync.locked,
            ) {
                PhaseAction::Jump(beats) => {
                    let mut target = grid.seconds_at(beat + beats);
                    if target < 0.0 {
                        target = grid.seconds_at(beat + beats + 1.0);
                    }
                    self.jump_to_frame(target * self.source_rate(), retire);
                }
                PhaseAction::Bend(bend) => phase_bend = bend,
            }
        }
        self.sync.locked = lock_beats;
        self.tempo = tempo;
        self.phase_bend = phase_bend;
        self.update_rate();
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

    pub(crate) fn set_eq(&mut self, band: EqBand, position: f32) {
        self.strip.set_eq(band, position);
    }

    pub(crate) fn set_automatic_low(&mut self, position: Option<f32>) {
        self.strip.set_automatic_low(position);
    }

    /// The tempo at its own speed, in beats per minute, when the deck has a beat grid.
    pub(crate) fn own_bpm(&self) -> Option<f64> {
        self.beat_grid
            .as_ref()
            .map(|grid| grid.bpm_at(self.position_seconds()))
    }

    /// Plays at `bpm` for Smart Fader, or gives the tempo back to the fader with `None`; the
    /// fader takes over once moved through the playing tempo.
    pub(crate) fn set_smart_bpm(&mut self, bpm: Option<f64>) {
        match (bpm, self.own_bpm()) {
            (Some(bpm), Some(own)) => {
                if !self.smart_tempo {
                    self.smart_tempo = true;
                    self.fader.hand_over();
                }
                self.apply_tempo(bpm / own);
            }
            _ => {
                if std::mem::take(&mut self.smart_tempo) {
                    self.fader.take_back(self.tempo);
                }
            }
        }
    }

    pub(crate) fn set_filter(&mut self, position: f32) {
        self.strip.set_filter(position);
    }

    pub(crate) fn set_smart_cfx(&mut self, preset: Option<SmartCfx>) {
        self.strip.set_smart_cfx(preset);
    }

    pub(crate) fn set_filter_resonance(&mut self, resonance: f32) {
        self.strip.set_filter_resonance(resonance);
    }

    pub(crate) fn set_trim(&mut self, gain: f32) {
        self.trim.set_target(gain);
    }

    pub(crate) fn set_fader_position(&mut self, position: f32) {
        self.fader_position = position;
    }

    pub(crate) fn set_channel_gain(&mut self, gain: f32) {
        self.channel_gain.set_target(gain);
    }

    /// Moves every gain straight to its target, for setting up a deck before it plays.
    pub(crate) fn settle_gains(&mut self) {
        let channel_gain = self.channel_gain.target();
        self.channel_gain.jump_to(channel_gain);
        let trim = self.trim.target();
        self.trim.jump_to(trim);
    }

    fn clamp_position(&self, position: f64) -> f64 {
        let longest = MAX_TRACK_SECONDS as f64 * self.source_rate();
        let end = self.voice.end_frame().unwrap_or(longest);
        position.clamp(0.0, end)
    }

    fn at_end(&self) -> bool {
        self.voice
            .end_frame()
            .is_some_and(|end| self.voice.position >= end)
    }

    /// Renders `frame_count` frames into the deck's block, after the trim, the EQ, the filter
    /// and the inserts, and before the channel fader.
    pub(crate) fn render(
        &mut self,
        frame_count: usize,
        context: &ProcessContext,
        retire: &mut RetireQueue,
    ) -> DeckRender {
        let mut result = DeckRender::default();
        if let Some(frames) = self.platter.take_search() {
            self.jump_to_frame(self.voice.position + frames, retire);
        }
        let motor_step = if self.playing { self.rate } else { 0.0 };
        self.platter.begin_block(BlockStart {
            motor_step,
            frames: frame_count,
        });
        if std::mem::take(&mut self.pending_landing) {
            self.platter.drop_carry();
            self.land(retire);
        }
        self.engage_stems(retire);
        let holding = self.platter.is_holding();
        let transport_target = if self.playing || holding { 1.0 } else { 0.0 };
        if self.voice.transport.target() != transport_target {
            self.voice.transport.set_target(transport_target);
        }
        self.block.set_frame_count(frame_count);
        let frame_count = self.block.frame_count();
        let mut advance = 0.0;
        for step in &mut self.steps[..frame_count] {
            *step = self.platter.next_step(motor_step);
            advance += *step;
        }
        let unit_step = self.source_rate() / self.output_rate;
        let audible_step = unit_step * AUDIBLE_SPEED;
        let end_frame = self.voice.end_frame();
        let active_loop = self.looping.active_loop();
        self.slip
            .advance(motor_step * frame_count as f64, end_frame);
        let tempo_moved = (self.tempo - 1.0).abs() > KEY_LOCK_TEMPO_TOLERANCE;
        let key_shifted = self.key_shift != 0.0;
        let tempo_semitones = if self.key_lock_enabled {
            0.0
        } else {
            12.0 * self.tempo.log2() as f32
        };
        self.key_lock
            .set_transpose(self.key_shift + tempo_semitones);
        let stretched = self.key_lock.prepare(
            KeyLockBlock {
                wanted: ((self.key_lock_enabled && tempo_moved) || key_shifted)
                    && self.playing
                    && !holding,
                track: self.voice.track.as_ref(),
                playhead: self.voice.position,
                advance,
                frames: frame_count,
                unit_step,
                active_loop,
                stems: self.voice.reads_stems(),
            },
            retire,
            &mut result.starved,
        );
        let fully_stretched = stretched && self.key_lock.is_fully_in();
        let voice = &mut self.voice;
        let key_lock = &mut self.key_lock;
        let stem_mix = &mut self.stem_mix;
        let trim = &mut self.trim;
        let (left_channel, right_channel) = self.block.channels_mut();
        for (index, ((left, right), step)) in left_channel
            .iter_mut()
            .zip(right_channel.iter_mut())
            .zip(&self.steps[..frame_count])
            .enumerate()
        {
            let step = *step;
            let stylus_gain = (step.abs() / audible_step).min(1.0) as f32;
            let gain = voice.transport.next_value() * stylus_gain;
            let stem_gains = stem_mix.next_gains();
            let (playhead_left, playhead_right) = if stretched {
                key_lock.playhead_frame(index, voice, step, &stem_gains, &mut result.starved)
            } else if gain > 0.0 {
                voice.read_playhead(step, &stem_gains, &mut result.starved)
            } else {
                (0.0, 0.0)
            };
            let (tail_left, tail_right) =
                voice.next_tails(&stem_gains, retire, &mut result.starved);
            if step != 0.0 {
                let previous = voice.position;
                voice.position = (previous + step).max(0.0);
                if let Some(wrapped) =
                    active_loop.and_then(|active| active.wrap(previous, voice.position))
                {
                    if fully_stretched {
                        voice.position = wrapped;
                    } else {
                        voice.jump(wrapped, step, retire);
                    }
                } else if let Some(end) = end_frame
                    && voice.position >= end
                {
                    voice.position = end;
                    if self.playing && !holding {
                        self.playing = false;
                        voice.transport.jump_to(0.0);
                    }
                }
            }
            let trim = trim.next_value();
            *left = (playhead_left * gain + tail_left) * trim;
            *right = (playhead_right * gain + tail_right) * trim;
        }
        self.strip.process(&mut self.block, context);
        self.pad_effects.process(&mut self.block, context);
        for processor in self.inserts.iter_mut().flatten() {
            processor.process(&mut self.block, context);
        }
        result
    }

    /// The channel fader and crossfader gain the deck is heading to.
    pub(crate) fn channel_gain(&self) -> f32 {
        self.channel_gain.target()
    }

    /// Applies the channel fader and crossfader gain to the deck's block.
    pub(crate) fn apply_channel_gain(&mut self) {
        let (left_channel, right_channel) = self.block.channels_mut();
        self.channel_gain.apply(left_channel, right_channel);
    }

    /// Adds the deck's block into the master block and returns its peak.
    pub(crate) fn mix_into(&self, master: &mut StereoBlock) -> f32 {
        master.add(&self.block);
        self.block.peak()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_louder_deck_level_counts_the_fader_curve_and_the_crossfader_gain() {
        let track = crate::track::Track::from_interleaved_stereo(&[0.1, 0.1], 48_000).unwrap();
        let (producer, _consumer) = rtrb::RingBuffer::new(16);
        let mut retire = RetireQueue::new(producer);
        let mut deck = Deck::new(48_000, 512, 240);
        let standard = ChannelFaderCurve::Standard;
        deck.load(Some(Arc::clone(track.buffer())), &mut retire);
        assert_eq!(deck.level(standard, 1.0), 0.0);
        deck.play();
        deck.set_trim(2.0);
        assert_eq!(deck.fader_position(), 1.0);
        assert_eq!(deck.level(standard, 1.0), 2.0, "the trim stands alone");
        assert_eq!(deck.level(standard, 0.5), 1.0, "the crossfader counts");
        deck.set_fader_position(0.5);
        assert_eq!(deck.level(standard, 1.0), 2.0 * 0.25);
        assert_eq!(deck.level(ChannelFaderCurve::Fast, 1.0), 2.0 * 0.75);
        assert_eq!(
            deck.level(standard, 0.0),
            0.0,
            "the crossfader cuts a playing deck"
        );
    }
}
