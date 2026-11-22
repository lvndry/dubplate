//! The crossfader assign switches and the fader curves: rekordbox's mixer panel and its
//! Preferences > Controller > Mixer, kept in the settings.

use dubplate_engine::{
    ChannelFaderCurve, Crossfader, CrossfaderAssign, CrossfaderCurve, DECK_COUNT, DeckId,
};
use gpui::Context;

use super::DubplateApp;
use super::notice::NoticePlace;
use crate::settings::MixerSetting;

/// What the window says when Smart Fader has nothing to move between.
const SMART_FADER_RESTS: &str =
    "Smart Fader rests until one deck is on A and the other on B of the crossfader.";

/// A crossfader curve's name in the settings.
pub(super) fn crossfader_curve_label(curve: CrossfaderCurve) -> &'static str {
    match curve {
        CrossfaderCurve::Blend => "Blend",
        CrossfaderCurve::Constant => "Constant",
        CrossfaderCurve::Scratch => "Scratch",
    }
}

/// A channel fader curve's name in the settings.
pub(super) fn channel_fader_curve_label(curve: ChannelFaderCurve) -> &'static str {
    match curve {
        ChannelFaderCurve::Slow => "Slow",
        ChannelFaderCurve::Standard => "Standard",
        ChannelFaderCurve::Fast => "Fast",
    }
}

impl DubplateApp {
    /// The crossfader as the engine plays it: where the DJ put it, its curve and its sides.
    pub(super) fn crossfader(&self) -> Crossfader {
        Crossfader {
            position: self.mixer.crossfader,
            curve: self.settings.mixer.crossfader_curve(),
            assign: DeckId::ALL.map(|deck| self.settings.mixer.crossfader_assign(deck)),
        }
    }

    /// The channel fader curve the engine plays.
    pub(super) fn channel_fader_curve(&self) -> ChannelFaderCurve {
        self.settings.mixer.channel_fader_curve()
    }

    /// Whether Smart Fader is on but has no deck on one of the crossfader's sides.
    pub(super) fn smart_fader_resting(&self) -> bool {
        self.smart_fader_on() && self.crossfader().sides().is_none()
    }

    /// Puts `deck`'s channel on side A or B of the crossfader, or THRU, and remembers it.
    pub(super) fn set_crossfader_assign(
        &mut self,
        deck: DeckId,
        assign: CrossfaderAssign,
        cx: &mut Context<'_, Self>,
    ) {
        let mut sides = self.crossfader().assign;
        sides[deck.index()] = assign;
        self.engine_command(cx, |controller| {
            controller.set_crossfader_assign(deck, assign)
        });
        self.keep_mixer_setting(
            self.crossfader().curve,
            self.channel_fader_curve(),
            sides,
            cx,
        );
        if self.smart_fader_resting() {
            self.show_notice(NoticePlace::Window, SMART_FADER_RESTS, cx);
        }
    }

    /// Moves `deck`'s crossfader assign switch on: A, THRU, B, A.
    pub(super) fn cycle_crossfader_assign(&mut self, deck: DeckId, cx: &mut Context<'_, Self>) {
        let next = self.crossfader().assign[deck.index()].next();
        self.set_crossfader_assign(deck, next, cx);
    }

    /// Chooses the crossfader curve and remembers it.
    pub(super) fn set_crossfader_curve(
        &mut self,
        curve: CrossfaderCurve,
        cx: &mut Context<'_, Self>,
    ) {
        self.engine_command(cx, |controller| controller.set_crossfader_curve(curve));
        self.keep_mixer_setting(
            curve,
            self.channel_fader_curve(),
            self.crossfader().assign,
            cx,
        );
    }

    /// Chooses the channel fader curve and remembers it.
    pub(super) fn set_channel_fader_curve(
        &mut self,
        curve: ChannelFaderCurve,
        cx: &mut Context<'_, Self>,
    ) {
        self.engine_command(cx, |controller| controller.set_channel_fader_curve(curve));
        self.keep_mixer_setting(self.crossfader().curve, curve, self.crossfader().assign, cx);
    }

    /// Says why Smart Fader does nothing yet, when it is turned on with a deck on THRU or both
    /// decks on one side.
    pub(super) fn explain_resting_smart_fader(&mut self, cx: &mut Context<'_, Self>) {
        if self.smart_fader_resting() {
            self.show_notice(NoticePlace::Window, SMART_FADER_RESTS, cx);
        }
    }

    /// Gives a fresh engine the curves and switches kept in the settings.
    pub(super) fn restore_mixer_switches(&mut self, cx: &mut Context<'_, Self>) {
        let crossfader = self.crossfader();
        let channel_fader_curve = self.channel_fader_curve();
        self.engine_command(cx, |controller| {
            controller.set_crossfader_curve(crossfader.curve)?;
            controller.set_channel_fader_curve(channel_fader_curve)?;
            for deck in DeckId::ALL {
                controller.set_crossfader_assign(deck, crossfader.assign[deck.index()])?;
            }
            Ok(())
        });
    }

    fn keep_mixer_setting(
        &mut self,
        crossfader_curve: CrossfaderCurve,
        channel_fader_curve: ChannelFaderCurve,
        crossfader_assign: [CrossfaderAssign; DECK_COUNT],
        cx: &mut Context<'_, Self>,
    ) {
        self.settings.mixer =
            MixerSetting::of(crossfader_curve, channel_fader_curve, crossfader_assign);
        self.save_settings(cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_curve_has_a_label() {
        for curve in CrossfaderCurve::ALL {
            assert!(!crossfader_curve_label(curve).is_empty());
        }
        for curve in ChannelFaderCurve::ALL {
            assert!(!channel_fader_curve_label(curve).is_empty());
        }
    }
}
