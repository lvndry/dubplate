//! Tempo, key, sync, the tempo master and beat grids.

use std::sync::atomic::Ordering;

use crate::command::{Command, DeckCommand};
use crate::deck::DeckId;
use crate::error::{EngineError, check_range};
use crate::grid::BeatGrid;
use crate::state::TempoState;
use crate::sync::SyncMode;
use crate::tempo::{MasterTempo, TempoRange};

use super::{Controller, MAX_TEMPO};

impl Controller {
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
        self.send_deck(deck, DeckCommand::SetTempo(tempo))
    }

    /// Moves the tempo fader, from -1.0 (slowest) through 0.0 (the track's own tempo) to 1.0
    /// (fastest), in the steps of the deck's [`TempoRange`]. While the deck follows the tempo
    /// master, or after sync was switched off, the fader only takes over once moved through
    /// the playing tempo, as on a CDJ.
    pub fn set_tempo_fader(&mut self, deck: DeckId, position: f64) -> Result<(), EngineError> {
        check_range("tempo fader", position, -1.0, 1.0)?;
        self.send_deck(deck, DeckCommand::SetTempoFader(position))
    }

    /// Sets how far a deck's tempo fader reaches. The fader keeps its position, so the tempo
    /// changes with the range, as on a CDJ.
    pub fn set_tempo_range(&mut self, deck: DeckId, range: TempoRange) -> Result<(), EngineError> {
        self.send_deck(deck, DeckCommand::SetTempoRange(range))
    }

    /// Holds a pitch bend of `bend`, a share of the tempo such as 0.04 for +4%, until set back
    /// to 0.0. The bend glides in and out. On a deck that follows with beat sync it turns beat
    /// sync into tempo sync.
    pub fn set_pitch_bend(&mut self, deck: DeckId, bend: f64) -> Result<(), EngineError> {
        check_range("pitch bend", bend, -0.9, 0.9)?;
        self.send_deck(deck, DeckCommand::SetPitchBend(bend))
    }

    /// Sets a deck's SYNC. A synced deck other than the tempo master plays at the master's
    /// tempo, following every move of the master's tempo; with beat sync its beats also land on
    /// the master's, jumping to the nearest beat at once and holding there. Both decks need a
    /// beat grid. Choose the master with [`Controller::set_master_tempo`].
    pub fn set_sync(&mut self, deck: DeckId, mode: SyncMode) -> Result<(), EngineError> {
        self.send_deck(deck, DeckCommand::SetSync(mode))
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

    /// Chooses the tempo master: the deck synced decks follow and whose tempo effects on the
    /// master follow, like the MASTER button of a CDJ.
    pub fn set_master_tempo(&mut self, source: MasterTempo) -> Result<(), EngineError> {
        self.send(Command::SetMasterTempo(source))
    }

    /// The deck whose tempo effects on the master follow, as the audio thread last chose it.
    pub fn tempo_master_deck(&self) -> Option<DeckId> {
        self.shared.tempo_master.load()
    }

    /// Sets the beat grid of the track on a deck, so effects on it follow its tempo and beats.
    /// Loading a track clears the grid: set it after [`Controller::load_track`]. Commands
    /// sent one after the other take effect in the same callback, so no audio plays in between.
    pub fn set_beat_grid(
        &mut self,
        deck: DeckId,
        beat_grid: Option<BeatGrid>,
    ) -> Result<(), EngineError> {
        self.send_deck(deck, DeckCommand::SetBeatGrid(beat_grid.clone()))?;
        self.decks[deck.index()].beat_grid = beat_grid;
        Ok(())
    }

    /// The beat grid last set on a deck. Cloning a grid shares its beats.
    pub fn beat_grid(&self, deck: DeckId) -> Option<BeatGrid> {
        self.decks[deck.index()].beat_grid.clone()
    }

    /// Key lock, a CDJ's MASTER TEMPO: tempo changes keep the track's pitch, through a
    /// Signalsmith Stretch time-stretcher lined up with the playhead. Scratching and a stopped
    /// record play without it, as on a CDJ. Switching it crossfades over 20 ms.
    pub fn set_key_lock(&mut self, deck: DeckId, enabled: bool) -> Result<(), EngineError> {
        self.send_deck(deck, DeckCommand::SetKeyLock(enabled))?;
        self.decks[deck.index()].key_lock = enabled;
        Ok(())
    }

    /// Whether key lock is on for a deck.
    pub fn key_lock(&self, deck: DeckId) -> bool {
        self.decks[deck.index()].key_lock
    }

    /// Shifts a deck's key by `semitones`, from -12 to +12, through the key lock time-stretcher,
    /// whatever the tempo: Key Shift and Keyboard pads. 0 plays the track's own key.
    pub fn set_key_shift(&mut self, deck: DeckId, semitones: f32) -> Result<(), EngineError> {
        check_range("key shift", f64::from(semitones), -12.0, 12.0)?;
        self.send_deck(deck, DeckCommand::SetKeyShift { semitones })?;
        self.decks[deck.index()].key_shift = semitones;
        Ok(())
    }

    /// A deck's key shift in semitones.
    pub fn key_shift(&self, deck: DeckId) -> f32 {
        self.decks[deck.index()].key_shift
    }

    /// Smart Fader, for long clean blends: while it is on, the two decks share one tempo that
    /// the crossfader moves from deck A's own BPM at the left to deck B's at the right, gliding
    /// there, with the decks' beats locked together, and the LOW knobs swap across the middle
    /// third of the crossfader, the outgoing bass turning down as the incoming bass comes up. A
    /// LOW knob turned away from its centre takes over from the swap. Switching it off gives
    /// the tempo back to the faders, which take over once moved through the playing tempo.
    pub fn set_smart_fader(&mut self, enabled: bool) -> Result<(), EngineError> {
        self.send(Command::SetSmartFader(enabled))?;
        self.smart_fader = enabled;
        Ok(())
    }

    /// Whether Smart Fader is on.
    pub fn smart_fader(&self) -> bool {
        self.smart_fader
    }
}
