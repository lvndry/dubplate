//! The window's side of set preparation: editing the set from the keys and the mouse, checking
//! each set track's file and analysis in the background, the Next suggestions, and loading the
//! set onto the decks.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use dubplate_prep::{
    EnergyDirection, PrepTrack, ReadyReport, SetTrack, SetWarning, WarningPosition, set_warnings,
    suggest_next,
};
use gpui::{App, Context};

use super::{DubplateApp, NoticePlace};
use crate::collection::TrackIndex;
use crate::commands::{Direction, SetEdit};
use crate::loading::LoadSource;
use crate::prep::{SeamSummary, TrackCheck, accept_proposal, check_track, readiness};
use crate::set::SetEntry;

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
    /// The track in the collection, when the library holds it.
    pub track: Option<PrepTrack>,
    /// How long it plays.
    pub duration: Option<Duration>,
    /// Whether it is ready to mix, and what is missing.
    pub ready: ReadyReport,
    /// Whether a proposal could fill a missing mix cue.
    pub has_proposal: bool,
    /// The set's entry.
    pub entry: SetEntry,
}

/// A suggestion of the Next panel.
pub(super) struct NextTrack {
    /// The track's identity.
    pub identity: String,
    /// Its title.
    pub title: String,
    /// Its artist.
    pub artist: String,
    /// Why it fits, such as "One step on the wheel · +1.6%".
    pub reason: String,
}

impl DubplateApp {
    /// Carries out a change to the set.
    pub(super) fn edit_set(&mut self, edit: SetEdit, cx: &mut Context<'_, Self>) {
        match edit {
            SetEdit::Add => {
                if let Some(identity) = self.library_selection(cx) {
                    let position = self.set_editor.add(identity);
                    self.set_selection = Some(position);
                }
            }
            SetEdit::InsertAfterSelection => {
                if let Some(identity) = self.library_selection(cx) {
                    let position = self.set_editor.insert_after(self.set_selection, identity);
                    self.set_selection = Some(position);
                }
            }
            SetEdit::Move(direction) => {
                if let Some(position) = self.set_selection {
                    let target = match direction {
                        Direction::Up => position.saturating_sub(1),
                        Direction::Down => position + 1,
                    };
                    self.set_selection = Some(self.set_editor.move_track(position, target));
                }
            }
            SetEdit::Remove => {
                if let Some(position) = self.set_selection {
                    self.set_editor.remove(position);
                    let count = self.set_editor.set().entries.len();
                    self.set_selection = (count > 0).then(|| position.min(count - 1));
                }
            }
            SetEdit::TogglePlanned => {
                if let Some(position) = self.set_selection {
                    self.set_editor.toggle_free(position);
                }
            }
            SetEdit::AcceptProposedCues => self.accept_proposed_cues(cx),
            SetEdit::ToggleBackup => {
                if let Some(identity) = self.library_selection(cx) {
                    self.set_editor.toggle_backup(identity);
                }
            }
            SetEdit::Undo => {
                if self.set_editor.undo() {
                    let count = self.set_editor.set().entries.len();
                    self.set_selection = self
                        .set_selection
                        .filter(|_| count > 0)
                        .map(|position| position.min(count - 1));
                }
            }
        }
        self.set_changed(cx);
    }

    /// Moves a set track by dragging it onto another row.
    pub(super) fn drop_set_track(&mut self, from: usize, to: usize, cx: &mut Context<'_, Self>) {
        self.set_selection = Some(self.set_editor.move_track(from, to));
        self.set_changed(cx);
    }

    /// Puts a library track dragged onto the set right after `after`, or first.
    pub(super) fn drop_library_track(
        &mut self,
        identity: String,
        after: Option<usize>,
        cx: &mut Context<'_, Self>,
    ) {
        self.set_selection = Some(self.set_editor.insert_after(after, identity));
        self.set_changed(cx);
    }

    /// Inserts a suggestion right after the selected set track.
    pub(super) fn insert_suggestion(&mut self, identity: String, cx: &mut Context<'_, Self>) {
        let after = self
            .set_selection
            .or_else(|| self.set_editor.set().entries.len().checked_sub(1));
        self.drop_library_track(identity, after, cx);
    }

    /// Selects a set track and moves the keys to the set.
    pub(super) fn select_set_track(&mut self, position: usize, cx: &mut Context<'_, Self>) {
        self.set_selection = Some(position);
        self.prepare_focus = PrepareFocus::Set;
        cx.notify();
    }

    /// Moves the set selection one row up or down.
    pub(super) fn move_set_selection(&mut self, direction: Direction) {
        let count = self.set_editor.set().entries.len();
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

    /// The identity of the track selected in the library.
    fn library_selection(&self, cx: &App) -> Option<String> {
        let collection = self.collection()?;
        let selected = self.browser.read(cx).selected()?;
        Some(collection.track(selected).identity())
    }

    fn accept_proposed_cues(&mut self, cx: &mut Context<'_, Self>) {
        let Some(position) = self.set_selection else {
            return;
        };
        let Some(entry) = self.set_editor.set().entries.get(position).cloned() else {
            return;
        };
        match self.track_checks.get(&entry.track) {
            Some(check) if check.proposal.mix_in.is_some() || check.proposal.mix_out.is_some() => {
                let cues = accept_proposal(entry.cues, check.proposal);
                self.set_editor.set_cues(position, cues);
            }
            Some(_) => self.show_notice(
                NoticePlace::Window,
                "No cues to propose: rekordbox has no beat grid for this track.",
                cx,
            ),
            None => {}
        }
    }

    /// After every change: mark the set's tracks in the library and check the new ones.
    pub(super) fn set_changed(&mut self, cx: &mut Context<'_, Self>) {
        let in_set: HashSet<TrackIndex> = match self.collection() {
            Some(collection) => self
                .set_editor
                .set()
                .entries
                .iter()
                .filter_map(|entry| collection.find(&entry.track))
                .collect(),
            None => HashSet::new(),
        };
        let in_set = Arc::new(in_set);
        self.browser.update(cx, |browser, cx| {
            browser.set_marked(Arc::clone(&in_set), cx)
        });
        self.check_set_tracks(cx);
        cx.notify();
    }

    /// Reads the file and analysis of every set track not checked yet, off the interface
    /// thread.
    pub(super) fn check_set_tracks(&mut self, cx: &mut Context<'_, Self>) {
        let Some(collection) = self.collection().cloned() else {
            return;
        };
        let unchecked: Vec<String> = self
            .set_editor
            .set()
            .entries
            .iter()
            .map(|entry| entry.track.clone())
            .chain(self.set_editor.set().backups.iter().cloned())
            .filter(|identity| {
                !self.track_checks.contains_key(identity) && !self.checks_pending.contains(identity)
            })
            .collect();
        if unchecked.is_empty() {
            return;
        }
        self.checks_pending.extend(unchecked.iter().cloned());
        cx.spawn(async move |this, cx| {
            let checks = cx
                .background_executor()
                .spawn(async move {
                    unchecked
                        .into_iter()
                        .filter_map(|identity| {
                            let track = collection.track(collection.find(&identity)?);
                            Some((identity, check_track(track, collection.share_directory())))
                        })
                        .collect::<Vec<(String, TrackCheck)>>()
                })
                .await;
            this.update(cx, |this, cx| {
                for (identity, check) in checks {
                    this.checks_pending.remove(&identity);
                    this.track_checks.insert(identity, check);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The suggestion engine's view of a track in the library.
    fn prep_track(&self, identity: &str) -> Option<&PrepTrack> {
        let collection = self.collection()?;
        self.catalogue.get(collection.find(identity)?.position())
    }

    /// The set's tracks as the panel shows them.
    pub(super) fn set_rows(&self) -> Vec<SetRow> {
        self.set_editor
            .set()
            .entries
            .iter()
            .enumerate()
            .map(|(position, entry)| {
                let track = self.prep_track(&entry.track).cloned();
                let check = self.track_checks.get(&entry.track);
                let ready = match &track {
                    Some(track) => readiness(track, check, entry.cues),
                    None => ReadyReport {
                        problems: vec![dubplate_prep::ReadyProblem::FileMissing],
                    },
                };
                let duration = self
                    .collection()
                    .and_then(|collection| {
                        collection
                            .find(&entry.track)
                            .map(|index| collection.track(index))
                    })
                    .and_then(|track| track.track.length_seconds)
                    .map(|seconds| Duration::from_secs(u64::from(seconds)));
                let has_proposal = check.is_some_and(|check| {
                    (entry.cues.mix_in_ms.is_none() && check.proposal.mix_in.is_some())
                        || (entry.cues.mix_out_ms.is_none() && check.proposal.mix_out.is_some())
                });
                SetRow {
                    position,
                    track,
                    duration,
                    ready,
                    has_proposal,
                    entry: entry.clone(),
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

    /// The set's warnings: key clashes, tempo gaps, energy drops and duplicates.
    pub(super) fn warnings(rows: &[SetRow]) -> Vec<SetWarning> {
        let known: Vec<(usize, &PrepTrack, bool)> = rows
            .iter()
            .filter_map(|row| Some((row.position, row.track.as_ref()?, row.entry.free_into_next)))
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

    /// Five tracks from the library that would follow the selected set track, or the last one.
    pub(super) fn next_tracks(&self) -> (Option<String>, Vec<NextTrack>) {
        let set = self.set_editor.set();
        let reference_position = self
            .set_selection
            .or_else(|| set.entries.len().checked_sub(1));
        let Some(reference) = reference_position
            .and_then(|position| set.entries.get(position))
            .and_then(|entry| self.prep_track(&entry.track))
        else {
            return (None, Vec::new());
        };
        let suggestions = suggest_next(
            reference,
            &self.catalogue,
            EnergyDirection::default(),
            |candidate| set.contains(&candidate.rekordbox_id),
            SUGGESTIONS,
        );
        let next = suggestions
            .into_iter()
            .map(|suggestion| NextTrack {
                identity: suggestion.track.rekordbox_id.clone(),
                title: suggestion.track.title.clone(),
                artist: suggestion.track.artist.clone(),
                reason: suggestion.reason(),
            })
            .collect();
        (Some(reference.title.clone()), next)
    }

    /// Puts the set's first track on deck A and its second on deck B, each cued at its mix-in.
    pub(super) fn load_set(&mut self, cx: &mut Context<'_, Self>) {
        let Some(collection) = self.collection().cloned() else {
            return;
        };
        let entries = self.set_editor.set().entries.clone();
        if entries.is_empty() {
            self.show_notice(NoticePlace::Window, "The set is empty.", cx);
            return;
        }
        for (deck, entry) in dubplate_engine::DeckId::ALL.into_iter().zip(&entries) {
            let Some(index) = collection.find(&entry.track) else {
                continue;
            };
            let mix_in = entry.cues.mix_in_ms.or_else(|| {
                self.track_checks
                    .get(&entry.track)
                    .and_then(|check| check.proposal.mix_in)
                    .map(|cue| cue.time_ms)
            });
            let cue = mix_in.map(|milliseconds| f64::from(milliseconds) / 1000.0);
            self.load_cued(deck, LoadSource::Collection(index), cue, cx);
        }
    }
}
