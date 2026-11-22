//! The window's side of Dubplate's analysis: asking for the decks' and the set's tracks before
//! the rest of the library, taking the analyses as they arrive, and giving their tempo, key,
//! energy and beat grid to the library, the set, the suggestions, the track check and the
//! decks.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use dubplate_engine::DeckId;
use gpui::Context;

use super::DubplateApp;
use crate::analysis::AnalysisPriority;
use crate::deck::{ANALYSED_GRID_NOTE, DeckContent, DeckGrid, GridSource};

/// How often the analyses that have arrived are taken while tracks are waiting.
const COLLECT_INTERVAL: Duration = Duration::from_millis(250);

impl DubplateApp {
    /// Asks for the analysis of the set's tracks not analysed yet, before anything else.
    pub(super) fn request_set_analysis(&mut self, cx: &mut Context<'_, Self>) {
        let set_tracks: Vec<PathBuf> = self
            .set_editor
            .as_ref()
            .map(|editor| {
                let set = editor.set();
                set.entries
                    .iter()
                    .map(|entry| entry.track_path.clone())
                    .collect()
            })
            .unwrap_or_default();
        self.request_analysis(
            set_tracks.iter().map(PathBuf::as_path),
            AnalysisPriority::Set,
            cx,
        );
    }

    /// Asks for the analysis of every other track of the library not analysed yet.
    pub(super) fn request_library_analysis(&mut self, cx: &mut Context<'_, Self>) {
        let catalogue = Arc::clone(&self.catalogue);
        self.request_analysis(
            catalogue
                .tracks
                .iter()
                .map(|track| track.track_path.as_path()),
            AnalysisPriority::Library,
            cx,
        );
    }

    /// Asks for the tracks' analysis at `priority` and takes the answers as they arrive.
    pub(super) fn request_analysis<'tracks>(
        &mut self,
        tracks: impl IntoIterator<Item = &'tracks Path>,
        priority: AnalysisPriority,
        cx: &mut Context<'_, Self>,
    ) {
        self.analyses.request(tracks, priority);
        cx.notify();
        self.collect_analyses_while_waiting(cx);
    }

    /// Gives a freshly built catalogue the analyses already made, while a rescan confirms them.
    pub(super) fn restore_analyses(&mut self) {
        self.rematch_library();
        let catalogue = Arc::make_mut(&mut self.catalogue);
        for (path, analysis) in self.analyses.found() {
            catalogue.set_analysis(path, analysis);
        }
    }

    /// Gives the track on `deck` the beat grid of Dubplate's analysis, when the track has no
    /// grid yet and its analysis has one. A grid from rekordbox or from the DJ's taps stays.
    pub(super) fn give_deck_analysed_grid(&mut self, deck: DeckId, cx: &mut Context<'_, Self>) {
        let deck_state = &self.decks[deck.index()];
        if deck_state.grid_source.is_some() {
            return;
        }
        let Some(track) = deck_state.track.clone() else {
            return;
        };
        let Some(analysis) = track
            .track
            .file_path
            .as_deref()
            .and_then(|path| self.analyses.get(path))
        else {
            return;
        };
        let Some(grid) = analysis
            .rekordbox_grid()
            .filter(|grid| !grid.beats.is_empty())
        else {
            return;
        };
        let fixed = DeckGrid::new(&grid, self.grid_correction(&track));
        let deck_state = &mut self.decks[deck.index()];
        let DeckContent::Loaded(visuals) = &mut deck_state.content else {
            return;
        };
        visuals.beats = Arc::new(fixed.beats);
        visuals.read_grid = Some(Arc::new(grid));
        if visuals.zoomed.is_none() {
            visuals.analysis_problem = Some(ANALYSED_GRID_NOTE.to_owned());
        }
        deck_state.grid_source = Some(GridSource::Analysis);
        self.engine_command(cx, |controller| {
            controller.set_beat_grid(deck, fixed.engine)
        });
    }

    fn collect_analyses_while_waiting(&mut self, cx: &mut Context<'_, Self>) {
        if self.analysis_collecting || self.analyses.waiting() == 0 {
            return;
        }
        self.analysis_collecting = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(COLLECT_INTERVAL).await;
                let waiting = this.update(cx, |this, cx| {
                    this.collect_analyses(cx);
                    let waiting = this.analyses.waiting() > 0;
                    this.analysis_collecting = waiting;
                    waiting
                });
                if !matches!(waiting, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    fn collect_analyses(&mut self, cx: &mut Context<'_, Self>) {
        let answered = self.analyses.collect();
        if answered.is_empty() {
            return;
        }
        self.rematch_library();
        let catalogue = Arc::make_mut(&mut self.catalogue);
        for (path, analysis) in &answered {
            if let Ok(analysis) = analysis {
                catalogue.set_analysis(path, analysis);
            }
        }
        for deck in DeckId::ALL {
            self.give_deck_analysed_grid(deck, cx);
        }
        self.check_again_with_analysed_grids(&answered, cx);
        self.refresh_prepare_view(cx);
        self.browser
            .update(cx, |browser, cx| browser.add_analyses(answered, cx));
        self.refresh_filter_reference(cx);
        cx.notify();
    }

    /// Checks again the set tracks whose analysis just brought a beat grid, so the track check
    /// shows it and proposes mix cues on it.
    fn check_again_with_analysed_grids(
        &mut self,
        answered: &[crate::analysis::AnalysisAnswer],
        cx: &mut Context<'_, Self>,
    ) {
        let Some(editor) = &self.set_editor else {
            return;
        };
        let in_set: HashSet<&Path> = editor
            .set()
            .entries
            .iter()
            .map(|entry| entry.track_path.as_path())
            .collect();
        let gridded: Vec<&Path> = answered
            .iter()
            .filter(|(path, analysis)| {
                in_set.contains(path.as_path())
                    && analysis
                        .as_ref()
                        .is_ok_and(|analysis| analysis.beat_grid.is_some())
            })
            .map(|(path, _)| path.as_path())
            .collect();
        if gridded.is_empty() {
            return;
        }
        if gridded
            .iter()
            .any(|path| self.checks_pending.contains(*path))
        {
            self.forget_track_checks();
        }
        for path in &gridded {
            self.track_checks.remove(*path);
        }
        let detail_path = self
            .detail_pending
            .as_deref()
            .or(self.track_detail.as_ref().map(|(path, _)| path.as_path()));
        if detail_path.is_some_and(|path| gridded.contains(&path)) {
            self.detail_pending = None;
            self.track_detail = None;
        }
        self.check_set_tracks(cx);
        self.ensure_track_detail(cx);
    }

    /// How many tracks are waiting for their analysis, for the header.
    pub(super) fn analysis_waiting(&self) -> usize {
        self.analyses.waiting()
    }
}
