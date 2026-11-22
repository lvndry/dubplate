//! SMART CFX, the Sound Color FX and SMART FADER, as on the DDJ-FLX4 with rekordbox: SMART CFX
//! turns both channels' CFX knobs into the chosen preset, SHIFT + SMART CFX steps the presets;
//! with SMART CFX off each CFX knob plays its channel's Sound Color FX, the filter unless the DJ
//! picked another; SMART FADER makes the crossfader move a shared tempo and swap the bass.

use dubplate_engine::{DeckId, SmartCfx, SmartFaderSource};
use gpui::Context;

use super::DubplateApp;

/// The preset SMART CFX starts with until the DJ picks another.
const DEFAULT_PRESET: SmartCfx = SmartCfx::DubEcho;

/// A preset's name as the picker shows it.
pub(super) fn preset_label(preset: SmartCfx) -> &'static str {
    match preset {
        SmartCfx::Space => "Space",
        SmartCfx::DubEcho => "Dub echo",
        SmartCfx::Sweep => "Sweep",
        SmartCfx::Noise => "Noise",
        SmartCfx::Crush => "Crush",
        SmartCfx::Chop => "Chop",
        SmartCfx::Jet => "Jet",
        SmartCfx::Phase => "Phase",
        SmartCfx::Pitch => "Pitch",
    }
}

/// What a CFX knob plays, as the mixer names it: the filter or a Sound Color FX.
pub(super) fn colour_label(colour: Option<SmartCfx>) -> &'static str {
    colour.map_or("Filter", preset_label)
}

/// The name settings keep for a Sound Color FX, `filter` for the plain filter.
fn colour_name(colour: Option<SmartCfx>) -> String {
    colour.map_or_else(|| FILTER_NAME.to_owned(), |preset| preset.to_string())
}

const FILTER_NAME: &str = "filter";

/// The preset a saved name stands for.
fn preset_named(name: &str) -> Option<SmartCfx> {
    SmartCfx::ALL
        .into_iter()
        .find(|preset| preset.to_string() == name)
}

impl DubplateApp {
    /// The preset SMART CFX turns on.
    pub(super) fn smart_cfx_preset(&self) -> SmartCfx {
        self.settings
            .smart_cfx_preset
            .as_deref()
            .and_then(preset_named)
            .unwrap_or(DEFAULT_PRESET)
    }

    /// Whether SMART CFX is on.
    pub(super) fn smart_cfx_on(&self) -> bool {
        self.settings.smart_cfx_on
    }

    /// The Sound Color FX a channel's CFX knob plays with SMART CFX off; `None` is the filter.
    pub(super) fn cfx_colour(&self, deck: DeckId) -> Option<SmartCfx> {
        self.settings
            .cfx_colours
            .get(deck.index())
            .and_then(|name| preset_named(name))
    }

    /// Picks the Sound Color FX of one channel, or of both, and remembers it.
    pub(super) fn choose_cfx_colour(
        &mut self,
        decks: &[DeckId],
        colour: Option<SmartCfx>,
        cx: &mut Context<'_, Self>,
    ) {
        let mut colours: Vec<String> = DeckId::ALL
            .into_iter()
            .map(|deck| colour_name(self.cfx_colour(deck)))
            .collect();
        for deck in decks {
            colours[deck.index()] = colour_name(colour);
        }
        self.settings.cfx_colours = colours;
        self.save_settings(cx);
        self.apply_cfx(cx);
    }

    /// Whether SMART FADER is on.
    pub(super) fn smart_fader_on(&self) -> bool {
        self.engine
            .as_ref()
            .is_ok_and(|engine| engine.controller.smart_fader())
    }

    /// Turns SMART CFX on with the chosen preset on both channels, or back to the plain filter,
    /// and remembers which for the next launch.
    pub(super) fn toggle_smart_cfx(&mut self, cx: &mut Context<'_, Self>) {
        self.settings.smart_cfx_on = !self.smart_cfx_on();
        self.apply_cfx(cx);
        self.save_settings(cx);
    }

    /// Puts the CFX knobs back as they were when Dubplate closed: SMART CFX, or each channel's
    /// Sound Color FX.
    pub(super) fn restore_smart_cfx(&mut self, cx: &mut Context<'_, Self>) {
        self.apply_cfx(cx);
    }

    /// Picks the preset SMART CFX plays and remembers it; with SMART CFX on, the knobs take it
    /// at once.
    pub(super) fn choose_smart_cfx(&mut self, preset: SmartCfx, cx: &mut Context<'_, Self>) {
        self.settings.smart_cfx_preset = Some(preset.to_string());
        self.save_settings(cx);
        if self.smart_cfx_on() {
            self.apply_cfx(cx);
        }
        cx.notify();
    }

    /// Gives each channel's CFX knob what it plays: the SMART CFX preset while SMART CFX is
    /// on, else the channel's Sound Color FX.
    fn apply_cfx(&mut self, cx: &mut Context<'_, Self>) {
        let played = DeckId::ALL.map(|deck| {
            if self.smart_cfx_on() {
                Some(self.smart_cfx_preset())
            } else {
                self.cfx_colour(deck)
            }
        });
        self.engine_command(cx, |controller| {
            for deck in DeckId::ALL {
                controller.set_smart_cfx(deck, played[deck.index()])?;
            }
            Ok(())
        });
        cx.notify();
    }

    /// Turns SMART FADER on or off.
    pub(super) fn toggle_smart_fader(&mut self, cx: &mut Context<'_, Self>) {
        let enabled = !self.smart_fader_on();
        if enabled
            && !self.crossfader().enabled
            && self.smart_fader_source() != SmartFaderSource::ChannelFaders
        {
            self.explain_unavailable_smart_fader(cx);
            return;
        }
        self.engine_command(cx, |controller| controller.set_smart_fader(enabled));
        self.explain_resting_smart_fader(cx);
        cx.notify();
    }

    /// Opens or closes the Smart CFX preset picker.
    pub(super) fn toggle_smart_cfx_menu(&mut self, cx: &mut Context<'_, Self>) {
        self.smart_cfx_menu = !self.smart_cfx_menu;
        cx.notify();
    }

    /// Closes the Smart CFX preset picker.
    pub(super) fn close_smart_cfx_menu(&mut self, cx: &mut Context<'_, Self>) {
        if self.smart_cfx_menu {
            self.smart_cfx_menu = false;
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_has_a_label_and_comes_back_by_its_saved_name() {
        for preset in SmartCfx::ALL {
            assert!(!preset_label(preset).is_empty());
            assert_eq!(preset_named(&preset.to_string()), Some(preset));
        }
        assert_eq!(preset_named("nonsense"), None);
        assert_eq!(preset_named(&colour_name(None)), None);
        assert_eq!(colour_label(None), "Filter");
        assert_eq!(
            preset_named(&colour_name(Some(SmartCfx::Sweep))),
            Some(SmartCfx::Sweep)
        );
    }
}
