//! The window's side of set preparation: editing the set from the keys and the mouse, checking
//! each set track's file and analysis in the background, the Next suggestions, and loading the
//! set onto the decks.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use dubplate_engine::DeckId;
use dubplate_prep::{
    AudioAnalysis, EnergyDirection, GridCorrection, PrepTrack, ReadyProblem, ReadyReport, SetEntry,
    SetTrack, SetWarning, StoreError, WarningPosition, set_warnings, suggest_next,
};
use gpui::{App, Context};

use super::DubplateApp;
use super::notice::NoticePlace;
use crate::collection::{TrackIndex, TrackOrigin};
use crate::commands::{Direction, Mode, SetEdit};
use crate::deck::deck_letter;
use crate::energy::{EnergyRange, direction_toward_target, set_curve};
use crate::loading::{LoadSource, check_present};
use crate::prep::{
    SeamSummary, TrackCheck, accept_proposal, check_track, nearest_bar_ms, readiness,
};
use crate::set::{SetEditor, SetTrackRef};

/// How many tracks the Next panel suggests.
const SUGGESTIONS: usize = 5;

/// Which part of the Prepare screen the arrow keys and Enter work on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum PrepareFocus {
    /// The library's tracks.
    #[default]
    Library,
    /// The set's tracks.
    Set,
    /// The Next suggestions.
    Next,
}

impl PrepareFocus {
    /// The part Tab moves to, or Shift-Tab with [`Direction::Up`].
    pub(super) fn step(self, direction: Direction) -> Self {
        match (self, direction) {
            (Self::Library, Direction::Down) | (Self::Next, Direction::Up) => Self::Set,
            (Self::Set, Direction::Down) | (Self::Library, Direction::Up) => Self::Next,
            (Self::Next, Direction::Down) | (Self::Set, Direction::Up) => Self::Library,
        }
    }
}

/// A set track as the panel shows it.
#[derive(Clone)]
pub(super) struct SetRow {
    /// Its place in the set, from 0.
    pub position: usize,
    /// The track in the library, when the library holds its file.
    pub track: Option<PrepTrack>,
    /// How long it plays.
    pub duration: Option<Duration>,
    /// Whether it is ready to mix, and what is missing.
    pub ready: ReadyReport,
    /// Whether a proposal could fill a missing mix cue.
    pub has_proposal: bool,
    /// Whether the seam into the next track is left to the moment.
    pub free_into_next: bool,
}

/// A suggestion of the Next panel.
#[derive(Clone)]
pub(super) struct NextTrack {
    /// The track's audio file.
    pub path: PathBuf,
    /// Its title.
    pub title: String,
    /// Its artist.
    pub artist: String,
    /// Why it fits, such as "One step on the wheel · +1.6%".
    pub reason: String,
}

/// The Next panel's suggestions.
#[derive(Default, Clone)]
pub(super) struct NextSuggestions {
    /// Ranking is running for the current inputs.
    pub pending: bool,
    /// The title of the track they follow, when there is one.
    pub after: Option<String>,
    /// Where they take the energy, once the track they follow is rated.
    pub direction: Option<EnergyDirection>,
    /// The suggestions, best first.
    pub tracks: Vec<NextTrack>,
}

impl NextSuggestions {
    /// Pending results can retain old rows internally; none may be inserted until current.
    fn suggestion(&self, rank: usize) -> Result<&NextTrack, &'static str> {
        if self.pending {
            return Err("Finding suggestions for this track. Try again in a moment.");
        }
        self.tracks.get(rank).ok_or(if self.after.is_none() {
            "Select a set track first: the suggestions follow it."
        } else {
            "There is no such suggestion: nothing else in the library mixes well after it."
        })
    }
}

/// Derived preparation data, rebuilt on input changes rather than animation frames.
#[derive(Default, Clone)]
pub(super) struct PrepareViewModel {
    pub rows: Vec<SetRow>,
    pub seams: Vec<Option<SeamSummary>>,
    pub warnings: Vec<SetWarning>,
    pub next: NextSuggestions,
    pub backups: Vec<String>,
    pub energy_range: EnergyRange,
}

impl DubplateApp {
    /// Executes serialized storage work on the set worker and accepts only newer snapshots.
    /// Failures are reported even when a newer completion has already updated the view.
    pub(super) fn queue_set_edit<ResultValue: Send + 'static>(
        &mut self,
        edit: impl FnOnce(&mut SetEditor) -> Result<ResultValue, StoreError> + Send + 'static,
        complete: impl FnOnce(&mut Self, ResultValue, &mut Context<'_, Self>) + 'static,
        cx: &mut Context<'_, Self>,
    ) {
        if self.quit_requested {
            return;
        }
        let Some(session) = &self.set_editor else {
            self.show_notice(
                NoticePlace::Window,
                "The set is still opening. Try again in a moment.",
                cx,
            );
            return;
        };
        let job = match session.submit(edit) {
            Ok(job) => job,
            Err(error) => {
                self.report_set_save_failure(error, cx);
                return;
            }
        };
        self.set_edits_pending += 1;
        cx.spawn(async move |this, cx| {
            let answer = cx
                .background_executor()
                .spawn(async move { job.wait() })
                .await;
            this.update(cx, |this, cx| {
                this.set_edits_pending = this.set_edits_pending.saturating_sub(1);
                let completion = match answer {
                    Ok(completion) => completion,
                    Err(error) => {
                        this.report_set_save_failure(error, cx);
                        this.finish_quit_if_saved(cx);
                        return;
                    }
                };
                let selected_path = this.selected_set_path();
                let Some(session) = this.set_editor.as_mut() else {
                    this.finish_quit_if_saved(cx);
                    return;
                };
                let invalidated = invalidated_check_paths(session.set(), &completion.set);
                let applied = session.apply(&completion);
                if applied {
                    this.set_selection = selected_path
                        .as_deref()
                        .and_then(|path| session.position(path))
                        .or_else(|| {
                            this.set_selection.and_then(|position| {
                                let count = session.set().main_entries().count();
                                (count > 0).then(|| position.min(count - 1))
                            })
                        });
                    if invalidated
                        .iter()
                        .any(|path| this.checks_pending.contains(path))
                    {
                        // An older result for one of these files must not reinstall its old grid.
                        this.forget_track_checks();
                    } else {
                        for path in &invalidated {
                            this.track_checks.remove(path);
                        }
                    }
                }
                match completion.outcome {
                    Ok(value) => {
                        this.quit_refused = false;
                        if applied {
                            complete(this, value, cx);
                        }
                    }
                    Err(error) => this.report_set_save_failure(error, cx),
                }
                if applied {
                    this.set_changed(cx);
                }
                this.finish_quit_if_saved(cx);
            })
            .ok();
        })
        .detach();
    }

    fn selected_set_path(&self) -> Option<PathBuf> {
        self.set_editor
            .as_ref()?
            .set()
            .main_entries()
            .nth(self.set_selection?)
            .map(|entry| entry.track_path.clone())
    }

    /// Carries out a change to the set without waiting for storage on the UI thread.
    pub(super) fn edit_set(&mut self, edit: SetEdit, cx: &mut Context<'_, Self>) {
        let edit = match edit {
            SetEdit::Add | SetEdit::InsertAfterSelection
                if self.mode == Mode::Prepare && self.prepare_focus == PrepareFocus::Next =>
            {
                SetEdit::InsertSuggestion(self.next_cursor)
            }
            edit => edit,
        };
        if let SetEdit::InsertSuggestion(rank) = edit {
            self.insert_suggestion_ranked(rank, cx);
            return;
        }
        match edit {
            SetEdit::AcceptProposedCues => {
                self.accept_proposed_cues(cx);
                return;
            }
            SetEdit::MixInAtPlayhead | SetEdit::MixOutAtPlayhead => {
                self.set_cue_at_playhead(edit == SetEdit::MixInAtPlayhead, cx);
                return;
            }
            _ => {}
        }
        let selected = self.library_selection(cx);
        let selection = self.selected_set_path();
        self.set_selection_generation += 1;
        let generation = self.set_selection_generation;
        self.queue_set_edit(
            move |editor| apply_edit(editor, edit, selected, selection.as_deref()),
            move |this, selection, _| {
                if this.set_selection_generation == generation {
                    this.set_selection = selection;
                }
            },
            cx,
        );
    }

    /// Moves the originally dragged track relative to the originally targeted track.
    pub(super) fn drop_set_track(&mut self, from: usize, to: usize, cx: &mut Context<'_, Self>) {
        let Some(session) = &self.set_editor else {
            return;
        };
        let tracks = session.tracks();
        let (Some(from_path), Some(to_path)) = (tracks.get(from), tracks.get(to)) else {
            return;
        };
        let from_path = from_path.track_path.clone();
        let to_path = to_path.track_path.clone();
        self.set_selection_generation += 1;
        let generation = self.set_selection_generation;
        self.queue_set_edit(
            move |editor| {
                let (Some(from), Some(to)) =
                    (editor.position(&from_path), editor.position(&to_path))
                else {
                    return Ok(None);
                };
                editor.move_track(from, to).map(Some)
            },
            move |this, position, _| {
                if this.set_selection_generation == generation {
                    this.set_selection = position;
                }
            },
            cx,
        );
    }

    /// Puts a library track after the original target, resolving its current place on the worker.
    pub(super) fn insert_into_set(
        &mut self,
        track: TrackIndex,
        after: Option<usize>,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(reference) = self.track_ref(track) else {
            return;
        };
        let Some(session) = &self.set_editor else {
            return;
        };
        let after_path = after
            .and_then(|position| session.set().main_entries().nth(position))
            .map(|entry| entry.track_path.clone());
        self.set_selection_generation += 1;
        let generation = self.set_selection_generation;
        self.queue_set_edit(
            move |editor| {
                let after_position = after_path.as_deref().and_then(|path| editor.position(path));
                if after.is_some() && after_position.is_none() {
                    return Err(StoreError::PositionNotFound(after.unwrap_or(0)));
                }
                editor.insert_after(after_position, reference)
            },
            move |this, position, _| {
                if this.set_selection_generation == generation {
                    this.set_selection = Some(position);
                }
            },
            cx,
        );
    }

    /// Inserts exactly the suggestion currently shown at this rank, without ranking again.
    fn insert_suggestion_ranked(&mut self, rank: usize, cx: &mut Context<'_, Self>) {
        if self.set_edits_pending > 0 || self.prepare_view.next.pending {
            self.show_notice(
                NoticePlace::Window,
                "Updating the set and its suggestions. Try again in a moment.",
                cx,
            );
            return;
        }
        let path = match self.prepare_view.next.suggestion(rank) {
            Ok(suggestion) => suggestion.path.clone(),
            Err(message) => {
                self.show_notice(NoticePlace::Window, message, cx);
                return;
            }
        };
        self.insert_suggestion(&path, cx);
    }

    /// Moves through the cached suggestions. Pending rankings have no selectable rows.
    pub(super) fn move_next_cursor(&mut self, direction: Direction) {
        let count = if self.prepare_view.next.pending {
            0
        } else {
            self.prepare_view.next.tracks.len()
        };
        if count == 0 {
            self.next_cursor = 0;
            return;
        }
        self.next_cursor = match direction {
            Direction::Up => self.next_cursor.saturating_sub(1),
            Direction::Down => (self.next_cursor + 1).min(count.saturating_sub(1)),
        };
    }

    /// Inserts a suggestion right after the selected set track, or the last one.
    pub(super) fn insert_suggestion(&mut self, path: &Path, cx: &mut Context<'_, Self>) {
        if self.prepare_view.next.pending || self.set_edits_pending > 0 {
            self.show_notice(
                NoticePlace::Window,
                "Updating the set and its suggestions. Try again in a moment.",
                cx,
            );
            return;
        }
        let Some(track) = self
            .collection()
            .and_then(|collection| collection.find_path(path))
        else {
            return;
        };
        let after = self
            .set_selection
            .or_else(|| self.set_track_count().checked_sub(1));
        self.next_cursor = 0;
        self.insert_into_set(track, after, cx);
    }

    /// How many tracks the set holds.
    pub(super) fn set_track_count(&self) -> usize {
        self.set_editor
            .as_ref()
            .map_or(0, |editor| editor.tracks().len())
    }

    /// Selects a set track and moves the keys to the set.
    pub(super) fn select_set_track(&mut self, position: usize, cx: &mut Context<'_, Self>) {
        self.set_selection_generation += 1;
        self.set_selection = Some(position);
        self.prepare_focus = PrepareFocus::Set;
        self.next_cursor = 0;
        self.refresh_prepare_view(cx);
        self.refresh_filter_reference(cx);
        self.ensure_track_detail(cx);
        cx.notify();
    }

    /// Moves the set selection one row up or down.
    pub(super) fn move_set_selection(&mut self, direction: Direction, cx: &mut Context<'_, Self>) {
        let count = self.set_track_count();
        if count == 0 {
            return;
        }
        let next = match (self.set_selection, direction) {
            (None, _) => 0,
            (Some(position), Direction::Up) => position.saturating_sub(1),
            (Some(position), Direction::Down) => (position + 1).min(count - 1),
        };
        self.set_selection_generation += 1;
        self.set_selection = Some(next);
        self.next_cursor = 0;
        self.reveal_set_selection();
        self.refresh_prepare_view(cx);
        self.refresh_filter_reference(cx);
    }

    /// Each track row is followed by its seam; reveal keyboard and insertion selections.
    fn reveal_set_selection(&self) {
        if let Some(position) = self.set_selection {
            self.set_scroll.scroll_to_item(position * 2);
        }
    }

    /// The track selected in the library, as a set names it.
    fn library_selection(&self, cx: &App) -> Option<SetTrackRef> {
        self.track_ref(self.browser.read(cx).selected()?)
    }

    /// A library track as a set names it: its file, and its rekordbox id when it has one.
    fn track_ref(&self, track: TrackIndex) -> Option<SetTrackRef> {
        let entry = self.collection()?.track(track);
        Some(SetTrackRef {
            path: entry.track.file_path.clone()?,
            rekordbox_id: (entry.origin == TrackOrigin::Rekordbox).then(|| entry.track.id.clone()),
        })
    }

    /// Puts the selected set track's mix-in or mix-out on the bar nearest the track check's
    /// playhead, on the DJ's corrected grid.
    fn set_cue_at_playhead(&mut self, mix_in: bool, cx: &mut Context<'_, Self>) {
        let Some((entry, detail)) = self.selected_detail() else {
            self.show_notice(
                NoticePlace::Window,
                "The track check is still reading this track.",
                cx,
            );
            return;
        };
        let seconds = self.check_playhead(&entry.track_path);
        self.set_selection_generation += 1;
        self.queue_set_edit(
            move |editor| {
                let Some(position) = editor.position(&entry.track_path) else {
                    return Ok(());
                };
                let current = editor.tracks()[position];
                let milliseconds = detail
                    .grid
                    .as_ref()
                    .map(|grid| GridCorrection::of(current).apply(grid))
                    .and_then(|grid| nearest_bar_ms(&grid, seconds))
                    .unwrap_or_else(|| (seconds * 1000.0).round().max(0.0) as u32);
                if mix_in {
                    editor.set_mix_in(position, milliseconds)
                } else {
                    editor.set_mix_out(position, milliseconds)
                }
            },
            |_, (), _| {},
            cx,
        );
    }

    fn accept_proposed_cues(&mut self, cx: &mut Context<'_, Self>) {
        let Some(path) = self.selected_set_path() else {
            return;
        };
        match self.track_checks.get(&path).copied() {
            Some(check) if check.proposal.mix_in.is_some() || check.proposal.mix_out.is_some() => {
                self.set_selection_generation += 1;
                self.queue_set_edit(
                    move |editor| {
                        let Some(position) = editor.position(&path) else {
                            return Ok(());
                        };
                        let (mix_in, mix_out) =
                            accept_proposal(editor.tracks()[position], check.proposal);
                        editor.set_proposed_cues(position, mix_in, mix_out)
                    },
                    |_, (), _| {},
                    cx,
                );
            }
            Some(_) => self.show_notice(
                NoticePlace::Window,
                "No cues to propose: this track has no beat grid yet.",
                cx,
            ),
            None => {}
        }
    }

    /// Drops what was found about the set tracks, to look again in a new collection. Checks
    /// still running for the old one are ignored when they come back.
    pub(super) fn forget_track_checks(&mut self) {
        self.track_checks.clear();
        self.checks_pending.clear();
        self.check_generation += 1;
    }

    /// After every change: mark the set's tracks in the library and check the new ones.
    pub(super) fn set_changed(&mut self, cx: &mut Context<'_, Self>) {
        self.stop_audition(cx);
        let in_set: HashSet<TrackIndex> = match (self.collection(), &self.set_editor) {
            (Some(collection), Some(editor)) => editor
                .tracks()
                .iter()
                .filter_map(|entry| collection.find_path(&entry.track_path))
                .collect(),
            _ => HashSet::new(),
        };
        let in_set = Arc::new(in_set);
        self.browser.update(cx, |browser, cx| {
            browser.set_marked(Arc::clone(&in_set), cx)
        });
        self.check_set_tracks(cx);
        self.reveal_set_selection();
        self.refresh_filter_reference(cx);
        self.ensure_track_detail(cx);
        self.request_set_analysis(cx);
        self.refresh_prepare_view(cx);
        cx.notify();
    }

    /// Reads the file and analysis of every set track and backup not checked yet, off the
    /// interface thread.
    pub(super) fn check_set_tracks(&mut self, cx: &mut Context<'_, Self>) {
        let (Some(collection), Some(editor)) = (self.collection().cloned(), &self.set_editor)
        else {
            return;
        };
        let unchecked: Vec<(PathBuf, GridCorrection, Option<Arc<AudioAnalysis>>)> = editor
            .set()
            .entries
            .iter()
            .filter(|entry| {
                !self.track_checks.contains_key(&entry.track_path)
                    && !self.checks_pending.contains(&entry.track_path)
            })
            .map(|entry| {
                (
                    entry.track_path.clone(),
                    GridCorrection::of(entry),
                    self.analyses.get(&entry.track_path).cloned(),
                )
            })
            .collect();
        if unchecked.is_empty() {
            return;
        }
        self.checks_pending
            .extend(unchecked.iter().map(|(path, _, _)| path.clone()));
        let generation = self.check_generation;
        cx.spawn(async move |this, cx| {
            let checks = cx
                .background_executor()
                .spawn(async move {
                    unchecked
                        .into_iter()
                        .map(|(path, correction, analysis)| {
                            let check = match collection.find_path(&path) {
                                Some(index) => check_track(
                                    collection.track(index),
                                    collection.share_directory(),
                                    correction,
                                    analysis.as_deref(),
                                ),
                                None => TrackCheck::not_in_library(&path),
                            };
                            (path, check)
                        })
                        .collect::<Vec<(PathBuf, TrackCheck)>>()
                })
                .await;
            this.update(cx, |this, cx| {
                if this.check_generation != generation {
                    return;
                }
                for (path, check) in checks {
                    this.checks_pending.remove(&path);
                    this.track_checks.insert(path, check);
                }
                this.refresh_prepare_view(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The set's tracks as the panel shows them.
    pub(super) fn set_rows(&self) -> Vec<SetRow> {
        let Some(editor) = &self.set_editor else {
            return Vec::new();
        };
        editor
            .tracks()
            .iter()
            .enumerate()
            .map(|(position, entry)| {
                let track = self.catalogue.get(&entry.track_path).cloned();
                let check = self.track_checks.get(&entry.track_path);
                let ready = match &track {
                    Some(track) => readiness(track, check, entry),
                    None => ReadyReport {
                        problems: vec![ReadyProblem::FileMissing],
                    },
                };
                let duration = self
                    .collection()
                    .and_then(|collection| {
                        collection
                            .find_path(&entry.track_path)
                            .map(|index| collection.track(index))
                    })
                    .and_then(|track| track.track.length_seconds)
                    .map(|seconds| Duration::from_secs(u64::from(seconds)));
                SetRow {
                    position,
                    track,
                    duration,
                    ready,
                    has_proposal: check.is_some_and(|check| can_propose(entry, check)),
                    free_into_next: editor.is_free_after(position),
                }
            })
            .collect()
    }

    /// The key move and tempo step of the seam after each set track.
    pub(super) fn seams(rows: &[SetRow]) -> Vec<Option<SeamSummary>> {
        rows.windows(2)
            .map(|pair| match (&pair[0].track, &pair[1].track) {
                (Some(outgoing), Some(incoming)) => Some(SeamSummary::between(outgoing, incoming)),
                _ => None,
            })
            .collect()
    }

    /// The set's warnings: key clashes, tempo gaps, energy drops and duplicates, at the
    /// positions the panel shows.
    pub(super) fn warnings(rows: &[SetRow]) -> Vec<SetWarning> {
        let known: Vec<(usize, &PrepTrack, bool)> = rows
            .iter()
            .filter_map(|row| Some((row.position, row.track.as_ref()?, row.free_into_next)))
            .collect();
        let entries: Vec<SetTrack<'_>> = known
            .iter()
            .map(|(_, track, free)| SetTrack {
                track,
                vocals_at_start: false,
                vocals_at_end: false,
                free_into_next: *free,
            })
            .collect();
        set_warnings(&entries)
            .into_iter()
            .map(|warning| SetWarning {
                position: match warning.position {
                    WarningPosition::Seam { after } => WarningPosition::Seam {
                        after: known[after].0,
                    },
                    WarningPosition::Track { index } => WarningPosition::Track {
                        index: known[index].0,
                    },
                },
                kind: warning.kind,
            })
            .collect()
    }

    /// The backups' titles, in order.
    pub(super) fn backup_titles(&self) -> Vec<String> {
        let (Some(editor), Some(collection)) = (&self.set_editor, self.collection()) else {
            return Vec::new();
        };
        editor
            .backups()
            .iter()
            .map(|entry| match collection.find_path(&entry.track_path) {
                Some(index) => collection.track(index).row.title.to_string(),
                None => file_title(&entry.track_path),
            })
            .collect()
    }

    /// Rebuilds set-sized data once; catalogue scans and ranking run on the executor. An
    /// older calculation can never overwrite a newer selection, edit or analysis update.
    pub(super) fn refresh_prepare_view(&mut self, cx: &mut Context<'_, Self>) {
        self.prepare_generation += 1;
        let generation = self.prepare_generation;
        let rows = self.set_rows();
        let mut view = PrepareViewModel {
            seams: Self::seams(&rows),
            warnings: Self::warnings(&rows),
            backups: self.backup_titles(),
            rows,
            ..Default::default()
        };
        let reference = self
            .set_selection
            .or_else(|| view.rows.len().checked_sub(1))
            .and_then(|position| Some((position, view.rows.get(position)?.track.clone()?)));
        if let Some((_, reference)) = &reference {
            view.next.after = Some(reference.title.clone());
            view.next.pending = true;
        }
        let target = self
            .set_editor
            .as_ref()
            .map(|editor| editor.target())
            .unwrap_or_default();
        let lengths: Vec<_> = view.rows.iter().map(|row| (row.duration, None)).collect();
        let paths: HashSet<PathBuf> = self
            .set_editor
            .as_ref()
            .map(|editor| {
                editor
                    .set()
                    .main_entries()
                    .map(|entry| entry.track_path.clone())
                    .collect()
            })
            .unwrap_or_default();
        let catalogue = Arc::clone(&self.catalogue);
        self.prepare_view = Arc::new(view.clone());
        cx.spawn(async move |this, cx| {
            let view = cx
                .background_executor()
                .spawn(async move {
                    view.energy_range = EnergyRange::of_library(
                        catalogue.tracks.iter().filter_map(|track| track.energy),
                    );
                    if let Some((position, reference)) = reference {
                        let end = set_curve(&lengths, target)
                            .get(position)
                            .map_or(1.0, |step| step.end);
                        view.next = ranked_next(
                            &reference,
                            &catalogue.tracks,
                            &paths,
                            end,
                            view.energy_range,
                        );
                    }
                    Arc::new(view)
                })
                .await;
            this.update(cx, |this, cx| {
                if this.prepare_generation == generation {
                    this.next_cursor = this
                        .next_cursor
                        .min(view.next.tracks.len().saturating_sub(1));
                    this.prepare_view = view;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Puts the set's first track on deck A and its second on deck B, each cued at its mix-in.
    pub(super) fn load_set(&mut self, cx: &mut Context<'_, Self>) {
        let (Some(collection), Some(editor)) = (self.collection().cloned(), &self.set_editor)
        else {
            return;
        };
        let entries: Vec<SetEntry> = editor.tracks().into_iter().take(2).cloned().collect();
        if entries.is_empty() {
            self.show_notice(NoticePlace::Window, "The set is empty.", cx);
            return;
        }
        let mut unloaded: Vec<(DeckId, String)> = Vec::new();
        let mut missing_tracks: Vec<TrackIndex> = Vec::new();
        for (deck, entry) in DeckId::ALL.into_iter().zip(&entries) {
            let index = collection.find_path(&entry.track_path);
            let present = index.is_some_and(|index| check_present(collection.track(index)).is_ok());
            let Some(index) = index.filter(|_| present) else {
                let title = index.map_or_else(
                    || file_title(&entry.track_path),
                    |index| collection.track(index).row.title.to_string(),
                );
                unloaded.push((deck, title));
                missing_tracks.extend(index);
                continue;
            };
            let mix_in = entry.mix_in_ms.or_else(|| {
                self.track_checks
                    .get(&entry.track_path)
                    .and_then(|check| check.proposal.mix_in)
                    .map(|cue| cue.time_ms)
            });
            let cue = mix_in.map(|milliseconds| f64::from(milliseconds) / 1000.0);
            self.load_cued(deck, LoadSource::Collection(index), cue, cx);
        }
        if let Some(text) = unloaded_notice(&unloaded) {
            self.report_missing_file(text, &missing_tracks, cx);
        }
    }
}

/// A newly added file may still have cached checks from a previous entry; it needs a fresh
/// check just as a changed correction does. Reordering alone leaves checks valid.
fn invalidated_check_paths(
    before: &dubplate_prep::Set,
    after: &dubplate_prep::Set,
) -> HashSet<PathBuf> {
    let previous: HashMap<_, _> = before
        .entries
        .iter()
        .map(|entry| {
            (
                (entry.track_path.as_path(), entry.is_backup),
                GridCorrection::of(entry),
            )
        })
        .collect();
    after
        .entries
        .iter()
        .filter(|entry| {
            previous
                .get(&(entry.track_path.as_path(), entry.is_backup))
                .is_none_or(|correction| *correction != GridCorrection::of(entry))
        })
        .map(|entry| entry.track_path.clone())
        .collect()
}

/// The ranking input is a snapshot, so it needs no UI or storage access.
fn ranked_next(
    reference: &PrepTrack,
    catalogue: &[PrepTrack],
    in_set: &HashSet<PathBuf>,
    reference_end: f32,
    range: EnergyRange,
) -> NextSuggestions {
    let direction = direction_toward_target(reference.energy, reference_end, range);
    let tracks = suggest_next(
        reference,
        catalogue,
        direction,
        |candidate| in_set.contains(&candidate.track_path),
        SUGGESTIONS,
    )
    .into_iter()
    .map(|suggestion| NextTrack {
        path: suggestion.track.track_path.clone(),
        title: suggestion.track.title.clone(),
        artist: suggestion.track.artist.clone(),
        reason: suggestion.reason(),
    })
    .collect();
    NextSuggestions {
        pending: false,
        after: Some(reference.title.clone()),
        direction: reference.energy.map(|_| direction),
        tracks,
    }
}

/// Carries out an edit on the set, moving the selection with it. Accepting proposed cues needs
/// the track checks and is finished by the caller.
fn apply_edit(
    editor: &mut SetEditor,
    edit: SetEdit,
    selected: Option<SetTrackRef>,
    selected_path: Option<&Path>,
) -> Result<Option<usize>, StoreError> {
    let mut selection = selected_path.and_then(|path| editor.position(path));
    match edit {
        SetEdit::Add => {
            if let Some(track) = selected {
                selection = Some(editor.add(track)?);
            }
        }
        SetEdit::InsertAfterSelection => {
            if let Some(track) = selected {
                selection = Some(editor.insert_after(selection, track)?);
            }
        }
        SetEdit::Move(direction) => {
            if let Some(position) = selection {
                let target = match direction {
                    Direction::Up => position.saturating_sub(1),
                    Direction::Down => position + 1,
                };
                selection = Some(editor.move_track(position, target)?);
            }
        }
        SetEdit::Remove => {
            if let Some(position) = selection {
                editor.remove(position)?;
                let count = editor.tracks().len();
                selection = (count > 0).then(|| position.min(count - 1));
            }
        }
        SetEdit::TogglePlanned => {
            if let Some(position) = selection {
                editor.toggle_free(position)?;
            }
        }
        SetEdit::ToggleBackup => {
            if let Some(track) = selected {
                editor.toggle_backup(track)?;
            }
        }
        SetEdit::Undo => {
            if editor.undo()? {
                let count = editor.tracks().len();
                selection = selection
                    .filter(|_| count > 0)
                    .map(|position| position.min(count - 1));
            }
        }
        SetEdit::ConfirmGrid => {
            if let Some(position) = selection {
                editor.toggle_grid_confirmed(position)?;
            }
        }
        SetEdit::NudgeGrid(milliseconds) => {
            if let Some(position) = selection {
                editor.nudge_grid(position, milliseconds)?;
            }
        }
        SetEdit::ShiftDownbeat(beats) => {
            if let Some(position) = selection {
                editor.shift_downbeat(position, beats)?;
            }
        }
        SetEdit::AcceptProposedCues
        | SetEdit::MixInAtPlayhead
        | SetEdit::MixOutAtPlayhead
        | SetEdit::InsertSuggestion(_) => {}
    }
    Ok(selection)
}

/// What the DJ is told when set tracks were kept off their decks because their files are
/// missing, such as "Deck A was not loaded: Uthando's file is missing.", or `None` when every
/// deck was loaded.
fn unloaded_notice(unloaded: &[(DeckId, String)]) -> Option<String> {
    match unloaded {
        [] => None,
        [(deck, title)] => Some(format!(
            "Deck {} was not loaded: {title}'s file is missing.",
            deck_letter(*deck)
        )),
        [(_, first), (_, second), ..] => Some(format!(
            "Neither deck was loaded: the files of {first} and {second} are missing."
        )),
    }
}

/// Whether the proposal could fill a cue the entry lacks.
fn can_propose(entry: &SetEntry, check: &TrackCheck) -> bool {
    (entry.mix_in_ms.is_none() && check.proposal.mix_in.is_some())
        || (entry.mix_out_ms.is_none() && check.proposal.mix_out.is_some())
}

/// A file's name without its extension, for a set track the library no longer holds.
fn file_title(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use dubplate_prep::SetSession;

    use super::*;

    fn track(name: &str) -> SetTrackRef {
        SetTrackRef {
            path: PathBuf::from(format!("/Music/{name}.mp3")),
            rekordbox_id: None,
        }
    }

    #[test]
    fn queued_moves_resolve_the_selected_file_after_preceding_removals() {
        let directory = tempfile::tempdir().unwrap();
        let mut editor = SetEditor::open(&directory.path().join("sets.sqlite")).unwrap();
        for name in ["a", "b", "c"] {
            editor.add(track(name)).unwrap();
        }
        let mut session = SetSession::new(editor).unwrap();
        let (started, started_rx) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        let removal = session
            .submit(move |editor| {
                started.send(()).unwrap();
                release_rx.recv().unwrap();
                apply_edit(editor, SetEdit::Remove, None, Some(&track("a").path))
            })
            .unwrap();
        started_rx.recv().unwrap();
        let movement = session
            .submit(move |editor| {
                apply_edit(
                    editor,
                    SetEdit::Move(Direction::Down),
                    None,
                    Some(&track("b").path),
                )
            })
            .unwrap();
        assert_eq!(
            session.position(&track("b").path),
            Some(1),
            "the view is still the prior committed snapshot"
        );
        release.send(()).unwrap();
        let moved = movement.wait().unwrap();
        assert_eq!(moved.outcome.as_ref().unwrap(), &Some(1));
        assert!(session.apply(&moved));
        assert!(!session.apply(&removal.wait().unwrap()));
        let paths: Vec<_> = session
            .tracks()
            .iter()
            .map(|entry| entry.track_path.clone())
            .collect();
        assert_eq!(paths, [track("c").path, track("b").path]);
    }

    #[test]
    fn a_queued_edit_never_targets_the_neighbor_of_a_removed_selection() {
        let directory = tempfile::tempdir().unwrap();
        let mut editor = SetEditor::open(&directory.path().join("sets.sqlite")).unwrap();
        editor.add(track("a")).unwrap();
        editor.add(track("b")).unwrap();
        apply_edit(&mut editor, SetEdit::Remove, None, Some(&track("a").path)).unwrap();
        apply_edit(
            &mut editor,
            SetEdit::ConfirmGrid,
            None,
            Some(&track("a").path),
        )
        .unwrap();
        assert!(!editor.tracks()[0].grid_confirmed);
    }

    #[test]
    fn grid_check_invalidation_follows_identity_and_catches_undo() {
        let directory = tempfile::tempdir().unwrap();
        let mut editor = SetEditor::open(&directory.path().join("sets.sqlite")).unwrap();
        editor.add(track("a")).unwrap();
        editor.add(track("b")).unwrap();
        let before = editor.set().clone();
        editor.move_track(0, 1).unwrap();
        assert!(invalidated_check_paths(&before, editor.set()).is_empty());
        editor.nudge_grid(0, 5).unwrap();
        let nudged = editor.set().clone();
        assert!(!invalidated_check_paths(&before, &nudged).is_empty());
        editor.undo().unwrap();
        assert!(!invalidated_check_paths(&nudged, editor.set()).is_empty());
    }

    #[test]
    fn readding_a_file_invalidates_checks_from_its_previous_entry() {
        let directory = tempfile::tempdir().unwrap();
        let mut editor = SetEditor::open(&directory.path().join("sets.sqlite")).unwrap();
        editor.add(track("a")).unwrap();
        editor.nudge_grid(0, 5).unwrap();
        editor.remove(0).unwrap();
        let without = editor.set().clone();
        editor.add(track("a")).unwrap();
        assert_eq!(editor.tracks()[0].grid_offset_ms, 0);
        assert!(invalidated_check_paths(&without, editor.set()).contains(&track("a").path));
    }

    #[test]
    fn a_pending_ranking_cannot_insert_an_old_cached_suggestion() {
        let suggestion = NextTrack {
            path: track("a").path,
            title: "a".to_owned(),
            artist: String::new(),
            reason: String::new(),
        };
        let mut next = NextSuggestions {
            pending: true,
            after: Some("reference".to_owned()),
            tracks: vec![suggestion],
            ..Default::default()
        };
        assert!(
            next.suggestion(0).is_err(),
            "old rows are not current while ranking runs"
        );
        next.pending = false;
        assert_eq!(next.suggestion(0).unwrap().path, track("a").path);
        assert!(next.suggestion(1).is_err());
    }

    #[test]
    fn the_notice_names_each_track_kept_off_its_deck() {
        assert_eq!(unloaded_notice(&[]), None);
        assert_eq!(
            unloaded_notice(&[(DeckId::B, "Amalobolo".to_owned())]).as_deref(),
            Some("Deck B was not loaded: Amalobolo's file is missing.")
        );
        assert_eq!(
            unloaded_notice(&[
                (DeckId::A, "Uthando".to_owned()),
                (DeckId::B, "Amalobolo".to_owned())
            ])
            .as_deref(),
            Some("Neither deck was loaded: the files of Uthando and Amalobolo are missing.")
        );
    }
}
