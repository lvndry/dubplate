//! What the DJ can do with the selected track beyond loading it: show its file in Finder, copy
//! its path, separate its stems and look for it when it moved. The selected track is the set's
//! when the keys are on the set, else the library's, so the keys, the menu bar and a track's
//! context menu all act on the same one.

use std::path::PathBuf;

use gpui::{App, ClipboardItem, Context};

use super::DubplateApp;
use super::notice::NoticePlace;
use super::prepare::PrepareFocus;
use crate::collection::TrackIndex;
use crate::commands::Mode;
use crate::finder;

impl DubplateApp {
    /// The track the keys act on: the selected set track while the keys are on the set in
    /// Prepare, else the track selected in the library.
    pub(super) fn selected_track(&self, cx: &App) -> Option<TrackIndex> {
        if self.mode == Mode::Prepare && self.prepare_focus == PrepareFocus::Set {
            let path = self.selected_set_path()?;
            return self.collection()?.find_path(&path);
        }
        self.browser.read(cx).selected()
    }

    /// Whether the keys act on the set's tracks rather than the library's.
    pub(super) fn keys_on_set(&self, _cx: &App) -> bool {
        self.mode == Mode::Prepare && self.prepare_focus == PrepareFocus::Set
    }

    /// The tracks an edit applies to: the library's selected tracks, or the selected set track
    /// while the keys are on the set.
    pub(super) fn selected_tracks(&self, cx: &App) -> Vec<TrackIndex> {
        if self.keys_on_set(cx) {
            return self.selected_track(cx).into_iter().collect();
        }
        self.browser.read(cx).selected_tracks()
    }

    /// The file of the selected track, when it has one.
    fn selected_file(&self, cx: &App) -> Option<PathBuf> {
        self.track_file(self.selected_track(cx)?)
    }

    /// Shows the selected track's file in Finder.
    pub(super) fn reveal_selected(&mut self, cx: &mut Context<'_, Self>) {
        let Some(path) = self.selected_file(cx) else {
            return;
        };
        if !path.is_file() {
            self.show_notice(
                NoticePlace::Window,
                "The file is missing, so Finder cannot show it. Relink it first.",
                cx,
            );
            return;
        }
        if let Err(problem) = finder::reveal(&path) {
            self.show_notice(
                NoticePlace::Window,
                format!("Finder could not show the file: {problem}"),
                cx,
            );
        }
    }

    /// Copies the selected track's file path.
    pub(super) fn copy_selected_path(&mut self, cx: &mut Context<'_, Self>) {
        let paths: Vec<String> = self
            .selected_tracks(cx)
            .into_iter()
            .filter_map(|track| self.track_file(track))
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        if !paths.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(paths.join("\n")));
        }
    }

    /// Separates the selected track's stems now.
    pub(super) fn separate_selected_stems(&mut self, cx: &mut Context<'_, Self>) {
        let paths: Vec<PathBuf> = self
            .selected_tracks(cx)
            .into_iter()
            .filter_map(|track| self.track_file(track))
            .collect();
        for path in paths {
            self.separate_stems_now(path, cx);
        }
    }

    /// Looks for the moved file of the selected track.
    pub(super) fn relink_selected(&mut self, cx: &mut Context<'_, Self>) {
        if let Some(track) = self.selected_track(cx) {
            self.start_relink(Some(track), cx);
        }
    }
}
