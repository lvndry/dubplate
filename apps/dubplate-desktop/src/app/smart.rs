//! SMART CFX and SMART FADER, as on the DDJ-FLX4 with rekordbox: SMART CFX turns both channels'
//! filter knobs into the chosen preset, SHIFT + SMART CFX steps the presets, and SMART FADER
//! makes the crossfader move a shared tempo and swap the bass.

use dubplate_engine::{DeckId, SmartCfx};
use gpui::Context;

use super::DubplateApp;

/// The preset SMART CFX starts with until the DJ picks another.
const DEFAULT_PRESET: SmartCfx = SmartCfx::DubEcho;

/// A preset's name as the picker shows it.
pub(super) fn preset_label(preset: SmartCfx) -> &'static str {
    match preset {
        SmartCfx::Space => "Space",
        SmartCfx::DubEcho => "Dub echo",
        SmartCfx::Crush => "Crush",
        SmartCfx::Chop => "Chop",
        SmartCfx::Jet => "Jet",
        SmartCfx::Phase => "Phase",
        SmartCfx::Pitch => "Pitch",
    }
}

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
        self.engine
            .as_ref()
            .is_ok_and(|engine| engine.controller.smart_cfx(DeckId::A).is_some())
    }

    /// Whether SMART FADER is on.
    pub(super) fn smart_fader_on(&self) -> bool {
        self.engine
            .as_ref()
            .is_ok_and(|engine| engine.controller.smart_fader())
    }

    /// Turns SMART CFX on with the chosen preset on both channels, or back to the plain filter.
    pub(super) fn toggle_smart_cfx(&mut self, cx: &mut Context<'_, Self>) {
        let preset = (!self.smart_cfx_on()).then(|| self.smart_cfx_preset());
        self.apply_smart_cfx(preset, cx);
    }

    /// Picks the preset SMART CFX plays and remembers it; with SMART CFX on, the knobs take it
    /// at once.
    pub(super) fn choose_smart_cfx(&mut self, preset: SmartCfx, cx: &mut Context<'_, Self>) {
        self.settings.smart_cfx_preset = Some(preset.to_string());
        self.save_settings(cx);
        if self.smart_cfx_on() {
            self.apply_smart_cfx(Some(preset), cx);
        }
        cx.notify();
    }

    fn apply_smart_cfx(&mut self, preset: Option<SmartCfx>, cx: &mut Context<'_, Self>) {
        self.engine_command(cx, |controller| {
            for deck in DeckId::ALL {
                controller.set_smart_cfx(deck, preset)?;
            }
            Ok(())
        });
        cx.notify();
    }

    /// Turns SMART FADER on or off.
    pub(super) fn toggle_smart_fader(&mut self, cx: &mut Context<'_, Self>) {
        let enabled = !self.smart_fader_on();
        self.engine_command(cx, |controller| controller.set_smart_fader(enabled));
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
    }
}
