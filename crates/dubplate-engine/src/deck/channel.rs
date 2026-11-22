//! The deck's mixer channel: trim, EQ, filter, channel fader and crossfader gain.

use dubplate_fx::EqBand;

use crate::processor::StereoBlock;
use crate::smart_cfx::SmartCfx;

use super::Deck;

impl Deck {
    pub(crate) fn set_eq(&mut self, band: EqBand, position: f32) {
        self.strip.set_eq(band, position);
    }

    pub(crate) fn set_automatic_low(&mut self, position: Option<f32>) {
        self.strip.set_automatic_low(position);
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

    /// Applies the channel fader and crossfader gain to the deck's block.
    pub(crate) fn apply_channel_gain(&mut self) {
        let (left_channel, right_channel) = self.block.channels_mut();
        for (left, right) in left_channel.iter_mut().zip(right_channel.iter_mut()) {
            let gain = self.channel_gain.next_value();
            *left *= gain;
            *right *= gain;
        }
    }

    /// Adds the deck's block into the master block and returns its peak.
    pub(crate) fn mix_into(&self, master: &mut StereoBlock) -> f32 {
        master.add(&self.block);
        self.block.peak()
    }
}
