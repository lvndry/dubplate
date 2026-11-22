//! The audio-thread side of a deck. Each submodule holds one part of what a deck does.

use crate::INSERT_SLOTS;
use crate::channel::ChannelStrip;
use crate::curves::{Ramp, channel_fader_gain};
use crate::grid::BeatGrid;
use crate::jog::Platter;
use crate::keylock::KeyLock;
use crate::looping::{LoopRegion, Looping};
use crate::pad_effects::PadEffects;
use crate::processor::{AudioProcessor, StereoBlock};
use crate::slip::Slip;
use crate::sync::{SyncFollow, SyncMode};
use crate::tempo::{BeatClock, TempoFader, TempoRange};
use crate::track::MAX_TRACK_SECONDS;
use crate::voice::Voice;

mod channel;
mod commands;
mod loops;
mod render;
mod tempo;
mod transport;

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
    strip: ChannelStrip,
    inserts: [Option<Box<dyn AudioProcessor>>; INSERT_SLOTS],
    pub(crate) block: StereoBlock,
    output_rate: f64,
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
            key_lock: KeyLock::new(output_rate, declick_frames),
            key_lock_enabled: false,
            strip: ChannelStrip::new(output_rate),
            inserts: std::array::from_fn(|_| None),
            block: StereoBlock::with_capacity(max_block_frames),
            output_rate: f64::from(output_rate.max(1)),
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
    /// the channel fader and the crossfader side gain `crossfader_gain`, the same way the
    /// louder-deck rule hears the deck. The channel fader is not read from `channel_gain`
    /// because that gain already carries the crossfader.
    pub(crate) fn level(&self, crossfader_gain: f32) -> f32 {
        if self.playing {
            self.trim.target() * channel_fader_gain(self.fader_position()) * crossfader_gain
        } else {
            0.0
        }
    }

    pub(super) fn source_rate(&self) -> f64 {
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

    pub(super) fn update_rate(&mut self) {
        self.rate = self.tempo * (1.0 + self.phase_bend) * self.source_rate() / self.output_rate;
        self.platter.set_track_rate(self.source_rate());
    }

    pub(crate) fn slip_enabled(&self) -> bool {
        self.slip.enabled
    }

    pub(crate) fn slip_shadow(&self) -> Option<f64> {
        self.slip.shadow()
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

    pub(crate) fn tempo_range(&self) -> TempoRange {
        self.fader.range()
    }

    /// Whether the tempo fader waits to be moved through the playing tempo before it takes
    /// over again.
    pub(crate) fn fader_waits_for_pickup(&self) -> bool {
        !self.fader.is_in_control()
    }

    pub(crate) fn sync_mode(&self) -> SyncMode {
        self.sync.mode
    }

    pub(crate) fn is_following(&self) -> bool {
        self.sync.following
    }

    /// The tempo at its own speed, in beats per minute, when the deck has a beat grid.
    pub(crate) fn own_bpm(&self) -> Option<f64> {
        self.beat_grid
            .as_ref()
            .map(|grid| grid.bpm_at(self.position_seconds()))
    }

    pub(super) fn clamp_position(&self, position: f64) -> f64 {
        let longest = MAX_TRACK_SECONDS as f64 * self.source_rate();
        let end = self.voice.end_frame().unwrap_or(longest);
        position.clamp(0.0, end)
    }

    pub(super) fn at_end(&self) -> bool {
        self.voice
            .end_frame()
            .is_some_and(|end| self.voice.position >= end)
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;
    use std::sync::Arc;

    use crate::curves::channel_fader_gain;
    use crate::retire::RetireQueue;

    use super::*;

    #[test]
    fn the_louder_deck_level_counts_the_crossfader_gain() {
        let track = crate::track::Track::from_interleaved_stereo(&[0.1, 0.1], 48_000).unwrap();
        let (producer, _consumer) = rtrb::RingBuffer::new(16);
        let mut retire = RetireQueue::new(producer);
        let mut deck = Deck::new(48_000, 512, 240);
        deck.load(Some(Arc::clone(track.buffer())), &mut retire);
        assert_eq!(deck.level(1.0), 0.0);
        deck.play();
        deck.set_trim(2.0);
        assert_eq!(deck.fader_position(), 1.0);
        // Crossfader fully to this deck: the trim stands alone.
        assert_eq!(deck.level(1.0), 2.0);
        // Half-open fader, -3 dB crossfader: both count.
        let crossfader_gain = FRAC_PI_2.cos();
        let half_fader = channel_fader_gain(0.5);
        assert_eq!(deck.level(0.5 * 1.0), 2.0 * 1.0 * 0.5);
        assert_eq!(deck.level(1.0) * half_fader, 2.0 * half_fader);
        assert_eq!(deck.level(0.5), 2.0 * 1.0 * 0.5);
        let _ = crossfader_gain;
        // Crossfader fully to the other deck: the level goes to zero while the deck still plays.
        assert_eq!(deck.level(0.0), 0.0);
    }
}
