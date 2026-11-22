//! The tempo fader, pitch bend, sync to the tempo master and Smart Fader's tempo.

use crate::grid::BeatGrid;
use crate::retire::RetireQueue;
use crate::sync::{MasterBeat, PhaseAction, SyncMode, phase_action, sync_multiple};
use crate::tempo::TempoRange;

use super::{Deck, DeckId};

impl Deck {
    pub(crate) fn set_beat_grid(&mut self, beat_grid: Option<BeatGrid>, retire: &mut RetireQueue) {
        if let Some(previous) = std::mem::replace(&mut self.beat_grid, beat_grid) {
            retire.beat_grid(previous);
        }
        self.sync.multiple = None;
    }

    pub(super) fn apply_tempo(&mut self, tempo: f64) {
        self.tempo = tempo;
        self.phase_bend = 0.0;
        self.update_rate();
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

    pub(super) fn play_fader_tempo(&mut self, tempo: f64) {
        if !self.sync.following && !self.smart_tempo {
            self.apply_tempo(tempo);
        }
    }

    pub(crate) fn set_pitch_bend(&mut self, bend: f64) {
        if bend != 0.0 {
            self.loosen_beat_sync();
        }
        self.platter.set_held_bend(bend);
    }

    pub(crate) fn set_sync(&mut self, mode: SyncMode) {
        self.sync.mode = mode;
        self.sync.multiple = None;
        self.sync.locked = false;
    }

    /// A hand on a following deck drops beat sync to tempo sync, as on a CDJ-3000.
    pub(super) fn loosen_beat_sync(&mut self) {
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
        if !self.sync.following {
            self.sync.following = true;
            self.fader.hand_over();
        }
        let Some(grid) = &self.beat_grid else {
            return;
        };
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
}
