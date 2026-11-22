//! The crossfader assign switches and the fader curves: rekordbox's mixer panel and its
//! Preferences > Controller > Mixer, kept in the settings.

use dubplate_engine::{ChannelFaderCurve, Crossfader, CrossfaderAssign, CrossfaderCurve, DeckId};
use gpui::Context;

use super::DubplateApp;
use super::notice::NoticePlace;
use crate::settings::MixerSetting;

/// What the window says when Smart Fader has nothing to move between.
const SMART_FADER_RESTS: &str =
    "Smart Fader rests until one deck is on A and the other on B of the crossfader.";
/// What the window says when Smart Fader is on with the crossfader off.
const SMART_FADER_NEEDS_CROSSFADER: &str =
    "Smart Fader needs the crossfader. Turn it on in Settings > Mixer or with Option+X.";

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
    /// The crossfader as the engine plays it: whether it is on, where the DJ put it, its curve
    /// and its sides.
    pub(super) fn crossfader(&self) -> Crossfader {
        let mixer = &self.settings.mixer;
        Crossfader {
            enabled: mixer.crossfader_on,
            position: self.mixer.crossfader,
            curve: mixer.crossfader_curve(),
            assign: DeckId::ALL.map(|deck| mixer.crossfader_assign(deck)),
        }
    }

    /// The channel fader curve the engine plays.
    pub(super) fn channel_fader_curve(&self) -> ChannelFaderCurve {
        self.settings.mixer.channel_fader_curve()
    }

    /// Whether Smart Fader is on but has no deck on one of the crossfader's sides, or the
    /// crossfader is off.
    pub(super) fn smart_fader_resting(&self) -> bool {
        self.smart_fader_on() && self.crossfader().sides().is_none()
    }

    /// Puts the crossfader in or out of use and remembers it.
    pub(super) fn toggle_crossfader(&mut self, cx: &mut Context<'_, Self>) {
        let crossfader = Crossfader {
            enabled: !self.crossfader().enabled,
            ..self.crossfader()
        };
        self.engine_command(cx, |controller| {
            controller.set_crossfader_enabled(crossfader.enabled)
        });
        self.keep_mixer_setting(crossfader, self.channel_fader_curve(), cx);
        self.explain_resting_smart_fader(cx);
    }

    /// Puts `deck`'s channel on side A or B of the crossfader, or THRU, and remembers it.
    pub(super) fn set_crossfader_assign(
        &mut self,
        deck: DeckId,
        assign: CrossfaderAssign,
        cx: &mut Context<'_, Self>,
    ) {
        let mut crossfader = self.crossfader();
        crossfader.assign[deck.index()] = assign;
        self.engine_command(cx, |controller| {
            controller.set_crossfader_assign(deck, assign)
        });
        self.keep_mixer_setting(crossfader, self.channel_fader_curve(), cx);
        self.explain_resting_smart_fader(cx);
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
        let crossfader = Crossfader {
            curve,
            ..self.crossfader()
        };
        self.keep_mixer_setting(crossfader, self.channel_fader_curve(), cx);
    }

    /// Chooses the channel fader curve and remembers it.
    pub(super) fn set_channel_fader_curve(
        &mut self,
        curve: ChannelFaderCurve,
        cx: &mut Context<'_, Self>,
    ) {
        self.engine_command(cx, |controller| controller.set_channel_fader_curve(curve));
        self.keep_mixer_setting(self.crossfader(), curve, cx);
    }

    /// Says why Smart Fader does nothing, when it is on with the crossfader off, a deck on
    /// THRU or both decks on one side.
    pub(super) fn explain_resting_smart_fader(&mut self, cx: &mut Context<'_, Self>) {
        if !self.smart_fader_resting() {
            return;
        }
        let why = if self.crossfader().enabled {
            SMART_FADER_RESTS
        } else {
            SMART_FADER_NEEDS_CROSSFADER
        };
        self.show_notice(NoticePlace::Window, why, cx);
    }

    /// Says that Smart Fader needs the crossfader, when the DJ asks for it with the
    /// crossfader off.
    pub(super) fn explain_unavailable_smart_fader(&mut self, cx: &mut Context<'_, Self>) {
        self.show_notice(NoticePlace::Window, SMART_FADER_NEEDS_CROSSFADER, cx);
    }

    /// Gives a fresh engine the crossfader and fader curves kept in the settings.
    pub(super) fn restore_mixer_switches(&mut self, cx: &mut Context<'_, Self>) {
        let crossfader = self.crossfader();
        let channel_fader_curve = self.channel_fader_curve();
        self.engine_command(cx, |controller| {
            controller.set_crossfader_enabled(crossfader.enabled)?;
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
        crossfader: Crossfader,
        channel_fader_curve: ChannelFaderCurve,
        cx: &mut Context<'_, Self>,
    ) {
        self.settings.mixer = MixerSetting::of(&crossfader, channel_fader_curve);
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
