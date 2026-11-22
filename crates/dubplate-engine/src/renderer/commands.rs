//! Applying the control side's commands at the start of a callback. Each command goes to the
//! part of the engine that owns the state it changes.

use crate::command::{Command, DeckCommand};
use crate::curves::{channel_fader_gain, crossfader_gains};
use crate::deck::DeckId;
use crate::retire::replace_insert;

use super::{MAX_COMMANDS_PER_CALLBACK, Renderer};

impl Renderer {
    pub(super) fn apply_commands(&mut self) {
        for _ in 0..MAX_COMMANDS_PER_CALLBACK {
            let Ok(command) = self.commands.pop() else {
                return;
            };
            self.apply(command);
        }
    }

    fn apply(&mut self, command: Command) {
        match command {
            Command::Deck { deck, command } => {
                let fader_moved = matches!(command, DeckCommand::SetChannelFader(_));
                self.decks[deck.index()].apply(command, &mut self.retire);
                if fader_moved {
                    self.update_channel_gain(deck);
                }
            }
            Command::Headphones(command) => self.headphones.apply(command),
            Command::Sampler(command) => {
                let master = self
                    .tempo_master
                    .current()
                    .and_then(|deck| self.decks[deck.index()].master_beat(deck));
                self.sampler.apply(command, master, &mut self.retire);
            }
            Command::ChangeEffect { unit, change } => self.effects.change(unit, change),
            Command::SetCrossfader(position) => {
                self.crossfader = position;
                for deck in DeckId::ALL {
                    self.update_channel_gain(deck);
                }
            }
            Command::SetMasterGain(gain) => self.master_gain.set_target(gain),
            Command::SetMasterInsert { slot, processor } => {
                replace_insert(
                    self.master_inserts.get_mut(slot),
                    processor,
                    &mut self.retire,
                );
            }
            Command::SetMasterTempo(source) => self.tempo_master.set_source(source),
            Command::SetJogTicksPerRevolution(ticks) => {
                for deck in &mut self.decks {
                    deck.set_jog_ticks_per_revolution(ticks);
                }
            }
            Command::SetSyncHalfAndDouble(allowed) => self.sync_half_and_double = allowed,
            Command::SetQuantize(enabled) => {
                for deck in &mut self.decks {
                    deck.set_quantize(enabled);
                }
            }
            Command::SetSmartFader(enabled) => self.set_smart_fader(enabled),
            Command::Preview(plan) => self.preview.set(plan, &mut self.retire),
            Command::SetRecording(sink) => {
                if let Some(previous) = std::mem::replace(&mut self.recording, sink) {
                    self.retire.recording(previous);
                }
            }
        }
    }

    pub(super) fn update_channel_gain(&mut self, deck: DeckId) {
        let (left_gain, right_gain) = crossfader_gains(self.crossfader);
        let side_gain = match deck {
            DeckId::A => left_gain,
            DeckId::B => right_gain,
        };
        let deck = &mut self.decks[deck.index()];
        deck.set_channel_gain(channel_fader_gain(deck.fader_position()) * side_gain);
    }
}
