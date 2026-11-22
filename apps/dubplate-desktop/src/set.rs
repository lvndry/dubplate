//! The set being prepared, kept in Dubplate's own set database: its tracks in order, the
//! backups beside it, and the plan of each seam. Tracks are named by their file, so a set holds
//! tracks from music folders and from rekordbox alike. Every edit saves at once and can be
//! undone for the session.

use std::path::{Path, PathBuf};
use std::time::Duration;

use dubplate_prep::{
    BEATS_PER_BAR, CueSource, EqIntent, Seam, SeamStatus, Set, SetEntry, SetId, SetList, SetStore,
    StoreError,
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
        let mut target = after.map_or(0, |position| position + 1);
        match self.position(&track.path) {
            Some(existing) => {
                if existing < target {
                    target -= 1;
                }
                let target = target.min(self.tracks().len().saturating_sub(1));
                self.move_track(existing, target)
            }
            None => {
                let target = target.min(self.tracks().len());
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

    /// Marks the grid of the track at `position` as checked by ear, or no longer.
    pub fn toggle_grid_confirmed(&mut self, position: usize) -> Result<(), StoreError> {
        self.update(position, |entry| {
            entry.grid_confirmed = !entry.grid_confirmed
        })
    }

    /// Moves the grid of the track at `position` by `milliseconds`, later when positive. A
    /// moved grid needs confirming again.
    pub fn nudge_grid(&mut self, position: usize, milliseconds: i32) -> Result<(), StoreError> {
        self.update(position, |entry| {
            entry.grid_offset_ms = entry.grid_offset_ms.saturating_add(milliseconds);
            entry.grid_confirmed = false;
        })
    }

    /// Moves the bars of the track at `position` by `beats`, so the downbeat lands where the
    /// music's bar starts. A moved grid needs confirming again.
    pub fn shift_downbeat(&mut self, position: usize, beats: i32) -> Result<(), StoreError> {
        self.update(position, |entry| {
            entry.downbeat_shift = (entry.downbeat_shift + beats).rem_euclid(BEATS_PER_BAR);
            entry.grid_confirmed = false;
        })
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

    /// Points the set's entries and seams that name the file `from` at its new place `to`, as
    /// one step to undo.
    pub fn relink(&mut self, from: &Path, to: &Path) -> Result<(), StoreError> {
        let renamed = |path: &PathBuf| {
            if path == from {
                to.to_path_buf()
            } else {
                path.clone()
            }
        };
        let main: Vec<(usize, SetEntry)> = self
            .tracks()
            .into_iter()
            .enumerate()
            .filter(|(_, entry)| entry.track_path == from)
            .map(|(position, entry)| {
                (
                    position,
                    SetEntry {
                        track_path: to.to_path_buf(),
                        ..entry.clone()
                    },
                )
            })
            .collect();
        let backups: Vec<(usize, SetEntry)> = self
            .backups()
            .into_iter()
            .enumerate()
            .filter(|(_, entry)| entry.track_path == from)
            .map(|(position, entry)| {
                (
                    position,
                    SetEntry {
                        track_path: to.to_path_buf(),
                        ..entry.clone()
                    },
                )
            })
            .collect();
        let seams: Vec<Seam> = self
            .set
            .seams
            .iter()
            .filter(|seam| seam.from_track_path == from || seam.to_track_path == from)
            .map(|seam| Seam {
                from_track_path: renamed(&seam.from_track_path),
                to_track_path: renamed(&seam.to_track_path),
                ..seam.clone()
            })
            .collect();
        if main.is_empty() && backups.is_empty() && seams.is_empty() {
            return Ok(());
        }
        self.edit(|store, id| {
            for (position, entry) in main {
                store.update_entry(id, SetList::Main, position, entry)?;
            }
            for (position, entry) in backups {
                store.update_entry(id, SetList::Backups, position, entry)?;
            }
            for seam in seams {
                store.set_seam(id, seam)?;
            }
            Ok(())
        })
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
        let Some(previous) = self.undo.pop() else {
            return Ok(false);
        };
        let id = self.id;
        for list in [SetList::Main, SetList::Backups] {
            let count = self
                .set
                .entries
                .iter()
                .filter(|entry| entry.is_backup == list.is_backup())
                .count();
            for _ in 0..count {
                self.store.remove_entry(id, list, 0)?;
            }
        }
        for list in [SetList::Main, SetList::Backups] {
            let entries = previous
                .entries
                .iter()
                .filter(|entry| entry.is_backup == list.is_backup());
            for (position, entry) in entries.enumerate() {
                self.store.insert_entry(id, list, position, entry.clone())?;
            }
        }
        for seam in &previous.seams {
            self.store.set_seam(id, seam.clone())?;
        }
        self.set = self.store.load_set(id)?;
        Ok(true)
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

fn entry(track: SetTrackRef) -> SetEntry {
    SetEntry {
        track_path: track.path,
        rekordbox_id: track.rekordbox_id,
        ..SetEntry::default()
    }
}

/// `duration` as `h:mm:ss`, or `m:ss` under an hour.
pub fn running_time(duration: Duration) -> String {
    let seconds = duration.as_secs();
    let hours = seconds / 3600;
    let minutes = seconds / 60 % 60;
    let rest = seconds % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{rest:02}")
    } else {
        format!("{minutes}:{rest:02}")
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
    fn a_relinked_file_keeps_its_place_its_cues_and_its_backup_in_one_undo() {
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
        assert_eq!(editor.tracks()[1].track_path, from);
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
    fn grid_fixes_and_hand_set_cues_are_kept_and_a_moved_grid_needs_confirming() {
        let directory = tempfile::tempdir().unwrap();
        let mut editor = editor(&directory);
        editor.add(track("a")).unwrap();
        editor.toggle_grid_confirmed(0).unwrap();
        assert!(editor.tracks()[0].grid_confirmed);
        editor.nudge_grid(0, -5).unwrap();
        editor.nudge_grid(0, -5).unwrap();
        editor.shift_downbeat(0, -1).unwrap();
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

    #[test]
    fn running_times_read_like_a_clock() {
        assert_eq!(running_time(Duration::from_secs(59)), "0:59");
        assert_eq!(running_time(Duration::from_secs(61 * 60 + 5)), "1:01:05");
    }
}
