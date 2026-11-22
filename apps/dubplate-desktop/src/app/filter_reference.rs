//! The track the library's quick filters start from, worked out when what it depends on
//! changes (a command, a load, a set edit, an analysis), never on every frame.

use dubplate_engine::DeckId;
use dubplate_prep::KeyFit;
use gpui::Context;

use super::DubplateApp;
use crate::analysis::{AnalysisState, track_key};
use crate::commands::Mode;
use crate::library_view::FilterReference;

impl DubplateApp {
    /// Hands the browser the track its quick filters start from now.
    pub(super) fn refresh_filter_reference(&mut self, cx: &mut Context<'_, Self>) {
        let reference = self.filter_reference();
        self.browser.update(cx, |browser, cx| {
            browser.set_filter_reference(reference, cx);
        });
    }

    /// In Prepare the selected set track, else the deck the room hears; in Perform the other
    /// way round.
    fn filter_reference(&self) -> Option<FilterReference> {
        match self.mode {
            Mode::Prepare => self
                .set_track_reference()
                .or_else(|| self.on_air_reference()),
            Mode::Perform => self
                .on_air_reference()
                .or_else(|| self.set_track_reference()),
        }
    }

    /// The selected set track, or the last one, as the Next suggestions follow it.
    fn set_track_reference(&self) -> Option<FilterReference> {
        let tracks = self.set_editor.as_ref()?.tracks();
        let position = self.set_selection.or_else(|| tracks.len().checked_sub(1))?;
        let track = self.catalogue.get(&tracks.get(position)?.track_path)?;
        Some(FilterReference {
            title: track.title.clone().into(),
            key: track.key.map(KeyFit::new),
            bpm: track.bpm.map(f64::from),
        })
    }

    /// The playing deck the room hears most, deck A when they are level.
    pub(super) fn on_air_deck(&self) -> Option<DeckId> {
        DeckId::ALL
            .into_iter()
            .filter(|deck| self.deck_state(*deck).playing)
            .reduce(|loudest, deck| {
                if self.on_air_level(deck) > self.on_air_level(loudest) {
                    deck
                } else {
                    loudest
                }
            })
    }

    /// The track of the playing deck the room hears most.
    fn on_air_reference(&self) -> Option<FilterReference> {
        let deck = self.on_air_deck()?;
        let track = self.deck(deck).track.as_ref()?;
        let analysis = track
            .track
            .file_path
            .as_deref()
            .map(|path| self.analyses.state(path))
            .unwrap_or(AnalysisState::Waiting);
        Some(FilterReference {
            title: track.row.title.clone(),
            key: track_key(track, analysis).map(KeyFit::new),
            bpm: self.tempo_state(deck).and_then(|state| state.bpm),
        })
    }

    /// How much of a deck the room hears from its channel fader and the crossfader, which
    /// lets both decks through in its middle half.
    fn on_air_level(&self, deck: DeckId) -> f32 {
        let crossfader_side = match deck {
            DeckId::A => 1.0 - self.mixer.crossfader,
            DeckId::B => self.mixer.crossfader,
        };
        self.mixer.channel_faders[deck.index()] * crossfader_side.min(0.5)
    }
}
