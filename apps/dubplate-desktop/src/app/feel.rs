//! QUANTIZE and each deck's VINYL switch, as in rekordbox's global section and on each deck,
//! kept in the settings.

use dubplate_engine::DeckId;
use gpui::Context;

use super::DubplateApp;

impl DubplateApp {
    /// Whether cues, loops, hot cues and beat jumps land on the beat grid.
    pub(super) fn quantize(&self) -> bool {
        self.settings.quantize
    }

    /// Whether `deck`'s jog top holds the record (vinyl mode) rather than only nudging.
    pub(super) fn vinyl_mode(&self, deck: DeckId) -> bool {
        self.settings.vinyl_mode[deck.index()]
    }

    /// Turns quantize on or off for both decks and remembers it.
    pub(super) fn toggle_quantize(&mut self, cx: &mut Context<'_, Self>) {
        let enabled = !self.quantize();
        self.settings.quantize = enabled;
        self.engine_command(cx, |controller| controller.set_quantize(enabled));
        self.save_settings(cx);
        self.update_lights();
        cx.notify();
    }

    /// Switches `deck` between vinyl and CDJ jog mode and remembers it.
    pub(super) fn toggle_vinyl_mode(&mut self, deck: DeckId, cx: &mut Context<'_, Self>) {
        let enabled = !self.vinyl_mode(deck);
        self.settings.vinyl_mode[deck.index()] = enabled;
        self.engine_command(cx, |controller| controller.set_vinyl_mode(deck, enabled));
        self.save_settings(cx);
        self.update_lights();
        cx.notify();
    }

    /// Gives a fresh engine the quantize and jog modes kept in the settings.
    pub(super) fn restore_jog_and_quantize(&mut self, cx: &mut Context<'_, Self>) {
        let quantize = self.quantize();
        let vinyl_mode = self.settings.vinyl_mode;
        self.engine_command(cx, |controller| {
            controller.set_quantize(quantize)?;
            for deck in DeckId::ALL {
                controller.set_vinyl_mode(deck, vinyl_mode[deck.index()])?;
            }
            Ok(())
        });
    }
}
