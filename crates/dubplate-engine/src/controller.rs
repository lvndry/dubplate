use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use rtrb::{Consumer, Producer, RingBuffer};

use crate::channel::ChannelSettings;
use crate::deck::DeckId;
use crate::decode::decode_file;
use crate::effects::{EffectChange, EffectSettings, EffectTarget, EffectUnitId, EffectUnitState};
use crate::error::{EngineError, check_range};
use crate::grid::BeatGrid;
use crate::headphones::HeadphoneSettings;
use crate::jog::JogInput;
use crate::mixer::{Command, Renderer, RetireQueue, Retired};
use crate::processor::AudioProcessor;
use crate::state::{
    DeckState, EngineStats, LoopState, Meters, PlatterState, SharedState, TempoState,
    decode_loop_length, take_peak,
};
use crate::sync::SyncMode;
use crate::tempo::{MasterTempo, TempoRange};
use crate::track::{MAX_TRACK_SECONDS, Track};
use crate::{BeatLength, DECK_COUNT, EFFECT_UNIT_COUNT, EffectKind, EqBand, INSERT_SLOTS};

const COMMAND_CAPACITY: usize = 1024;
const RETIRE_CAPACITY: usize = COMMAND_CAPACITY * 4;
const MAX_TEMPO: f64 = 4.0;
const MAX_GAIN: f32 = 16.0;
const MAX_JUMP_BEATS: f64 = 1_024.0;

/// Creates an engine whose audio side is driven by hand instead of a sound card.
///
/// Call [`Renderer::render`] to produce audio. [`crate::AudioOutput::start`] does the same with
/// a real device.
pub fn offline_engine(sample_rate: u32) -> (Controller, Renderer) {
    let (command_producer, command_consumer) = RingBuffer::new(COMMAND_CAPACITY);
    let (retire_producer, retire_consumer) = RingBuffer::new(RETIRE_CAPACITY);
    let shared = Arc::new(SharedState::new(sample_rate));
    let controller = Controller {
        commands: command_producer,
        retired: retire_consumer,
        shared: Arc::clone(&shared),
        tracks: std::array::from_fn(|_| None),
        beat_grids: std::array::from_fn(|_| None),
        effect_units: EffectUnitId::ALL.map(EffectSettings::initial),
        channels: [ChannelSettings::default(); DECK_COUNT],
        quantize: true,
        headphones: HeadphoneSettings::default(),
        key_locks: [false; DECK_COUNT],
    };
    let renderer = Renderer::new(
        sample_rate,
        command_consumer,
        RetireQueue::new(retire_producer),
        shared,
    );
    (controller, renderer)
}

/// The control side of the engine, for the UI, a MIDI controller or a test.
///
/// Every method returns at once: commands go to the audio thread through a lock-free queue and
/// take effect at the start of its next callback, and state comes back through atomics.
pub struct Controller {
    commands: Producer<Command>,
    retired: Consumer<Retired>,
    shared: Arc<SharedState>,
    tracks: [Option<Track>; DECK_COUNT],
    beat_grids: [Option<BeatGrid>; DECK_COUNT],
    effect_units: [EffectSettings; EFFECT_UNIT_COUNT],
    channels: [ChannelSettings; DECK_COUNT],
    quantize: bool,
    headphones: HeadphoneSettings,
    key_locks: [bool; DECK_COUNT],
}

impl Controller {
    /// Opens an audio file, starts decoding it and loads it on a deck, stopped at its start.
    pub fn load_file(
        &mut self,
        deck: DeckId,
        path: impl AsRef<Path>,
    ) -> Result<Track, EngineError> {
        let track = decode_file(path)?;
        self.load_track(deck, track.clone())?;
        Ok(track)
    }

    /// Loads a track on a deck, stopped at its start. Loading the track another deck holds
    /// plays the same decoded audio on both. A track's decoding stops early once nothing holds
    /// it any more: no deck and no [`Track`] handle.
    pub fn load_track(&mut self, deck: DeckId, track: Track) -> Result<(), EngineError> {
        self.send(Command::Load {
            deck,
            track: Some(Arc::clone(track.buffer())),
        })?;
        self.replace_track(deck, Some(track));
        Ok(())
    }

    /// Takes the track off a deck.
    pub fn eject(&mut self, deck: DeckId) -> Result<(), EngineError> {
        self.send(Command::Load { deck, track: None })?;
        self.replace_track(deck, None);
        Ok(())
    }

    /// The track last loaded on a deck.
    pub fn track(&self, deck: DeckId) -> Option<&Track> {
        self.tracks[deck.index()].as_ref()
    }

    /// Starts playback.
    pub fn play(&mut self, deck: DeckId) -> Result<(), EngineError> {
        self.send(Command::Play(deck))
    }

    /// Stops playback where it is.
    pub fn pause(&mut self, deck: DeckId) -> Result<(), EngineError> {
        self.send(Command::Pause(deck))
    }

    /// Plays a stopped deck or stops a playing one, as the deck was last published.
    pub fn toggle_play(&mut self, deck: DeckId) -> Result<(), EngineError> {
        if self.deck_state(deck).playing {
            self.pause(deck)
        } else {
            self.play(deck)
        }
    }

    /// The CDJ cue button: while playing, jumps back to the cue point and stops; while stopped,
    /// sets the cue point at the current position.
    pub fn cue(&mut self, deck: DeckId) -> Result<(), EngineError> {
        self.send(Command::Cue(deck))
    }

    /// Sets the cue point, in seconds from the start of the track.
    pub fn set_cue_point(&mut self, deck: DeckId, seconds: f64) -> Result<(), EngineError> {
        check_range("cue point", seconds, 0.0, MAX_TRACK_SECONDS as f64)?;
        self.send(Command::SetCuePoint { deck, seconds })
    }

    /// Jumps to a position in seconds from the start of the track, keeping the deck playing or
    /// stopped as it was.
    pub fn seek(&mut self, deck: DeckId, seconds: f64) -> Result<(), EngineError> {
        check_range("seek position", seconds, 0.0, MAX_TRACK_SECONDS as f64)?;
        self.send(Command::Seek { deck, seconds })
    }

    /// Sets the tempo the tempo fader asks for, where 1.0 is the track's own tempo and 1.08 is
    /// +8%, whatever the tempo range. Pitch follows speed, as on a turntable. While the deck
    /// follows the tempo master, or after sync was switched off, the fader only takes over
    /// once it reaches the playing tempo, as on a CDJ.
    pub fn set_tempo(&mut self, deck: DeckId, tempo: f64) -> Result<(), EngineError> {
        if !(tempo.is_finite() && tempo > 0.0 && tempo <= MAX_TEMPO) {
            return Err(EngineError::InvalidParameter {
                name: "tempo",
                value: tempo,
            });
        }
        self.send(Command::SetTempo { deck, tempo })
    }

    /// Sets the deck's trim gain, a linear factor where 1.0 leaves the level unchanged.
    pub fn set_gain(&mut self, deck: DeckId, gain: f32) -> Result<(), EngineError> {
        check_range("gain", f64::from(gain), 0.0, f64::from(MAX_GAIN))?;
        self.send(Command::SetTrim { deck, gain })
    }

    /// Sets the channel fader, from 0.0 (closed) to 1.0 (fully open).
    pub fn set_channel_fader(&mut self, deck: DeckId, position: f32) -> Result<(), EngineError> {
        check_range("channel fader", f64::from(position), 0.0, 1.0)?;
        self.send(Command::SetChannelFader { deck, position })
    }

    /// Sets the crossfader, from 0.0 (deck A only) to 1.0 (deck B only).
    pub fn set_crossfader(&mut self, position: f32) -> Result<(), EngineError> {
        check_range("crossfader", f64::from(position), 0.0, 1.0)?;
        self.send(Command::SetCrossfader(position))
    }

    /// Sets the master gain, a linear factor where 1.0 leaves the level unchanged.
    pub fn set_master_gain(&mut self, gain: f32) -> Result<(), EngineError> {
        check_range("master gain", f64::from(gain), 0.0, f64::from(MAX_GAIN))?;
        self.send(Command::SetMasterGain(gain))
    }

    /// Turns one of a channel's EQ knobs, from 0.0 (band killed) through 0.5 (unity) to 1.0
    /// (+6 dB). See [`crate::eq_gain`] for the curve.
    pub fn set_eq(&mut self, deck: DeckId, band: EqBand, position: f32) -> Result<(), EngineError> {
        check_range("EQ knob", f64::from(position), 0.0, 1.0)?;
        self.send(Command::SetEq {
            deck,
            band,
            position,
        })?;
        self.channels[deck.index()].eq[band.index()] = position;
        Ok(())
    }

    /// Turns a channel's filter knob, the DDJ-FLX4's CFX: from 0.0 (low-pass fully closed)
    /// through 0.5 (flat) to 1.0 (high-pass fully closed).
    pub fn set_filter(&mut self, deck: DeckId, position: f32) -> Result<(), EngineError> {
        check_range("filter knob", f64::from(position), 0.0, 1.0)?;
        self.send(Command::SetFilter { deck, position })?;
        self.channels[deck.index()].filter = position;
        Ok(())
    }

    /// Sets how much a channel's filter sings at its cutoff, from 0.0 to 1.0. The resonance
    /// grows as the knob leaves the centre, so small moves stay clean.
    pub fn set_filter_resonance(
        &mut self,
        deck: DeckId,
        resonance: f32,
    ) -> Result<(), EngineError> {
        check_range("filter resonance", f64::from(resonance), 0.0, 1.0)?;
        self.send(Command::SetFilterResonance { deck, resonance })?;
        self.channels[deck.index()].filter_resonance = resonance;
        Ok(())
    }

    /// A channel's EQ and filter knobs, as last set.
    pub fn channel(&self, deck: DeckId) -> ChannelSettings {
        self.channels[deck.index()]
    }

    /// The platter top was touched or let go. In vinyl mode a touch holds the record, like a
    /// hand on a turntable: the music stops under the hand and follows the platter
    /// ([`Controller::jog_scratch`]); letting go returns to the deck's tempo, or to a stop when
    /// the deck is paused. Out of vinyl mode a touch does nothing.
    pub fn set_jog_touch(&mut self, deck: DeckId, touched: bool) -> Result<(), EngineError> {
        self.jog(deck, JogInput::Touch(touched))
    }

    /// The platter turned by `ticks` while touched, positive clockwise: the record moves with
    /// it, one revolution for 1.8 seconds of the track, as a record at 33⅓ RPM. Without a hand
    /// on the platter it nudges instead.
    pub fn jog_scratch(&mut self, deck: DeckId, ticks: i32) -> Result<(), EngineError> {
        self.jog(deck, JogInput::Scratch(ticks))
    }

    /// The jog's edge turned by `ticks`, positive clockwise. On a playing deck it bends the
    /// tempo with the wheel's speed to line up beats, and the bend fades out smoothly when the
    /// wheel stops; every revolution moves the music 0.3 seconds. On a stopped deck it moves
    /// the playhead like a hand on the record, to find a beat by ear.
    pub fn jog_nudge(&mut self, deck: DeckId, ticks: i32) -> Result<(), EngineError> {
        self.jog(deck, JogInput::Nudge(ticks))
    }

    /// The jog turned by `ticks` with SHIFT held: moves quickly through the track, 15 seconds
    /// per revolution, playing or not.
    pub fn jog_search(&mut self, deck: DeckId, ticks: i32) -> Result<(), EngineError> {
        self.jog(deck, JogInput::Search(ticks))
    }

    /// Sets how many ticks the controller reports per revolution of the platter. The
    /// DDJ-FLX4 reports [`crate::DEFAULT_JOG_TICKS_PER_REVOLUTION`].
    pub fn set_jog_ticks_per_revolution(&mut self, ticks: u32) -> Result<(), EngineError> {
        if ticks == 0 {
            return Err(EngineError::InvalidParameter {
                name: "jog ticks per revolution",
                value: 0.0,
            });
        }
        self.send(Command::SetJogTicksPerRevolution(ticks))
    }

    /// Switches a deck's vinyl mode, on by default: on, touching the platter top holds the
    /// record for scratching; off, the platter only nudges, like a CDJ in CDJ mode.
    pub fn set_vinyl_mode(&mut self, deck: DeckId, enabled: bool) -> Result<(), EngineError> {
        self.send(Command::SetVinylMode { deck, enabled })
    }

    fn jog(&mut self, deck: DeckId, input: JogInput) -> Result<(), EngineError> {
        self.send(Command::Jog { deck, input })
    }

    /// Puts a processor in one of a deck's insert slots, or empties the slot with `None`. Deck
    /// inserts run after the trim and before the channel fader, in slot order.
    pub fn set_deck_insert(
        &mut self,
        deck: DeckId,
        slot: usize,
        processor: Option<Box<dyn AudioProcessor>>,
    ) -> Result<(), EngineError> {
        check_slot(slot)?;
        self.send(Command::SetDeckInsert {
            deck,
            slot,
            processor,
        })
    }

    /// Puts a processor in one of the master insert slots, or empties the slot with `None`.
    /// Master inserts run on the mix before the master gain, in slot order.
    pub fn set_master_insert(
        &mut self,
        slot: usize,
        processor: Option<Box<dyn AudioProcessor>>,
    ) -> Result<(), EngineError> {
        check_slot(slot)?;
        self.send(Command::SetMasterInsert { slot, processor })
    }

    /// Moves the tempo fader, from -1.0 (slowest) through 0.0 (the track's own tempo) to 1.0
    /// (fastest), in the steps of the deck's [`TempoRange`]. While the deck follows the tempo
    /// master, or after sync was switched off, the fader only takes over once moved through
    /// the playing tempo, as on a CDJ.
    pub fn set_tempo_fader(&mut self, deck: DeckId, position: f64) -> Result<(), EngineError> {
        check_range("tempo fader", position, -1.0, 1.0)?;
        self.send(Command::SetTempoFader { deck, position })
    }

    /// Sets how far a deck's tempo fader reaches. The fader keeps its position, so the tempo
    /// changes with the range, as on a CDJ.
    pub fn set_tempo_range(&mut self, deck: DeckId, range: TempoRange) -> Result<(), EngineError> {
        self.send(Command::SetTempoRange { deck, range })
    }

    /// Holds a pitch bend of `bend`, a share of the tempo such as 0.04 for +4%, until set back
    /// to 0.0. The bend glides in and out. On a deck that follows with beat sync it turns beat
    /// sync into tempo sync.
    pub fn set_pitch_bend(&mut self, deck: DeckId, bend: f64) -> Result<(), EngineError> {
        check_range("pitch bend", bend, -0.9, 0.9)?;
        self.send(Command::SetPitchBend { deck, bend })
    }

    /// Sets a deck's SYNC. A synced deck other than the tempo master plays at the master's
    /// tempo, following every move of the master's tempo; with beat sync its beats also land on
    /// the master's, jumping to the nearest beat at once and holding there. Both decks need a
    /// beat grid. Choose the master with [`Controller::set_master_tempo`].
    pub fn set_sync(&mut self, deck: DeckId, mode: SyncMode) -> Result<(), EngineError> {
        self.send(Command::SetSync { deck, mode })
    }

    /// Lets synced decks lock to half or double the master's tempo when that is closer to their
    /// own, so a 70 BPM track syncs to a 140 BPM one. On by default.
    pub fn set_sync_half_and_double(&mut self, allowed: bool) -> Result<(), EngineError> {
        self.send(Command::SetSyncHalfAndDouble(allowed))
    }

    /// A deck's tempo and sync, as the audio thread last published them.
    pub fn tempo_state(&self, deck: DeckId) -> TempoState {
        let shared = &self.shared.decks[deck.index()];
        let bpm = shared.bpm.load();
        TempoState {
            bpm: bpm.is_finite().then_some(bpm),
            tempo_range: TempoRange::from_index(shared.tempo_range.load(Ordering::Relaxed)),
            sync: SyncMode::from_index(shared.sync_mode.load(Ordering::Relaxed)),
            following: shared.following.load(Ordering::Relaxed),
            is_master: self.tempo_master_deck() == Some(deck),
            fader_waits_for_pickup: shared.fader_waits_for_pickup.load(Ordering::Relaxed),
        }
    }

    /// A channel's CUE button: sends the channel's sound before its fader, after its EQ and
    /// filter, to the headphones.
    pub fn set_headphone_cue(&mut self, deck: DeckId, enabled: bool) -> Result<(), EngineError> {
        self.send(Command::SetHeadphoneCue { deck, enabled })?;
        self.headphones.cue[deck.index()] = enabled;
        Ok(())
    }

    /// The MASTER CUE button: sends the master mix to the headphones' cue side too.
    pub fn set_master_cue(&mut self, enabled: bool) -> Result<(), EngineError> {
        self.send(Command::SetMasterCue(enabled))?;
        self.headphones.master_cue = enabled;
        Ok(())
    }

    /// The headphone MIXING knob, from 0.0 (the cued channels only) to 1.0 (the master only),
    /// with both at -3 dB in the middle.
    pub fn set_headphone_mix(&mut self, mix: f32) -> Result<(), EngineError> {
        check_range("headphone mix", f64::from(mix), 0.0, 1.0)?;
        self.send(Command::SetHeadphoneMix(mix))?;
        self.headphones.mix = mix;
        Ok(())
    }

    /// The headphone LEVEL knob, from 0.0 (silent) to 1.0 (full), on the channel fader's curve.
    pub fn set_headphone_level(&mut self, level: f32) -> Result<(), EngineError> {
        check_range("headphone level", f64::from(level), 0.0, 1.0)?;
        self.send(Command::SetHeadphoneLevel(level))?;
        self.headphones.level = level;
        Ok(())
    }

    /// The headphone section's buttons and knobs, as last set. The headphones play on outputs
    /// 3 and 4 of a device with four or more channels, such as a DDJ-FLX4.
    pub fn headphones(&self) -> HeadphoneSettings {
        self.headphones
    }

    /// Key lock, a CDJ's MASTER TEMPO: tempo changes keep the track's pitch, through a
    /// Signalsmith Stretch time-stretcher lined up with the playhead. Scratching and a stopped
    /// record play without it, as on a CDJ. Switching it crossfades over 20 ms.
    pub fn set_key_lock(&mut self, deck: DeckId, enabled: bool) -> Result<(), EngineError> {
        self.send(Command::SetKeyLock { deck, enabled })?;
        self.key_locks[deck.index()] = enabled;
        Ok(())
    }

    /// Whether key lock is on for a deck.
    pub fn key_lock(&self, deck: DeckId) -> bool {
        self.key_locks[deck.index()]
    }

    /// Switches quantize for both decks, on by default. With quantize and a beat grid, the cue
    /// button, loop in and loop out set their points on the nearest beat, beat loops start on
    /// the beat just passed, and hot cues and reloop jump in rhythm.
    pub fn set_quantize(&mut self, enabled: bool) -> Result<(), EngineError> {
        self.send(Command::SetQuantize(enabled))?;
        self.quantize = enabled;
        Ok(())
    }

    /// Whether quantize is on.
    pub fn quantize(&self) -> bool {
        self.quantize
    }

    /// A hot cue at `seconds`: while playing, the deck jumps there in rhythm with quantize, as
    /// far past it as the playhead was past its last beat, so the music carries on in time; on
    /// a stopped deck it jumps there and plays.
    pub fn hot_cue(&mut self, deck: DeckId, seconds: f64) -> Result<(), EngineError> {
        check_range("hot cue", seconds, 0.0, MAX_TRACK_SECONDS as f64)?;
        self.send(Command::HotCue { deck, seconds })
    }

    /// Jumps by `beats` beats on the beat grid, forwards when positive, keeping the place in
    /// the beat. With a loop on, the loop moves with the playhead. Without a grid, beats count
    /// at 120 BPM.
    pub fn beat_jump(&mut self, deck: DeckId, beats: f64) -> Result<(), EngineError> {
        check_range("beat jump", beats, -MAX_JUMP_BEATS, MAX_JUMP_BEATS)?;
        self.send(Command::BeatJump { deck, beats })
    }

    /// LOOP IN: marks where a loop starts, on the nearest beat with quantize.
    pub fn loop_in(&mut self, deck: DeckId) -> Result<(), EngineError> {
        self.send(Command::LoopIn(deck))
    }

    /// LOOP OUT: ends the loop where the deck is, on the nearest beat with quantize, and turns
    /// it on. Without a loop in point it moves the end of the current loop.
    pub fn loop_out(&mut self, deck: DeckId) -> Result<(), EngineError> {
        self.send(Command::LoopOut(deck))
    }

    /// A beat loop of `length`, from a sixteenth of a beat to thirty-two beats. It starts on
    /// the beat just passed with quantize, so the music never jumps; with a loop already on,
    /// it changes that loop's length from its start.
    pub fn beat_loop(&mut self, deck: DeckId, length: BeatLength) -> Result<(), EngineError> {
        self.send(Command::BeatLoop { deck, length })
    }

    /// Halves the loop from its start, down to a sixteenth of a beat. A playhead past the new
    /// end goes back inside it in rhythm.
    pub fn halve_loop(&mut self, deck: DeckId) -> Result<(), EngineError> {
        self.send(Command::HalveLoop(deck))
    }

    /// Doubles the loop from its start, up to thirty-two beats.
    pub fn double_loop(&mut self, deck: DeckId) -> Result<(), EngineError> {
        self.send(Command::DoubleLoop(deck))
    }

    /// RELOOP/EXIT: leaves the loop that is on and plays on, or turns the last loop back on
    /// and jumps to its start, in rhythm with quantize.
    pub fn reloop_or_exit(&mut self, deck: DeckId) -> Result<(), EngineError> {
        self.send(Command::ReloopOrExit(deck))
    }

    /// A deck's loop, as the audio thread last published it.
    pub fn loop_state(&self, deck: DeckId) -> LoopState {
        let shared = &self.shared.decks[deck.index()];
        let seconds = |value: f64| value.is_finite().then_some(value);
        LoopState {
            start_seconds: seconds(shared.loop_start_seconds.load()),
            end_seconds: seconds(shared.loop_end_seconds.load()),
            active: shared.loop_active.load(Ordering::Relaxed),
            length: decode_loop_length(shared.loop_length.load(Ordering::Relaxed)),
            pending_start_seconds: seconds(shared.pending_loop_start_seconds.load()),
        }
    }

    /// Sets the beat grid of the track on a deck, so effects on it follow its tempo and beats.
    /// Loading a track clears the grid: set it after [`Controller::load_track`]. Commands
    /// sent one after the other take effect in the same callback, so no audio plays in between.
    pub fn set_beat_grid(
        &mut self,
        deck: DeckId,
        beat_grid: Option<BeatGrid>,
    ) -> Result<(), EngineError> {
        self.send(Command::SetBeatGrid {
            deck,
            beat_grid: beat_grid.clone(),
        })?;
        self.beat_grids[deck.index()] = beat_grid;
        Ok(())
    }

    /// The beat grid last set on a deck. Cloning a grid shares its beats.
    pub fn beat_grid(&self, deck: DeckId) -> Option<BeatGrid> {
        self.beat_grids[deck.index()].clone()
    }

    /// Chooses the tempo master: the deck synced decks follow and whose tempo effects on the
    /// master follow, like the MASTER button of a CDJ.
    pub fn set_master_tempo(&mut self, source: MasterTempo) -> Result<(), EngineError> {
        self.send(Command::SetMasterTempo(source))
    }

    /// The deck whose tempo effects on the master follow, as the audio thread last chose it.
    pub fn tempo_master_deck(&self) -> Option<DeckId> {
        self.shared.tempo_master.load()
    }

    /// Selects an effect unit's effect. When the unit is on, the old effect fades out, or rings
    /// out its trail, while the new one fades in.
    pub fn set_effect_kind(
        &mut self,
        unit: EffectUnitId,
        kind: EffectKind,
    ) -> Result<(), EngineError> {
        self.change_effect(unit, EffectChange::Kind(kind))
    }

    /// Moves an effect unit to a deck or the master, like a mixer's channel select switch. When
    /// the unit is on, the effect fades out, or rings out its trail, where it was.
    pub fn set_effect_target(
        &mut self,
        unit: EffectUnitId,
        target: EffectTarget,
    ) -> Result<(), EngineError> {
        self.change_effect(unit, EffectChange::Target(target))
    }

    /// Sets an effect unit's beat length, like a mixer's beat buttons. Step through the
    /// buttons' lengths with [`BeatLength::longer`] and [`BeatLength::shorter`].
    pub fn set_effect_beat(
        &mut self,
        unit: EffectUnitId,
        beat: BeatLength,
    ) -> Result<(), EngineError> {
        self.change_effect(unit, EffectChange::Beat(beat))
    }

    /// Turns an effect unit's level and depth knob, from 0 to 1.
    pub fn set_effect_depth(&mut self, unit: EffectUnitId, depth: f32) -> Result<(), EngineError> {
        check_range("effect depth", f64::from(depth), 0.0, 1.0)?;
        self.change_effect(unit, EffectChange::Depth(depth))
    }

    /// Switches an effect unit on or off. Echo, ping-pong and reverb ring out when switched off.
    pub fn set_effect_enabled(
        &mut self,
        unit: EffectUnitId,
        enabled: bool,
    ) -> Result<(), EngineError> {
        self.change_effect(unit, EffectChange::Enabled(enabled))
    }

    /// The effect on button: switches a unit on when it is off and off when it is on.
    pub fn toggle_effect(&mut self, unit: EffectUnitId) -> Result<(), EngineError> {
        let enabled = self.effect_units[unit.index()].is_enabled();
        self.set_effect_enabled(unit, !enabled)
    }

    /// An effect unit's settings, and whether the audio thread last found it sounding.
    pub fn effect_unit(&self, unit: EffectUnitId) -> EffectUnitState {
        let sounding = self.shared.effect_sounding[unit.index()].load(Ordering::Relaxed);
        self.effect_units[unit.index()].with_sounding(sounding)
    }

    /// What a deck is doing, as the audio thread last published it.
    pub fn deck_state(&self, deck: DeckId) -> DeckState {
        let shared = &self.shared.decks[deck.index()];
        DeckState {
            loaded: shared.loaded.load(Ordering::Relaxed),
            playing: shared.playing.load(Ordering::Relaxed),
            position_seconds: shared.position_seconds.load(),
            cue_point_seconds: shared.cue_point_seconds.load(),
            tempo: shared.tempo.load(),
        }
    }

    /// What a deck's jog wheel is doing, as the audio thread last published it.
    pub fn platter_state(&self, deck: DeckId) -> PlatterState {
        let shared = &self.shared.decks[deck.index()];
        PlatterState {
            speed: shared.speed.load(),
            touched: shared.touched.load(Ordering::Relaxed),
            vinyl_mode: shared.vinyl_mode.load(Ordering::Relaxed),
        }
    }

    /// Peak levels since the previous call.
    pub fn take_meters(&self) -> Meters {
        Meters {
            deck_peaks: std::array::from_fn(|index| take_peak(&self.shared.decks[index].peak)),
            channel_peaks: std::array::from_fn(|index| {
                take_peak(&self.shared.decks[index].pre_fader_peak)
            }),
            master_peak: take_peak(&self.shared.master_peak),
            headphone_peak: take_peak(&self.shared.headphone_peak),
        }
    }

    /// Buffer size, latency and dropout counters.
    pub fn stats(&self) -> EngineStats {
        self.shared.stats.snapshot()
    }

    pub(crate) fn shared(&self) -> &Arc<SharedState> {
        &self.shared
    }

    /// Drops what the audio thread handed back. Every command does this first, so calling it
    /// is only needed to free memory sooner on an idle controller.
    pub fn collect_garbage(&mut self) {
        while let Ok(retired) = self.retired.pop() {
            drop(retired);
        }
    }

    fn send(&mut self, command: Command) -> Result<(), EngineError> {
        self.collect_garbage();
        self.commands
            .push(command)
            .map_err(|_| EngineError::CommandQueueFull)
    }

    fn change_effect(
        &mut self,
        unit: EffectUnitId,
        change: EffectChange,
    ) -> Result<(), EngineError> {
        self.send(Command::ChangeEffect { unit, change })?;
        self.effect_units[unit.index()].apply(change);
        Ok(())
    }

    fn replace_track(&mut self, deck: DeckId, track: Option<Track>) {
        self.tracks[deck.index()] = track;
        self.beat_grids[deck.index()] = None;
    }
}

fn check_slot(slot: usize) -> Result<(), EngineError> {
    if slot < INSERT_SLOTS {
        Ok(())
    } else {
        Err(EngineError::InvalidInsertSlot {
            slot,
            slot_count: INSERT_SLOTS,
        })
    }
}
