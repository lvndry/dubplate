//! Keeping the library's Match column on the track the DJ mixes from: the deck with MASTER
//! pressed, else the deck the room hears, else the deck loaded last. The matches are worked out off the interface
//! thread once per change of that track, its tempo or the catalogue.

use std::sync::Arc;
use std::time::Instant;

use dubplate_engine::{DeckId, MasterTempo};
use gpui::Context;

use super::DubplateApp;
use crate::library_match::LibraryMatches;

/// What the matches were worked out for: a change to any of it works them out again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct MatchSource {
    deck: DeckId,
    load_generation: u64,
    /// The tempo the deck plays at, in tenths of a BPM: finer moves of the tempo fader change
    /// no match worth working out again.
    tempo_tenths: Option<i64>,
}

impl DubplateApp {
    /// Works the matches out again when the track they follow changed. Cheap enough for every
    /// frame: it only compares.
    pub(super) fn follow_library_matches(&mut self, cx: &mut Context<'_, Self>) {
        let Some(source) = self.match_source() else {
            return;
        };
        if self.match_source == Some(source) {
            return;
        }
        self.match_source = Some(source);
        let Some(mut reference) = self
            .deck(source.deck)
            .track
            .as_ref()
            .and_then(|track| track.track.file_path.as_deref())
            .and_then(|path| self.catalogue.get(path))
            .cloned()
        else {
            return;
        };
        if let Some(bpm) = self.tempo_state(source.deck).and_then(|state| state.bpm) {
            reference.bpm = Some(bpm as f32);
        }
        let catalogue = Arc::clone(&self.catalogue);
        cx.spawn(async move |this, cx| {
            let matches = cx
                .background_executor()
                .spawn(
                    async move { LibraryMatches::new(source.deck, &reference, &catalogue.tracks) },
                )
                .await;
            this.update(cx, |this, cx| {
                if this.match_source == Some(source) {
                    let matches = Arc::new(matches);
                    this.browser
                        .update(cx, |browser, cx| browser.set_matches(matches, cx));
                }
            })
            .ok();
        })
        .detach();
    }

    /// Works the matches out again on the next frame, as when the catalogue learnt a tempo,
    /// a key or an energy.
    pub(super) fn rematch_library(&mut self) {
        self.match_source = None;
    }

    /// The deck the matches follow: the one the DJ pressed MASTER on, even while it is stopped;
    /// else the playing deck the room hears; else the deck loaded last, so the DJ can browse
    /// for the next track before anything plays.
    fn match_deck(&self) -> Option<DeckId> {
        let chosen_master =
            self.engine
                .as_ref()
                .ok()
                .and_then(|engine| match engine.controller.master_tempo() {
                    MasterTempo::Deck(deck) => Some(deck),
                    MasterTempo::LouderDeck => None,
                });
        let loaded = DeckId::ALL.map(|deck| {
            let side = self.deck(deck);
            side.visuals().and(side.loaded_at())
        });
        reference_deck(chosen_master, self.on_air_deck(), loaded)
    }

    /// What the matches follow now, when a deck holds a track.
    fn match_source(&self) -> Option<MatchSource> {
        let deck = self.match_deck()?;
        Some(MatchSource {
            deck,
            load_generation: self.deck(deck).load_generation(),
            tempo_tenths: self
                .tempo_state(deck)
                .and_then(|state| state.bpm)
                .map(|bpm| (bpm * 10.0).round() as i64),
        })
    }
}

/// The deck to match against: `chosen_master` when it holds a track, else `on_air`, else the
/// deck whose track finished loading last, from when each deck's track loaded.
fn reference_deck(
    chosen_master: Option<DeckId>,
    on_air: Option<DeckId>,
    loaded_at: [Option<Instant>; 2],
) -> Option<DeckId> {
    let holds_track = |deck: &DeckId| loaded_at[deck.index()].is_some();
    chosen_master.filter(holds_track).or(on_air).or_else(|| {
        DeckId::ALL
            .into_iter()
            .filter(holds_track)
            .max_by_key(|deck| loaded_at[deck.index()])
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn the_reference_is_the_master_then_the_deck_on_air_then_the_last_loaded() {
        let earlier = Instant::now();
        let later = earlier + Duration::from_secs(1);
        let both = [Some(earlier), Some(later)];
        assert_eq!(
            reference_deck(Some(DeckId::A), Some(DeckId::B), both),
            Some(DeckId::A),
            "MASTER counts even while its deck is stopped"
        );
        assert_eq!(
            reference_deck(Some(DeckId::A), Some(DeckId::B), [None, Some(later)]),
            Some(DeckId::B),
            "a master deck without a track does not"
        );
        assert_eq!(reference_deck(None, Some(DeckId::A), both), Some(DeckId::A));
        assert_eq!(reference_deck(None, None, both), Some(DeckId::B));
        assert_eq!(reference_deck(None, None, [None, None]), None);
    }
}
