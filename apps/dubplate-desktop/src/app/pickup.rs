//! Pick-up, as rekordbox shows it: a tempo slider shows the tempo the deck really plays, and
//! a fader or knob that no longer matches what it controls, after sync, Smart Fader or a move
//! on the other surface, shows where it waits until the DJ moves it through.

use dubplate_engine::DeckId;

use super::DubplateApp;
use crate::controls::{Control, MixerPositions, tempo_slider_at};

/// How far apart a waiting fader and the value on screen must be before the screen shows both.
const SHOWN_GAP: f32 = 0.005;

/// What a tempo slider shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct TempoSliderView {
    /// Where the thumb sits: the tempo the deck plays.
    pub thumb: f32,
    /// Where a fader that has not taken over yet sits, on the controller or on screen.
    pub waiting_fader: Option<f32>,
}

impl DubplateApp {
    /// A deck's tempo slider as it shows: the playing tempo, and the fader still to pick it up.
    /// A deck following the tempo master has nothing to pick up: sync holds its tempo.
    pub(super) fn tempo_slider_view(&self, deck: DeckId) -> TempoSliderView {
        let fader = self.mixer.tempo_sliders[deck.index()];
        let Some(state) = self.tempo_state(deck) else {
            return TempoSliderView {
                thumb: fader,
                waiting_fader: None,
            };
        };
        let thumb = if state.fader_waits_for_pickup {
            tempo_slider_at(self.deck_state(deck).tempo, state.tempo_range)
        } else {
            fader
        };
        if state.following {
            return TempoSliderView {
                thumb,
                waiting_fader: None,
            };
        }
        let controller = self
            .hardware
            .as_ref()
            .and_then(|hardware| hardware.dispatcher.tempo_waiting(deck, thumb));
        let on_screen = state.fader_waits_for_pickup.then_some(fader);
        TempoSliderView {
            thumb,
            waiting_fader: controller
                .or(on_screen)
                .filter(|waiting| (waiting - thumb).abs() > SHOWN_GAP),
        }
    }

    /// The faders and knobs as the screen shows them, which the controller's pick-up compares
    /// itself with: the tempo sliders at the tempo each deck plays.
    pub(super) fn shown_positions(&self) -> MixerPositions {
        let mut positions = self.mixer;
        positions.tempo_sliders = DeckId::ALL.map(|deck| self.tempo_slider_view(deck).thumb);
        positions
    }

    /// Where a fader or knob shows, from 0.0 to 1.0.
    pub(super) fn control_fraction(&self, control: Control) -> f32 {
        match control {
            Control::Tempo(deck) => self.tempo_slider_view(deck).thumb,
            other => self.mixer.fraction(other),
        }
    }
}
