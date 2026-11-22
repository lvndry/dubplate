//! The mixer: gains, faders, EQ, filter, Smart CFX, inserts and the headphone section.

use crate::channel::ChannelSettings;
use crate::command::{Command, DeckCommand, HeadphoneCommand};
use crate::deck::DeckId;
use crate::error::{EngineError, check_range};
use crate::headphones::HeadphoneSettings;
use crate::processor::AudioProcessor;
use crate::smart_cfx::SmartCfx;
use crate::{EqBand, INSERT_SLOTS};

use super::{Controller, MAX_GAIN};

impl Controller {
    /// Sets the deck's trim gain, a linear factor where 1.0 leaves the level unchanged.
    pub fn set_gain(&mut self, deck: DeckId, gain: f32) -> Result<(), EngineError> {
        check_range("gain", f64::from(gain), 0.0, f64::from(MAX_GAIN))?;
        self.send_deck(deck, DeckCommand::SetTrim(gain))
    }

    /// Sets the channel fader, from 0.0 (closed) to 1.0 (fully open).
    pub fn set_channel_fader(&mut self, deck: DeckId, position: f32) -> Result<(), EngineError> {
        check_range("channel fader", f64::from(position), 0.0, 1.0)?;
        self.send_deck(deck, DeckCommand::SetChannelFader(position))
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
        self.send_deck(deck, DeckCommand::SetEq { band, position })?;
        self.decks[deck.index()].channel.eq[band.index()] = position;
        Ok(())
    }

    /// Turns a channel's filter knob, the DDJ-FLX4's CFX: from 0.0 (low-pass fully closed)
    /// through 0.5 (flat) to 1.0 (high-pass fully closed).
    pub fn set_filter(&mut self, deck: DeckId, position: f32) -> Result<(), EngineError> {
        check_range("filter knob", f64::from(position), 0.0, 1.0)?;
        self.send_deck(deck, DeckCommand::SetFilter(position))?;
        self.decks[deck.index()].channel.filter = position;
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
        self.send_deck(deck, DeckCommand::SetFilterResonance(resonance))?;
        self.decks[deck.index()].channel.filter_resonance = resonance;
        Ok(())
    }

    /// Turns Smart CFX on a channel with `preset`, or back to the plain filter with `None`. The
    /// filter knob then drives the filter and the preset's effect together; the centre still
    /// leaves the music untouched. A preset switched off rings out.
    pub fn set_smart_cfx(
        &mut self,
        deck: DeckId,
        preset: Option<SmartCfx>,
    ) -> Result<(), EngineError> {
        self.send_deck(deck, DeckCommand::SetSmartCfx(preset))?;
        self.decks[deck.index()].smart_cfx = preset;
        Ok(())
    }

    /// A channel's Smart CFX preset, `None` for the plain filter.
    pub fn smart_cfx(&self, deck: DeckId) -> Option<SmartCfx> {
        self.decks[deck.index()].smart_cfx
    }

    /// A channel's EQ and filter knobs, as last set.
    pub fn channel(&self, deck: DeckId) -> ChannelSettings {
        self.decks[deck.index()].channel
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
        self.send_deck(deck, DeckCommand::SetInsert { slot, processor })
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

    /// A channel's CUE button: sends the channel's sound before its fader, after its EQ and
    /// filter, to the headphones.
    pub fn set_headphone_cue(&mut self, deck: DeckId, enabled: bool) -> Result<(), EngineError> {
        self.send(Command::Headphones(HeadphoneCommand::Cue { deck, enabled }))?;
        self.headphones.cue[deck.index()] = enabled;
        Ok(())
    }

    /// The MASTER CUE button: sends the master mix to the headphones' cue side too.
    pub fn set_master_cue(&mut self, enabled: bool) -> Result<(), EngineError> {
        self.send(Command::Headphones(HeadphoneCommand::MasterCue(enabled)))?;
        self.headphones.master_cue = enabled;
        Ok(())
    }

    /// The headphone MIXING knob, from 0.0 (the cued channels only) to 1.0 (the master only),
    /// with both at -3 dB in the middle.
    pub fn set_headphone_mix(&mut self, mix: f32) -> Result<(), EngineError> {
        check_range("headphone mix", f64::from(mix), 0.0, 1.0)?;
        self.send(Command::Headphones(HeadphoneCommand::Mix(mix)))?;
        self.headphones.mix = mix;
        Ok(())
    }

    /// The headphone LEVEL knob, from 0.0 (silent) to 1.0 (full), on the channel fader's curve.
    pub fn set_headphone_level(&mut self, level: f32) -> Result<(), EngineError> {
        check_range("headphone level", f64::from(level), 0.0, 1.0)?;
        self.send(Command::Headphones(HeadphoneCommand::Level(level)))?;
        self.headphones.level = level;
        Ok(())
    }

    /// The headphone section's buttons and knobs, as last set. The headphones play on outputs
    /// 3 and 4 of a device with four or more channels, such as a DDJ-FLX4.
    pub fn headphones(&self) -> HeadphoneSettings {
        self.headphones
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
