//! Keeping the library's Match column on the track the room follows: the tempo master's, or
//! the deck the room hears when no deck leads. The matches are worked out off the interface
//! thread once per change of that track, its tempo or the catalogue.

use std::sync::Arc;

use dubplate_engine::DeckId;
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
                .spawn(async move { LibraryMatches::new(&reference, &catalogue.tracks) })
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

    /// The deck the matches follow: the tempo master, else the deck the room hears.
    fn match_source(&self) -> Option<MatchSource> {
        let leading = DeckId::ALL.into_iter().find(|deck| {
            self.deck(*deck).visuals().is_some()
                && self.tempo_state(*deck).is_some_and(|state| state.is_master)
        });
        let deck = leading.or_else(|| self.on_air_deck())?;
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
