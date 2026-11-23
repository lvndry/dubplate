//! The window's side of Auto Gain: the gain each deck's track loaded with, applied with the
//! channel's TRIM knob, and the switch in Settings.

use dubplate_engine::DeckId;
use gpui::Context;

use super::DubplateApp;
use super::knob::KnobMark;
use crate::auto_gain::{AutoGain, applied_trim_fraction, channel_gain};
use crate::controls::Control;

impl DubplateApp {
    /// Gives the track on `deck` its Auto Gain, when Auto Gain is on, as loading it does.
    pub(super) fn level_deck(&mut self, deck: DeckId, cx: &mut Context<'_, Self>) {
        self.auto_gains[deck.index()] = if self.settings.auto_gain {
            self.track_auto_gain(deck)
        } else {
            None
        };
        self.apply_trim(deck, cx);
    }

    /// Levels the stopped decks whose track had no Auto Gain yet, once its analysis has come
    /// in. A playing deck keeps its level until its next load, so the music never jumps.
    pub(super) fn level_decks_with_new_analysis(&mut self, cx: &mut Context<'_, Self>) {
        for deck in DeckId::ALL {
            if self.auto_gains[deck.index()].is_none() && !self.deck_state(deck).playing {
                self.level_deck(deck, cx);
            }
        }
    }

    /// The Auto Gain of the track on `deck`: rekordbox's, else from Dubplate's analysis.
    fn track_auto_gain(&self, deck: DeckId) -> Option<AutoGain> {
        let track = self.deck(deck).track.as_ref()?;
        let loudness = track
            .track
            .file_path
            .as_deref()
            .and_then(|path| self.analyses.get(path))
            .and_then(|analysis| analysis.loudness);
        AutoGain::of(track.track.auto_gain_db, loudness)
    }

    /// Sets the channel's gain from its TRIM knob and its track's Auto Gain.
    pub(super) fn apply_trim(&mut self, deck: DeckId, cx: &mut Context<'_, Self>) {
        let gain = channel_gain(
            self.mixer.trims[deck.index()],
            self.auto_gains[deck.index()],
        );
        self.engine_command(cx, |controller| controller.set_gain(deck, gain));
    }

    /// The Auto Gain the track on `deck` plays with, for the TRIM knob.
    pub(super) fn auto_gain(&self, deck: DeckId) -> Option<AutoGain> {
        self.auto_gains[deck.index()]
    }

    /// Where Auto Gain puts the level of the TRIM knob `control`, for the knob to mark beside
    /// its own position, which stays the DJ's and the controller's.
    pub(super) fn knob_mark(&self, control: Control) -> Option<KnobMark> {
        let Control::Trim(deck) = control else {
            return None;
        };
        let auto_gain = self.auto_gain(deck)?;
        let fraction = applied_trim_fraction(self.mixer.trims[deck.index()], Some(auto_gain))?;
        Some(KnobMark {
            fraction,
            label: auto_gain.label().into(),
        })
    }

    /// Turns Auto Gain on or off for every deck at once, and remembers it.
    pub(super) fn set_auto_gain(&mut self, on: bool, cx: &mut Context<'_, Self>) {
        self.settings.auto_gain = on;
        self.save_settings(cx);
        for deck in DeckId::ALL {
            self.level_deck(deck, cx);
        }
        cx.notify();
    }
}
