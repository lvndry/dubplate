//! The tracks the decks played this session, which the library's `played:` filter reads.

use std::path::Path;
use std::sync::Arc;

use dubplate_engine::DeckId;
use gpui::Context;

use super::{DeckFrame, DubplateApp};

impl DubplateApp {
    /// Marks the tracks that started playing. Cheap enough for every frame: it only looks the
    /// playing decks' files up, and copies a path only the first time it plays.
    pub(super) fn mark_played(&mut self, frames: &[DeckFrame], cx: &mut Context<'_, Self>) {
        let mut newly_played = false;
        for (deck, frame) in DeckId::ALL.into_iter().zip(frames) {
            if !frame.state.playing {
                continue;
            }
            let newly_started = self
                .deck(deck)
                .track
                .as_ref()
                .and_then(|track| track.track.file_path.as_deref())
                .filter(|path| !self.played_tracks.contains(*path))
                .map(Path::to_path_buf);
            if let Some(path) = newly_started {
                Arc::make_mut(&mut self.played_tracks).insert(path);
                newly_played = true;
            }
        }
        if newly_played {
            let played = Arc::clone(&self.played_tracks);
            self.browser
                .update(cx, |browser, cx| browser.set_played(played, cx));
        }
    }
}
