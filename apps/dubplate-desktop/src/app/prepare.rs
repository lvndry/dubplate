//! The window's side of set preparation: editing the set from the keys and the mouse, checking
//! each set track's file and analysis in the background, the Next suggestions, and loading the
//! set onto the decks.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use dubplate_prep::{
    EnergyDirection, PrepTrack, ReadyProblem, ReadyReport, SetEntry, SetTrack, SetWarning,
    StoreError, WarningPosition, set_warnings, suggest_next,
};
use gpui::{App, Context};

use super::{DubplateApp, NoticePlace};
use crate::collection::{TrackIndex, TrackOrigin};
use crate::commands::{Direction, SetEdit};
use crate::energy::{direction_toward_target, set_curve};
use crate::loading::LoadSource;
use crate::prep::{
    GridCorrection, SeamSummary, TrackCheck, accept_proposal, check_track, nearest_bar_ms,
    readiness,
};
use crate::set::{SetEditor, SetTrackRef};

/// How many tracks the Next panel suggests.
const SUGGESTIONS: usize = 5;

/// Which side of the Prepare screen the arrow keys move through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum PrepareFocus {
    /// The library's tracks.
    #[default]
    Library,
    /// The set's tracks.
    Set,
}

/// A set track as the panel shows it.
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
#[derive(Default)]
pub(super) struct NextSuggestions {
    /// The title of the track they follow, when there is one.
    pub after: Option<String>,
    /// Where they take the energy, once the track they follow is rated.
    pub direction: Option<EnergyDirection>,
    /// The suggestions, best first.
    pub tracks: Vec<NextTrack>,
}

impl DubplateApp {
    /// Carries out a change to the set.
    pub(super) fn edit_set(&mut self, edit: SetEdit, cx: &mut Context<'_, Self>) {
        let selected = self.library_selection(cx);
        let Some(editor) = self.set_editor.as_mut() else {
            return;
        };
        let result = apply_edit(editor, edit, selected, &mut self.set_selection);
        let result = match (edit, result) {
            (SetEdit::AcceptProposedCues, Ok(())) => self.accept_proposed_cues(cx),
            (SetEdit::MixInAtPlayhead | SetEdit::MixOutAtPlayhead, Ok(())) => {
                self.set_cue_at_playhead(edit == SetEdit::MixInAtPlayhead, cx)
            }
            (SetEdit::NudgeGrid(_) | SetEdit::ShiftDownbeat(_), Ok(())) => {
                self.forget_track_checks();
                Ok(())
            }
            (_, result) => result,
        };
        if let Err(error) = result {
            self.show_notice(
                NoticePlace::Window,
                format!("The set was not saved: {error}"),
                cx,
            );
        }
        self.set_changed(cx);
    }

    /// Moves a set track by dragging it onto another row.
    pub(super) fn drop_set_track(&mut self, from: usize, to: usize, cx: &mut Context<'_, Self>) {
        let Some(editor) = self.set_editor.as_mut() else {
            return;
        };
        match editor.move_track(from, to) {
            Ok(position) => self.set_selection = Some(position),
            Err(error) => self.show_notice(
                NoticePlace::Window,
                format!("The set was not saved: {error}"),
                cx,
            ),
        }
        self.set_changed(cx);
    }

    /// Puts a library track right after the set track at `after`, or first.
    pub(super) fn insert_into_set(
        &mut self,
        track: TrackIndex,
        after: Option<usize>,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(reference) = self.track_ref(track) else {
            return;
        };
        let Some(editor) = self.set_editor.as_mut() else {
            return;
        };
        match editor.insert_after(after, reference) {
            Ok(position) => self.set_selection = Some(position),
            Err(error) => self.show_notice(
                NoticePlace::Window,
                format!("The set was not saved: {error}"),
                cx,
            ),
        }
        self.set_changed(cx);
    }

    /// Inserts a suggestion right after the selected set track, or the last one.
    pub(super) fn insert_suggestion(&mut self, path: &Path, cx: &mut Context<'_, Self>) {
        let Some(track) = self
            .collection()
            .and_then(|collection| collection.find_path(path))
        else {
            return;
        };
        let after = self
            .set_selection
            .or_else(|| self.set_track_count().checked_sub(1));
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
        self.set_selection = Some(position);
        self.prepare_focus = PrepareFocus::Set;
        self.ensure_track_detail(cx);
        cx.notify();
    }

    /// Moves the set selection one row up or down.
    pub(super) fn move_set_selection(&mut self, direction: Direction) {
        let count = self.set_track_count();
        if count == 0 {
            return;
        }
        let next = match (self.set_selection, direction) {
            (None, _) => 0,
            (Some(position), Direction::Up) => position.saturating_sub(1),
            (Some(position), Direction::Down) => (position + 1).min(count - 1),
        };
        self.set_selection = Some(next);
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
    fn set_cue_at_playhead(
        &mut self,
        mix_in: bool,
        cx: &mut Context<'_, Self>,
    ) -> Result<(), StoreError> {
        let Some(position) = self.set_selection else {
            return Ok(());
        };
        let Some((entry, detail)) = self.selected_detail() else {
            self.show_notice(
                NoticePlace::Window,
                "The track check is still reading this track.",
                cx,
            );
            return Ok(());
        };
        let seconds = self.check_playhead(&entry.track_path);
        let bar = detail
            .grid
            .as_ref()
            .map(|grid| GridCorrection::of(&entry).apply(grid))
            .and_then(|grid| nearest_bar_ms(&grid, seconds));
        let milliseconds = bar.unwrap_or_else(|| (seconds * 1000.0).round().max(0.0) as u32);
        match self.set_editor.as_mut() {
            Some(editor) if mix_in => editor.set_mix_in(position, milliseconds),
            Some(editor) => editor.set_mix_out(position, milliseconds),
            None => Ok(()),
        }
    }

    fn accept_proposed_cues(&mut self, cx: &mut Context<'_, Self>) -> Result<(), StoreError> {
        let Some(position) = self.set_selection else {
            return Ok(());
        };
        let Some(entry) = self
            .set_editor
            .as_ref()
            .and_then(|editor| editor.tracks().get(position).map(|entry| (*entry).clone()))
        else {
            return Ok(());
        };
        match self.track_checks.get(&entry.track_path).copied() {
            Some(check) if check.proposal.mix_in.is_some() || check.proposal.mix_out.is_some() => {
                let (mix_in, mix_out) = accept_proposal(&entry, check.proposal);
                match self.set_editor.as_mut() {
                    Some(editor) => editor.set_proposed_cues(position, mix_in, mix_out),
                    None => Ok(()),
                }
            }
            Some(_) => {
                self.show_notice(
                    NoticePlace::Window,
                    "No cues to propose: rekordbox has no beat grid for this track.",
                    cx,
                );
                Ok(())
            }
            None => Ok(()),
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
        self.ensure_track_detail(cx);
        self.request_set_energy(cx);
        cx.notify();
    }

    /// Reads the file and analysis of every set track and backup not checked yet, off the
    /// interface thread.
    fn check_set_tracks(&mut self, cx: &mut Context<'_, Self>) {
        let (Some(collection), Some(editor)) = (self.collection().cloned(), &self.set_editor)
        else {
            return;
        };
        let unchecked: Vec<(PathBuf, GridCorrection)> = editor
            .set()
            .entries
            .iter()
            .filter(|entry| {
                !self.track_checks.contains_key(&entry.track_path)
                    && !self.checks_pending.contains(&entry.track_path)
            })
            .map(|entry| (entry.track_path.clone(), GridCorrection::of(entry)))
            .collect();
        if unchecked.is_empty() {
            return;
        }
        self.checks_pending
            .extend(unchecked.iter().map(|(path, _)| path.clone()));
        let generation = self.check_generation;
        cx.spawn(async move |this, cx| {
            let checks = cx
                .background_executor()
                .spawn(async move {
                    unchecked
                        .into_iter()
                        .map(|(path, correction)| {
                            let check = match collection.find_path(&path) {
                                Some(index) => check_track(
                                    collection.track(index),
                                    collection.share_directory(),
                                    correction,
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

    /// Five tracks from the library that would follow the selected set track, or the last one,
    /// taking the energy toward the target.
    pub(super) fn next_tracks(&self, rows: &[SetRow]) -> NextSuggestions {
        let Some(editor) = &self.set_editor else {
            return NextSuggestions::default();
        };
        let Some((position, reference)) = self
            .set_selection
            .or_else(|| rows.len().checked_sub(1))
            .and_then(|position| Some((position, rows.get(position)?.track.as_ref()?)))
        else {
            return NextSuggestions::default();
        };
        let lengths: Vec<(Option<Duration>, Option<f32>)> =
            rows.iter().map(|row| (row.duration, None)).collect();
        let reference_end = set_curve(&lengths, editor.target())
            .get(position)
            .map_or(1.0, |step| step.end);
        let direction = direction_toward_target(reference.energy, reference_end);
        let tracks = suggest_next(
            reference,
            &self.catalogue.tracks,
            direction,
            |candidate| editor.position(&candidate.track_path).is_some(),
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
            after: Some(reference.title.clone()),
            direction: reference.energy.map(|_| direction),
            tracks,
        }
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
        for (deck, entry) in dubplate_engine::DeckId::ALL.into_iter().zip(&entries) {
            let Some(index) = collection.find_path(&entry.track_path) else {
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
    }
}

/// Carries out an edit on the set, moving the selection with it. Accepting proposed cues needs
/// the track checks and is finished by the caller.
fn apply_edit(
    editor: &mut SetEditor,
    edit: SetEdit,
    selected: Option<SetTrackRef>,
    selection: &mut Option<usize>,
) -> Result<(), StoreError> {
    match edit {
        SetEdit::Add => {
            if let Some(track) = selected {
                *selection = Some(editor.add(track)?);
            }
        }
        SetEdit::InsertAfterSelection => {
            if let Some(track) = selected {
                *selection = Some(editor.insert_after(*selection, track)?);
            }
        }
        SetEdit::Move(direction) => {
            if let Some(position) = *selection {
                let target = match direction {
                    Direction::Up => position.saturating_sub(1),
                    Direction::Down => position + 1,
                };
                *selection = Some(editor.move_track(position, target)?);
            }
        }
        SetEdit::Remove => {
            if let Some(position) = *selection {
                editor.remove(position)?;
                let count = editor.tracks().len();
                *selection = (count > 0).then(|| position.min(count - 1));
            }
        }
        SetEdit::TogglePlanned => {
            if let Some(position) = *selection {
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
                *selection = selection
                    .filter(|_| count > 0)
                    .map(|position| position.min(count - 1));
            }
        }
        SetEdit::ConfirmGrid => {
            if let Some(position) = *selection {
                editor.toggle_grid_confirmed(position)?;
            }
        }
        SetEdit::NudgeGrid(milliseconds) => {
            if let Some(position) = *selection {
                editor.nudge_grid(position, milliseconds)?;
            }
        }
        SetEdit::ShiftDownbeat(beats) => {
            if let Some(position) = *selection {
                editor.shift_downbeat(position, beats)?;
            }
        }
        SetEdit::AcceptProposedCues | SetEdit::MixInAtPlayhead | SetEdit::MixOutAtPlayhead => {}
    }
    Ok(())
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
