//! Applying a [`DeckCommand`] on the audio thread.

use crate::command::DeckCommand;
use crate::retire::{RetireQueue, replace_insert};

use super::Deck;

impl Deck {
    /// Makes the change `command` asks for. Whatever it replaces goes to `retire`.
    pub(crate) fn apply(&mut self, command: DeckCommand, retire: &mut RetireQueue) {
        match command {
            DeckCommand::Load(track) => self.load(track, retire),
            DeckCommand::Play => self.play(),
            DeckCommand::Pause => self.pause(retire),
            DeckCommand::Cue => self.cue(retire),
            DeckCommand::SetCuePoint { seconds } => self.set_cue_point(seconds),
            DeckCommand::Seek { seconds } => self.seek(seconds, retire),
            DeckCommand::HotCue { seconds } => self.hot_cue(seconds, retire),
            DeckCommand::BeatJump { beats } => self.beat_jump(beats, retire),
            DeckCommand::LoopIn => self.loop_in(),
            DeckCommand::LoopOut => self.loop_out(retire),
            DeckCommand::BeatLoop(length) => self.beat_loop(length, retire),
            DeckCommand::HalveLoop => self.halve_loop(retire),
            DeckCommand::DoubleLoop => self.double_loop(retire),
            DeckCommand::ReloopOrExit => self.reloop_or_exit(retire),
            DeckCommand::SetSlip(enabled) => self.set_slip(enabled),
            DeckCommand::BeatRoll(length) => self.beat_roll(length, retire),
            DeckCommand::HoldPadEffect(pad) => self.hold_pad_effect(pad),
            DeckCommand::ReleasePadEffect => self.release_pad_effect(),
            DeckCommand::SetBeatGrid(beat_grid) => self.set_beat_grid(beat_grid, retire),
            DeckCommand::SetTempo(tempo) => self.set_tempo(tempo),
            DeckCommand::SetTempoFader(position) => self.set_tempo_fader(position),
            DeckCommand::SetTempoRange(range) => self.set_tempo_range(range),
            DeckCommand::SetPitchBend(bend) => self.set_pitch_bend(bend),
            DeckCommand::SetSync(mode) => self.set_sync(mode),
            DeckCommand::SetKeyLock(enabled) => self.set_key_lock(enabled),
            DeckCommand::SetKeyShift { semitones } => self.set_key_shift(semitones),
            DeckCommand::Jog(input) => self.jog(input),
            DeckCommand::SetVinylMode(enabled) => self.set_vinyl_mode(enabled),
            DeckCommand::SetTrim(gain) => self.set_trim(gain),
            DeckCommand::SetChannelFader(position) => self.set_fader_position(position),
            DeckCommand::SetEq { band, position } => self.set_eq(band, position),
            DeckCommand::SetFilter(position) => self.set_filter(position),
            DeckCommand::SetFilterResonance(resonance) => self.set_filter_resonance(resonance),
            DeckCommand::SetSmartCfx(preset) => self.set_smart_cfx(preset),
            DeckCommand::SetInsert { slot, processor } => {
                replace_insert(self.inserts.get_mut(slot), processor, retire);
            }
        }
    }
}
