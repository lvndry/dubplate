//! The set being prepared, kept in Dubplate's own set database: its tracks in order, the
//! backups beside it, and the plan of each seam. Tracks are named by their file, so a set holds
//! tracks from music folders and from rekordbox alike. Every edit saves at once and can be
//! undone for the session.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use dubplate_rekordbox::TrackCues;

use crate::{
    CueSource, EqIntent, GridEdit, ImportedHistory, Play, PlayId, PlaylistEdit, PlaylistEdited,
    PlaylistTree, RecordedPlay, Seam, SeamStatus, Set, SetEntry, SetId, SetList, SetStore,
    StoreError, TagCategory, TrackGrid, TrackIdentity, TrackMetadata, TrackTagEdit, TrackTagEdited,
};

/// The name of the set made on first launch.
const FIRST_SET_NAME: &str = "Set";
/// How long the first set aims to run, in minutes.
const FIRST_SET_MINUTES: u32 = 120;
/// How long a new seam overlaps, in bars, until the seam strip lets the DJ say.
const DEFAULT_OVERLAP_BARS: u32 = 16;
/// How many edits undo goes back.
const UNDO_DEPTH: usize = 200;

/// A track to put in a set: its file and, when it comes from rekordbox, its id there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetTrackRef {
    /// The audio file.
    pub path: PathBuf,
    /// rekordbox's id for it, if rekordbox holds it.
    pub rekordbox_id: Option<String>,
}

/// The set being edited.
pub struct SetEditor {
    store: SetStore,
    id: SetId,
    set: Set,
    undo: Vec<Set>,
}

impl SetEditor {
    /// Opens the set database at `path` on its first set, making one when there is none.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let mut store = SetStore::open(path)?;
        let id = match store.list_sets()?.first() {
            Some((id, _, _)) => *id,
            None => store.create_set(FIRST_SET_NAME, FIRST_SET_MINUTES)?,
        };
        let set = store.load_set(id)?;
        Ok(Self {
            store,
            id,
            set,
            undo: Vec::new(),
        })
    }

    /// The set as it stands.
    pub fn set(&self) -> &Set {
        &self.set
    }

    /// How long the set aims to run.
    pub fn target(&self) -> Duration {
        Duration::from_secs(u64::from(self.set.target_minutes) * 60)
    }

    /// The set's tracks, in playing order.
    pub fn tracks(&self) -> Vec<&SetEntry> {
        self.set.main_entries().collect()
    }

    /// The backups, in the order they were kept.
    pub fn backups(&self) -> Vec<&SetEntry> {
        self.set
            .entries
            .iter()
            .filter(|entry| entry.is_backup)
            .collect()
    }

    /// Where the track with this file is in the set.
    pub fn position(&self, path: &Path) -> Option<usize> {
        self.set
            .main_entries()
            .position(|entry| entry.track_path == path)
    }

    /// Whether the seam after `position` is left to the moment, so it never warns.
    pub fn is_free_after(&self, position: usize) -> bool {
        self.set
            .seam_after(position)
            .is_some_and(|seam| seam.status == SeamStatus::Free)
    }

    /// Adds a track at the end, unless it is in the set already. Returns where it is.
    pub fn add(&mut self, track: SetTrackRef) -> Result<usize, StoreError> {
        if let Some(position) = self.position(&track.path) {
            return Ok(position);
        }
        let position = self.tracks().len();
        self.edit(|store, id| store.insert_entry(id, SetList::Main, position, entry(track)))?;
        Ok(position)
    }

    /// Puts a track right after `after`, or first, moving it there if it is in the set
    /// already. Returns where it is.
    pub fn insert_after(
        &mut self,
        after: Option<usize>,
        track: SetTrackRef,
    ) -> Result<usize, StoreError> {
        let count = self.tracks().len();
        match self.position(&track.path) {
            Some(existing) => {
                // `after` clamped to the set before the decrement: an out-of-date selection
                // lands at the end, not past it.
                let after = after.map_or(0, |position| position.min(count.saturating_sub(1)));
                let mut target = after + 1;
                if existing < target {
                    target -= 1;
                }
                self.move_track(existing, target)
            }
            None => {
                let target = after.map_or(0, |position| position + 1).min(count);
                self.edit(|store, id| store.insert_entry(id, SetList::Main, target, entry(track)))?;
                Ok(target)
            }
        }
    }

    /// Moves the track at `from` to `to`. Returns where it is now.
    pub fn move_track(&mut self, from: usize, to: usize) -> Result<usize, StoreError> {
        let count = self.tracks().len();
        if from >= count {
            return Ok(from);
        }
        let to = to.min(count - 1);
        if from != to {
            self.edit(|store, id| store.move_entry(id, SetList::Main, from, to))?;
        }
        Ok(to)
    }

    /// Takes the track at `position` out of the set.
    pub fn remove(&mut self, position: usize) -> Result<(), StoreError> {
        if position < self.tracks().len() {
            self.edit(|store, id| store.remove_entry(id, SetList::Main, position))?;
        }
        Ok(())
    }

    /// Switches the seam after `position` between planned and free.
    pub fn toggle_free(&mut self, position: usize) -> Result<(), StoreError> {
        let tracks = self.tracks();
        let (Some(from), Some(to)) = (tracks.get(position), tracks.get(position + 1)) else {
            return Ok(());
        };
        let mut seam = self.set.seam_after(position).cloned().unwrap_or(Seam {
            from_track_path: from.track_path.clone(),
            to_track_path: to.track_path.clone(),
            overlap_bars: DEFAULT_OVERLAP_BARS,
            eq_intent: EqIntent::None,
            status: SeamStatus::Planned,
            note: String::new(),
        });
        seam.status = match seam.status {
            SeamStatus::Planned => SeamStatus::Free,
            SeamStatus::Free => SeamStatus::Planned,
        };
        self.edit(|store, id| store.set_seam(id, seam))
    }

    /// Keeps mix cues Dubplate proposed for the track at `position`.
    pub fn set_proposed_cues(
        &mut self,
        position: usize,
        mix_in_ms: Option<u32>,
        mix_out_ms: Option<u32>,
    ) -> Result<(), StoreError> {
        self.update(position, |entry| {
            entry.mix_in_ms = mix_in_ms;
            entry.mix_out_ms = mix_out_ms;
            entry.cue_source = Some(CueSource::Euphoria);
        })
    }

    /// Sets the mix-in of the track at `position` by hand.
    pub fn set_mix_in(&mut self, position: usize, mix_in_ms: u32) -> Result<(), StoreError> {
        self.update(position, |entry| {
            entry.mix_in_ms = Some(mix_in_ms);
            entry.cue_source = Some(CueSource::Manual);
        })
    }

    /// Sets the mix-out of the track at `position` by hand.
    pub fn set_mix_out(&mut self, position: usize, mix_out_ms: u32) -> Result<(), StoreError> {
        self.update(position, |entry| {
            entry.mix_out_ms = Some(mix_out_ms);
            entry.cue_source = Some(CueSource::Manual);
        })
    }

    /// Makes `edit` to the grid of the track at `position`. A moved grid needs confirming
    /// again.
    pub fn edit_grid(&mut self, position: usize, edit: GridEdit) -> Result<(), StoreError> {
        self.update(position, |entry| {
            TrackGrid::of(entry).edited(edit).write_to(entry)
        })
    }

    /// Every track's grid fix, by its file, whether or not the track is in this set.
    pub fn track_grids(&self) -> Result<HashMap<PathBuf, TrackGrid>, StoreError> {
        self.store.track_grids()
    }

    /// Keeps `grid` as the grid of the track whose file is `path`, as a deck's grid edit does.
    /// A deck's edit is no set edit to undo, so the sets kept for undo carry it too: undoing a
    /// set edit never takes a track's grid back.
    pub fn set_track_grid(&mut self, path: &Path, grid: TrackGrid) -> Result<(), StoreError> {
        self.store.set_track_grid(path, grid)?;
        for kept in std::iter::once(&mut self.set).chain(&mut self.undo) {
            for entry in &mut kept.entries {
                if entry.track_path == path {
                    grid.write_to(entry);
                }
            }
        }
        Ok(())
    }

    /// Changes the track at `position` with `change`, saving only when something changed.
    fn update(
        &mut self,
        position: usize,
        change: impl FnOnce(&mut SetEntry),
    ) -> Result<(), StoreError> {
        let Some(current) = self.tracks().get(position).map(|entry| (*entry).clone()) else {
            return Ok(());
        };
        let mut updated = current.clone();
        change(&mut updated);
        if updated != current {
            self.edit(|store, id| store.update_entry(id, SetList::Main, position, updated))?;
        }
        Ok(())
    }

    /// Points the set's entries and seams that name the file `from` at its new place `to`.
    pub fn relink(&mut self, from: &Path, to: &Path) -> Result<(), StoreError> {
        self.follow_files(&HashMap::from([(from.to_path_buf(), to.to_path_buf())]))
    }

    /// Points the set's entries and seams, and the cues and playlist places kept for each file,
    /// at the new places of files that moved, each keyed by its old place. Where a file lives
    /// is not an edit to undo, so this is no step of its own, and the sets kept for undo follow
    /// the files too: undoing an edit never sends a track back to a place its file left.
    pub fn follow_files(&mut self, moves: &HashMap<PathBuf, PathBuf>) -> Result<(), StoreError> {
        self.store.follow_moved_files(moves)?;
        let moved = |path: &PathBuf| moves.get(path).cloned();
        let mut followed = self.set.clone();
        for entry in &mut followed.entries {
            if let Some(path) = moved(&entry.track_path) {
                entry.track_path = path;
            }
        }
        for seam in &mut followed.seams {
            *seam = follow_seam(seam, moves);
        }
        if followed == self.set {
            return Ok(());
        }
        self.set = self.store.replace_set(&followed)?;
        for kept in &mut self.undo {
            for entry in &mut kept.entries {
                if let Some(track_path) = moved(&entry.track_path) {
                    entry.track_path = track_path;
                }
            }
            for seam in &mut kept.seams {
                *seam = follow_seam(seam, moves);
            }
        }
        Ok(())
    }

    /// The cues the DJ made in Dubplate, by track.
    pub fn track_cues(&self) -> Result<HashMap<TrackIdentity, TrackCues>, StoreError> {
        self.store.track_cues()
    }

    /// What Dubplate has stored for each of the tracks, in one query; a track with nothing
    /// stored for it is absent.
    pub fn track_metadata_many(
        &self,
        tracks: &[TrackIdentity],
    ) -> Result<HashMap<TrackIdentity, TrackMetadata>, StoreError> {
        self.store.track_metadata_many(tracks)
    }

    /// The DJ's My Tag categories and their tags in the DJ's order.
    pub fn tag_categories(&self) -> Result<Vec<TagCategory>, StoreError> {
        self.store.tag_categories()
    }

    /// Carries out a change to a track's rating, colour, comment or tags, or to the My Tag
    /// system, in one transaction. Like the cues, it is no edit of the set, so there is no step
    /// to undo.
    pub fn edit_track_tags(&mut self, edit: TrackTagEdit) -> Result<TrackTagEdited, StoreError> {
        self.store.edit_track_tags(edit)
    }

    /// Keeps `cues` as the whole list of the track `identity`, whose file is at `track_path`.
    /// Cues are no edit of the set, so this is no step to undo.
    pub fn save_track_cues(
        &mut self,
        identity: &TrackIdentity,
        track_path: Option<&Path>,
        cues: &TrackCues,
    ) -> Result<(), StoreError> {
        self.store.save_track_cues(identity, track_path, cues)
    }

    /// The DJ's playlists and folders.
    pub fn playlists(&self) -> Result<PlaylistTree, StoreError> {
        self.store.playlists()
    }

    /// Carries out a change to the DJ's playlists, returning what it did and the playlists
    /// after it. Playlists are no edit of the set, so this is no step to undo.
    pub fn edit_playlists(
        &mut self,
        edit: PlaylistEdit,
    ) -> Result<(PlaylistEdited, PlaylistTree), StoreError> {
        let edited = self.store.edit_playlists(edit)?;
        Ok((edited, self.store.playlists()?))
    }

    /// Keeps a track as a backup, or stops keeping it.
    pub fn toggle_backup(&mut self, track: SetTrackRef) -> Result<(), StoreError> {
        let backups = self.backups();
        match backups
            .iter()
            .position(|entry| entry.track_path == track.path)
        {
            Some(position) => {
                self.edit(|store, id| store.remove_entry(id, SetList::Backups, position))
            }
            None => {
                let position = backups.len();
                self.edit(|store, id| {
                    store.insert_entry(id, SetList::Backups, position, entry(track))
                })
            }
        }
    }

    /// Goes back one edit, writing the set as it was. Returns whether there was one.
    pub fn undo(&mut self) -> Result<bool, StoreError> {
        let Some(previous) = self.undo.last() else {
            return Ok(false);
        };
        self.set = self.store.replace_set(previous)?;
        self.undo.pop();
        Ok(true)
    }

    /// Records a play in the history kept beside the set. Plays are not set edits: undo
    /// never takes one back.
    pub fn record_play(&mut self, play: &Play) -> Result<PlayId, StoreError> {
        self.store.record_play(play)
    }

    /// Sets how long a recorded play has played in all.
    pub fn set_play_length(&mut self, id: PlayId, played: Duration) -> Result<(), StoreError> {
        self.store.set_play_length(id, played)
    }

    /// Saves the selected CDJ histories and track cues as one change.
    pub fn import_usb_selection(
        &mut self,
        histories: &[ImportedHistory],
        tracks: &[(TrackIdentity, Option<PathBuf>, TrackCues)],
    ) -> Result<Vec<(i64, ImportedHistory)>, StoreError> {
        self.store.import_usb_selection(histories, tracks)
    }

    /// Every imported CDJ history.
    pub fn usb_histories(&self) -> Result<Vec<(i64, ImportedHistory)>, StoreError> {
        self.store.usb_histories()
    }

    /// Every recorded play, newest first.
    pub fn play_history(&self) -> Result<Vec<RecordedPlay>, StoreError> {
        self.store.play_history()
    }

    /// Carries out one change on the store, then reads the set back, keeping what it was for
    /// undo.
    fn edit(
        &mut self,
        change: impl FnOnce(&mut SetStore, SetId) -> Result<(), StoreError>,
    ) -> Result<(), StoreError> {
        let before = self.set.clone();
        change(&mut self.store, self.id)?;
        self.set = self.store.load_set(self.id)?;
        if self.undo.len() == UNDO_DEPTH {
            self.undo.remove(0);
        }
        self.undo.push(before);
        Ok(())
    }
}

/// `seam` with each end at its file's new place, when that file moved.
fn follow_seam(seam: &Seam, moves: &HashMap<PathBuf, PathBuf>) -> Seam {
    let followed = |path: &PathBuf| moves.get(path).unwrap_or(path).clone();
    Seam {
        from_track_path: followed(&seam.from_track_path),
        to_track_path: followed(&seam.to_track_path),
        ..seam.clone()
    }
}

fn entry(track: SetTrackRef) -> SetEntry {
    SetEntry {
        track_path: track.path,
        rekordbox_id: track.rekordbox_id,
        ..SetEntry::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(name: &str) -> SetTrackRef {
        SetTrackRef {
            path: PathBuf::from(format!("/Music/{name}.mp3")),
            rekordbox_id: None,
        }
    }

    fn names(editor: &SetEditor) -> Vec<String> {
        editor
            .tracks()
            .iter()
            .map(|entry| {
                entry
                    .track_path
                    .file_stem()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }

    fn editor(directory: &tempfile::TempDir) -> SetEditor {
        SetEditor::open(&directory.path().join("sets.sqlite")).unwrap()
    }

    #[test]
    fn undo_removes_seams_created_by_the_last_edit() {
        let directory = tempfile::tempdir().unwrap();
        let mut editor = editor(&directory);
        editor.add(track("a")).unwrap();
        editor.add(track("b")).unwrap();
        editor.toggle_free(0).unwrap();
        assert!(editor.is_free_after(0));
        assert!(editor.undo().unwrap());
        assert!(editor.set().seams.is_empty());
        assert!(
            SetEditor::open(&directory.path().join("sets.sqlite"))
                .unwrap()
                .set()
                .seams
                .is_empty()
        );
    }

    #[test]
    fn failed_undo_preserves_the_saved_set_and_can_be_retried() {
        let directory = tempfile::tempdir().unwrap();
        let mut editor = editor(&directory);
        editor.add(track("a")).unwrap();
        editor.add(track("b")).unwrap();
        editor.remove(0).unwrap();
        let before = editor.set().clone();
        let connection = rusqlite::Connection::open(directory.path().join("sets.sqlite")).unwrap();
        connection
            .execute_batch(
                "CREATE TRIGGER fail_restore BEFORE INSERT ON set_entry BEGIN
             SELECT RAISE(ABORT, 'injected write failure'); END;",
            )
            .unwrap();
        assert!(editor.undo().is_err());
        assert_eq!(editor.set(), &before);
        assert_eq!(
            SetEditor::open(&directory.path().join("sets.sqlite"))
                .unwrap()
                .set(),
            &before
        );
        connection
            .execute_batch("DROP TRIGGER fail_restore;")
            .unwrap();
        assert!(editor.undo().unwrap());
        assert_eq!(names(&editor), ["a", "b"]);
    }

    #[test]
    fn a_relinked_file_keeps_its_place_its_cues_and_its_backup_through_undo() {
        let directory = tempfile::tempdir().unwrap();
        let mut editor = editor(&directory);
        editor.add(track("a")).unwrap();
        editor.add(track("b")).unwrap();
        editor.set_mix_in(1, 4_000).unwrap();
        editor.toggle_backup(track("b")).unwrap();
        let from = PathBuf::from("/Music/b.mp3");
        let to = PathBuf::from("/Elsewhere/b.mp3");
        editor.relink(&from, &to).unwrap();
        assert_eq!(editor.tracks()[1].track_path, to);
        assert_eq!(editor.tracks()[1].mix_in_ms, Some(4_000));
        assert_eq!(editor.backups()[0].track_path, to);
        assert_eq!(editor.position(&to), Some(1));
        editor.undo().unwrap();
        assert!(editor.backups().is_empty(), "undo takes back the last edit");
        assert_eq!(
            editor.tracks()[1].track_path,
            to,
            "and leaves the track where its file is now"
        );
        assert_eq!(editor.tracks()[1].mix_in_ms, Some(4_000));
    }

    #[test]
    fn tracks_are_added_once_and_inserted_after_the_selection() {
        let directory = tempfile::tempdir().unwrap();
        let mut editor = editor(&directory);
        assert_eq!(editor.add(track("a")).unwrap(), 0);
        assert_eq!(editor.add(track("c")).unwrap(), 1);
        assert_eq!(
            editor.add(track("a")).unwrap(),
            0,
            "a track is in a set once"
        );
        assert_eq!(editor.insert_after(Some(0), track("b")).unwrap(), 1);
        assert_eq!(names(&editor), ["a", "b", "c"]);
        assert_eq!(editor.insert_after(Some(2), track("a")).unwrap(), 2);
        assert_eq!(names(&editor), ["b", "c", "a"]);
        assert_eq!(editor.insert_after(None, track("z")).unwrap(), 0);
        assert_eq!(names(&editor), ["z", "b", "c", "a"]);
    }

    #[test]
    fn an_out_of_date_selection_lands_where_the_dj_aimed() {
        let directory = tempfile::tempdir().unwrap();
        let mut editor = editor(&directory);
        for name in ["a", "b", "c"] {
            editor.add(track(name)).unwrap();
        }
        // The DJ picked after the third track, then removed two: the position is out of date.
        editor.remove(2).unwrap();
        editor.remove(1).unwrap();
        assert_eq!(names(&editor), ["a"]);
        // Moving b with that old position: it lands at the end, where the aim was.
        assert_eq!(editor.insert_after(Some(2), track("b")).unwrap(), 1);
        assert_eq!(names(&editor), ["a", "b"]);
        // Adding a new track with the old position: it is appended.
        assert_eq!(editor.insert_after(Some(2), track("z")).unwrap(), 2);
        assert_eq!(names(&editor), ["a", "b", "z"]);
        // The same old position on the track that already sits where the aim was leaves it.
        assert_eq!(editor.insert_after(Some(2), track("z")).unwrap(), 2);
    }

    #[test]
    fn tracks_move_and_leave_and_every_edit_undoes() {
        let directory = tempfile::tempdir().unwrap();
        let mut editor = editor(&directory);
        for name in ["a", "b", "c"] {
            editor.add(track(name)).unwrap();
        }
        assert_eq!(editor.move_track(2, 0).unwrap(), 0);
        assert_eq!(names(&editor), ["c", "a", "b"]);
        assert_eq!(editor.move_track(0, 9).unwrap(), 2);
        assert_eq!(names(&editor), ["a", "b", "c"]);
        editor.remove(1).unwrap();
        assert_eq!(names(&editor), ["a", "c"]);
        assert!(editor.undo().unwrap());
        assert_eq!(names(&editor), ["a", "b", "c"]);
        while editor.undo().unwrap() {}
        assert!(editor.tracks().is_empty());
    }

    #[test]
    fn seams_are_planned_or_free_and_follow_the_pair() {
        let directory = tempfile::tempdir().unwrap();
        let mut editor = editor(&directory);
        editor.add(track("a")).unwrap();
        editor.toggle_free(0).unwrap();
        assert!(!editor.is_free_after(0), "the last track has no seam");
        editor.add(track("b")).unwrap();
        editor.toggle_free(0).unwrap();
        assert!(editor.is_free_after(0));
        editor.insert_after(None, track("z")).unwrap();
        assert!(!editor.is_free_after(0), "z into a was never marked");
        assert!(editor.is_free_after(1), "a into b is still free");
        editor.undo().unwrap();
        editor.toggle_free(0).unwrap();
        assert!(!editor.is_free_after(0));
    }

    #[test]
    fn cues_and_backups_are_kept_and_the_set_survives_a_restart() {
        let directory = tempfile::tempdir().unwrap();
        {
            let mut editor = editor(&directory);
            editor.add(track("a")).unwrap();
            editor
                .set_proposed_cues(0, Some(32_000), Some(300_000))
                .unwrap();
            editor.toggle_backup(track("x")).unwrap();
            editor.toggle_backup(track("y")).unwrap();
            editor.toggle_backup(track("x")).unwrap();
        }
        let reopened = editor(&directory);
        assert_eq!(names(&reopened), ["a"]);
        let first = reopened.tracks()[0].clone();
        assert_eq!(first.mix_in_ms, Some(32_000));
        assert_eq!(first.mix_out_ms, Some(300_000));
        assert_eq!(first.cue_source, Some(CueSource::Euphoria));
        let backups: Vec<PathBuf> = reopened
            .backups()
            .iter()
            .map(|entry| entry.track_path.clone())
            .collect();
        assert_eq!(backups, [PathBuf::from("/Music/y.mp3")]);
        assert_eq!(reopened.target(), Duration::from_secs(2 * 60 * 60));
    }

    #[test]
    fn a_decks_grid_edit_survives_undoing_a_set_edit() {
        let directory = tempfile::tempdir().unwrap();
        let mut editor = editor(&directory);
        editor.add(track("a")).unwrap();
        let path = PathBuf::from("/Music/a.mp3");
        let deck_grid = TrackGrid::default().nudged(15);
        editor.set_track_grid(&path, deck_grid).unwrap();
        assert_eq!(TrackGrid::of(editor.tracks()[0]), deck_grid);
        editor.add(track("b")).unwrap();
        editor.undo().unwrap();
        editor.undo().unwrap();
        assert!(editor.tracks().is_empty());
        editor.add(track("a")).unwrap();
        assert_eq!(TrackGrid::of(editor.tracks()[0]), deck_grid);
        assert_eq!(editor.track_grids().unwrap()[&path], deck_grid);
    }

    #[test]
    fn grid_fixes_and_hand_set_cues_are_kept_and_a_moved_grid_needs_confirming() {
        let directory = tempfile::tempdir().unwrap();
        let mut editor = editor(&directory);
        editor.add(track("a")).unwrap();
        editor.edit_grid(0, GridEdit::ToggleConfirmed).unwrap();
        assert!(editor.tracks()[0].grid_confirmed);
        editor.edit_grid(0, GridEdit::Nudge(-5)).unwrap();
        editor.edit_grid(0, GridEdit::Nudge(-5)).unwrap();
        editor.edit_grid(0, GridEdit::ShiftDownbeat(-1)).unwrap();
        editor.set_mix_in(0, 16_000).unwrap();
        editor.set_mix_out(0, 330_000).unwrap();
        let entry = editor.tracks()[0].clone();
        assert!(!entry.grid_confirmed);
        assert_eq!(entry.grid_offset_ms, -10);
        assert_eq!(
            entry.downbeat_shift, 3,
            "one beat back is three beats on in a bar"
        );
        assert_eq!(
            (entry.mix_in_ms, entry.mix_out_ms),
            (Some(16_000), Some(330_000))
        );
        assert_eq!(entry.cue_source, Some(CueSource::Manual));
        drop(editor);
        let reopened = SetEditor::open(&directory.path().join("sets.sqlite")).unwrap();
        assert_eq!(reopened.tracks()[0].grid_offset_ms, -10);
    }
}
