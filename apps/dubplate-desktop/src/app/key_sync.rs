//! KEY SYNC, as on rekordbox's decks: a deck's key moves to match the other deck's, to the same
//! key or its relative major or minor, by the fewest semitones.

use dubplate_engine::DeckId;
use dubplate_prep::Key;
use gpui::Context;

use super::DubplateApp;
use super::notice::NoticePlace;
use crate::analysis::{AnalysisState, track_key};
use crate::transport::other_deck;

/// The semitones, from -6 to 5, that put `own` on the same Camelot number as `target`: the
/// same key, or its relative major or minor. A semitone up moves a key seven steps round the
/// wheel, and seven is its own inverse modulo twelve.
pub fn key_sync_shift(own: Key, target: Key) -> i8 {
    let steps =
        (i16::from(target.camelot_number()) - i16::from(own.camelot_number())).rem_euclid(12);
    let semitones = (steps * 7).rem_euclid(12);
    let semitones = if semitones > 5 {
        semitones - 12
    } else {
        semitones
    };
    semitones as i8
}

impl DubplateApp {
    /// KEY SYNC: moves `deck`'s key to match the other deck's, or back to the track's own key
    /// when key sync already moved it.
    pub(super) fn toggle_key_sync(&mut self, deck: DeckId, cx: &mut Context<'_, Self>) {
        let shift = if self.key_synced(deck) {
            0
        } else {
            let (Some(own), Some(target)) =
                (self.deck_key(deck), self.playing_key(other_deck(deck)))
            else {
                self.show_notice(
                    NoticePlace::Deck(deck),
                    "Key sync needs the key of the tracks on both decks.",
                    cx,
                );
                return;
            };
            i32::from(key_sync_shift(own, target))
        };
        if shift == self.key_shift(deck) {
            self.show_notice(NoticePlace::Deck(deck), "Already in key.", cx);
            return;
        }
        self.engine_command(cx, |controller| {
            controller.set_key_shift(deck, shift as f32)
        });
    }

    /// Whether key sync has moved `deck` to the other deck's key, and it still matches.
    pub(super) fn key_synced(&self, deck: DeckId) -> bool {
        self.key_shift(deck) != 0
            && self
                .playing_key(deck)
                .zip(self.playing_key(other_deck(deck)))
                .is_some_and(|(own, other)| own.camelot_number() == other.camelot_number())
    }

    /// The key `deck` sounds in: its track's key moved by the deck's key shift.
    fn playing_key(&self, deck: DeckId) -> Option<Key> {
        let shift = self.key_shift(deck);
        self.deck_key(deck).map(|key| key.transposed(shift as i8))
    }

    /// The key of the track on `deck`, as the library or the analysis knows it.
    fn deck_key(&self, deck: DeckId) -> Option<Key> {
        let entry = self.deck(deck).track.as_ref()?;
        let analysis = entry
            .track
            .file_path
            .as_deref()
            .map_or(AnalysisState::Waiting, |path| self.analyses.state(path));
        track_key(entry, analysis)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(camelot: &str) -> Key {
        Key::parse_file_key(camelot).unwrap()
    }

    #[test]
    fn key_sync_moves_to_the_same_number_by_the_fewest_semitones() {
        assert_eq!(key_sync_shift(key("8A"), key("8A")), 0);
        assert_eq!(key_sync_shift(key("8A"), key("3A")), 1);
        assert_eq!(key_sync_shift(key("3A"), key("8A")), -1);
        assert_eq!(key_sync_shift(key("8A"), key("10A")), 2);
        assert_eq!(
            key_sync_shift(key("8A"), key("8B")),
            0,
            "the relative major"
        );
        for own in 1..=12 {
            for target in 1..=12 {
                let own_key = key(&format!("{own}A"));
                let target_key = key(&format!("{target}B"));
                let shift = key_sync_shift(own_key, target_key);
                assert!((-6..=5).contains(&shift));
                assert_eq!(
                    own_key.transposed(shift).camelot_number(),
                    target_key.camelot_number()
                );
            }
        }
    }
}
