//! Fixing a track's beat grid from its deck, as rekordbox's deck grid edit does: the downbeat a
//! beat either way, the grid 5 ms either way, and confirming it by ear. A track keeps one grid
//! fix by its file, so the deck, the track check, sync and the export all read the same one.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dubplate_engine::DeckId;
use dubplate_prep::{GridEdit, Set, TrackGrid};
use gpui::prelude::*;
use gpui::{Context, div, px};

use super::DubplateApp;
use super::grid_controls;
use super::notice::NoticePlace;
use crate::commands::Command;
use crate::deck::{DeckContent, DeckGrid};
use crate::theme;

impl DubplateApp {
    /// The DJ's fix of the grid of the track whose file is `path`.
    pub(super) fn track_grid(&self, path: &Path) -> TrackGrid {
        self.track_grids.get(path).copied().unwrap_or_default()
    }

    /// The file of the track on `deck` and its grid fix, when its grid can be fixed: one read
    /// from rekordbox or Dubplate's analysis, not a tapped one.
    pub(super) fn deck_track_grid(&self, deck: DeckId) -> Option<(&Path, TrackGrid)> {
        let side = self.deck(deck);
        side.visuals()?.read_grid.as_ref()?;
        let path = side.track.as_ref()?.track.file_path.as_deref()?;
        Some((path, self.track_grid(path)))
    }

    /// Opens `deck`'s grid bar, closing the other deck's, or closes it.
    pub(super) fn toggle_grid_bar(&mut self, deck: DeckId) {
        self.grid_bar = (self.grid_bar != Some(deck)).then_some(deck);
    }

    /// Makes `edit` to the grid of the track on `deck`. The deck shows it at once and keeps its
    /// place in the track, playing or not; the store keeps it for every set and deck.
    pub(super) fn edit_deck_grid(
        &mut self,
        deck: DeckId,
        edit: GridEdit,
        cx: &mut Context<'_, Self>,
    ) {
        let Some((path, grid)) = self
            .deck_track_grid(deck)
            .map(|(path, grid)| (path.to_path_buf(), grid.edited(edit)))
        else {
            self.show_notice(
                NoticePlace::Deck(deck),
                "This deck has no analysed beat grid to fix.",
                cx,
            );
            return;
        };
        self.show_track_grid(&path, grid, cx);
        self.queue_set_edit(
            move |editor| editor.set_track_grid(&path, grid),
            |_, (), _| {},
            cx,
        );
    }

    /// Takes the grid fixes a set edit made, such as one from the track check, so a deck
    /// playing one of those tracks follows it.
    pub(super) fn follow_set_grids(
        &mut self,
        before: &Set,
        after: &Set,
        cx: &mut Context<'_, Self>,
    ) {
        for (path, grid) in grid_changes(before, after) {
            if self.track_grid(&path) != grid {
                self.show_track_grid(&path, grid, cx);
            }
        }
    }

    /// Keeps every track's grid fix as the store has them, and puts them on the decks loaded
    /// before the store was open.
    pub(super) fn take_track_grids(
        &mut self,
        grids: HashMap<PathBuf, TrackGrid>,
        cx: &mut Context<'_, Self>,
    ) {
        for (path, grid) in grids {
            self.show_track_grid(&path, grid, cx);
        }
    }

    /// Keeps `grid` as the fix of the track whose file is `path` and moves the grid of every
    /// deck holding that track to it, without moving the deck in the track.
    fn show_track_grid(&mut self, path: &Path, grid: TrackGrid, cx: &mut Context<'_, Self>) {
        self.track_grids.insert(path.to_path_buf(), grid);
        for deck in DeckId::ALL {
            let side = &mut self.decks[deck.index()];
            let holds_track = side
                .track
                .as_ref()
                .is_some_and(|track| track.track.file_path.as_deref() == Some(path));
            let DeckContent::Loaded(visuals) = &mut side.content else {
                continue;
            };
            let Some(read_grid) = visuals.read_grid.as_deref().filter(|_| holds_track) else {
                continue;
            };
            let fixed = DeckGrid::new(read_grid, grid.correction);
            visuals.beats = Arc::new(fixed.beats);
            self.engine_command(cx, |controller| {
                controller.set_beat_grid(deck, fixed.engine)
            });
        }
        cx.notify();
    }
}

/// The grids a set edit changed, by file: those that moved or were confirmed, and those of
/// tracks it brought into the set.
fn grid_changes(before: &Set, after: &Set) -> Vec<(PathBuf, TrackGrid)> {
    let previous: HashMap<&Path, TrackGrid> = before
        .entries
        .iter()
        .map(|entry| (entry.track_path.as_path(), TrackGrid::of(entry)))
        .collect();
    after
        .entries
        .iter()
        .map(|entry| (entry.track_path.as_path(), TrackGrid::of(entry)))
        .filter(|(path, grid)| previous.get(path) != Some(grid))
        .map(|(path, grid)| (path.to_path_buf(), grid))
        .collect()
}

/// The deck's grid bar, under its transport while open: the grid keys as buttons, for the
/// track on the deck.
pub(super) fn grid_bar(
    app: &DubplateApp,
    deck: DeckId,
    cx: &mut Context<'_, DubplateApp>,
) -> Option<impl IntoElement + use<>> {
    if app.grid_bar != Some(deck) {
        return None;
    }
    let content = match app.deck_track_grid(deck) {
        Some((_, grid)) => grid_controls::render(
            deck.index(),
            grid,
            move |edit| Command::EditDeckGrid(deck, edit),
            cx,
        )
        .into_any_element(),
        None => div()
            .text_size(theme::SMALL_TEXT_SIZE)
            .text_color(theme::MUTED)
            .child("Load an analysed track to fix its beat grid.")
            .into_any_element(),
    };
    Some(
        div()
            .id(("grid-bar", deck.index()))
            .role(gpui::Role::Group)
            .aria_label("Beat grid")
            .flex()
            .items_center()
            .h(px(32.0))
            .px(px(8.0))
            .rounded(theme::CONTROL_RADIUS)
            .bg(theme::RAISED)
            .child(content),
    )
}

#[cfg(test)]
mod tests {
    use dubplate_prep::{SetEntry, SetId};

    use super::*;

    fn set(entries: Vec<SetEntry>) -> Set {
        Set {
            id: SetId(1),
            name: String::new(),
            target_minutes: 60,
            created_at: 0,
            updated_at: 0,
            entries,
            seams: Vec::new(),
        }
    }

    fn entry(path: &str, grid: TrackGrid) -> SetEntry {
        let mut entry = SetEntry {
            track_path: PathBuf::from(path),
            ..SetEntry::default()
        };
        grid.write_to(&mut entry);
        entry
    }

    #[test]
    fn only_the_grids_an_edit_changed_are_taken() {
        let nudged = TrackGrid::default().nudged(5);
        let before = set(vec![
            entry("/a.mp3", TrackGrid::default()),
            entry("/b.mp3", nudged),
        ]);
        let after = set(vec![
            entry("/b.mp3", nudged),
            entry("/a.mp3", TrackGrid::default().confirmation_toggled()),
            entry("/c.mp3", nudged),
        ]);
        let changes = grid_changes(&before, &after);
        assert_eq!(
            changes,
            [
                (
                    PathBuf::from("/a.mp3"),
                    TrackGrid::default().confirmation_toggled()
                ),
                (PathBuf::from("/c.mp3"), nudged),
            ]
        );
    }
}
