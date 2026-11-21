//! The set being prepared: its tracks in order, the backups beside it, and the plan of each
//! seam. Tracks are named by their identity (a rekordbox id or a file path), so the set does not
//! depend on where the library happens to list them. Every edit can be undone for the session.

use std::time::Duration;

/// How long a set aims to run unless the DJ says otherwise.
pub const DEFAULT_TARGET: Duration = Duration::from_secs(2 * 60 * 60);
/// How many edits undo goes back.
const UNDO_DEPTH: usize = 200;

/// A mix cue the DJ accepted, in milliseconds from the start of the track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MixCues {
    /// Where the track comes in under the one before.
    pub mix_in_ms: Option<u32>,
    /// Where it starts handing over to the next.
    pub mix_out_ms: Option<u32>,
}

/// One track of the set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetEntry {
    /// The track's identity in the library.
    pub track: String,
    /// The mix cues the DJ accepted for it.
    pub cues: MixCues,
    /// Whether the seam into the next track is left to the moment: it never warns.
    pub free_into_next: bool,
}

impl SetEntry {
    fn new(track: String) -> Self {
        Self {
            track,
            cues: MixCues::default(),
            free_into_next: false,
        }
    }
}

/// The set and its backups.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DjSet {
    /// The set's name.
    pub name: String,
    /// How long the set should run.
    pub target: Duration,
    /// The tracks, in playing order.
    pub entries: Vec<SetEntry>,
    /// Tracks kept aside in case the room asks for something else.
    pub backups: Vec<String>,
}

impl Default for DjSet {
    fn default() -> Self {
        Self {
            name: "Set".to_owned(),
            target: DEFAULT_TARGET,
            entries: Vec::new(),
            backups: Vec::new(),
        }
    }
}

impl DjSet {
    /// Whether the track is in the set.
    pub fn contains(&self, track: &str) -> bool {
        self.entries.iter().any(|entry| entry.track == track)
    }
}

/// Where a set is kept between launches. The app holds sets in memory until the set store
/// lands; the store only has to read and write whole sets.
pub trait SetStorage {
    /// The set saved last, if any.
    fn load(&self) -> Option<DjSet>;
    /// Keeps `set`.
    fn save(&mut self, set: &DjSet);
}

/// Keeps the set for the session only.
#[derive(Debug, Default)]
pub struct SessionStorage {
    saved: Option<DjSet>,
}

impl SetStorage for SessionStorage {
    fn load(&self) -> Option<DjSet> {
        self.saved.clone()
    }

    fn save(&mut self, set: &DjSet) {
        self.saved = Some(set.clone());
    }
}

/// The set being edited, with undo, saving after every edit.
pub struct SetEditor {
    set: DjSet,
    undo: Vec<DjSet>,
    storage: Box<dyn SetStorage>,
}

impl SetEditor {
    /// Opens the set `storage` keeps, or an empty one.
    pub fn open(storage: Box<dyn SetStorage>) -> Self {
        Self {
            set: storage.load().unwrap_or_default(),
            undo: Vec::new(),
            storage,
        }
    }

    /// The set as it stands.
    pub fn set(&self) -> &DjSet {
        &self.set
    }

    /// Adds `track` at the end, unless it is in the set already. Returns where it is.
    pub fn add(&mut self, track: String) -> usize {
        if let Some(position) = self.position(&track) {
            return position;
        }
        self.edit(|set| set.entries.push(SetEntry::new(track)));
        self.set.entries.len() - 1
    }

    /// Puts `track` right after `position`, or first when the set is empty, moving it there if
    /// it is in the set already. Returns where it is.
    pub fn insert_after(&mut self, position: Option<usize>, track: String) -> usize {
        let mut target = position.map_or(0, |position| position + 1);
        self.edit(|set| {
            let entry = match set.entries.iter().position(|entry| entry.track == track) {
                Some(existing) => {
                    if existing < target {
                        target -= 1;
                    }
                    set.entries.remove(existing)
                }
                None => SetEntry::new(track),
            };
            target = target.min(set.entries.len());
            set.entries.insert(target, entry);
        });
        target
    }

    /// Moves the track at `from` to `to`. Returns where it is now.
    pub fn move_track(&mut self, from: usize, to: usize) -> usize {
        let count = self.set.entries.len();
        if from >= count {
            return from;
        }
        let to = to.min(count - 1);
        if from != to {
            self.edit(|set| {
                let entry = set.entries.remove(from);
                set.entries.insert(to, entry);
            });
        }
        to
    }

    /// Takes the track at `position` out of the set.
    pub fn remove(&mut self, position: usize) {
        if position < self.set.entries.len() {
            self.edit(|set| {
                set.entries.remove(position);
            });
        }
    }

    /// Switches the seam after `position` between planned and free.
    pub fn toggle_free(&mut self, position: usize) {
        if position + 1 < self.set.entries.len() {
            self.edit(|set| {
                let entry = &mut set.entries[position];
                entry.free_into_next = !entry.free_into_next;
            });
        }
    }

    /// Keeps the mix cues `cues` for the track at `position`.
    pub fn set_cues(&mut self, position: usize, cues: MixCues) {
        if self
            .set
            .entries
            .get(position)
            .is_some_and(|entry| entry.cues != cues)
        {
            self.edit(|set| set.entries[position].cues = cues);
        }
    }

    /// Adds `track` to the backups, or takes it off when it is there.
    pub fn toggle_backup(&mut self, track: String) {
        self.edit(|set| match set.backups.iter().position(|backup| *backup == track) {
            Some(position) => {
                set.backups.remove(position);
            }
            None => set.backups.push(track),
        });
    }

    /// Goes back one edit. Returns whether there was one.
    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.undo.pop() else {
            return false;
        };
        self.set = previous;
        self.storage.save(&self.set);
        true
    }

    /// Where `track` is in the set.
    pub fn position(&self, track: &str) -> Option<usize> {
        self.set.entries.iter().position(|entry| entry.track == track)
    }

    fn edit(&mut self, change: impl FnOnce(&mut DjSet)) {
        let before = self.set.clone();
        change(&mut self.set);
        if self.set != before {
            if self.undo.len() == UNDO_DEPTH {
                self.undo.remove(0);
            }
            self.undo.push(before);
            self.storage.save(&self.set);
        }
    }
}

/// `duration` as `h:mm:ss`, or `m:ss` under an hour.
pub fn running_time(duration: Duration) -> String {
    let seconds = duration.as_secs();
    let (hours, minutes, rest) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{rest:02}")
    } else {
        format!("{minutes}:{rest:02}")
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    fn tracks(editor: &SetEditor) -> Vec<&str> {
        editor
            .set()
            .entries
            .iter()
            .map(|entry| entry.track.as_str())
            .collect()
    }

    fn editor() -> SetEditor {
        SetEditor::open(Box::new(SessionStorage::default()))
    }

    #[test]
    fn tracks_are_added_once_and_inserted_after_the_selection() {
        let mut editor = editor();
        assert_eq!(editor.add("a".to_owned()), 0);
        assert_eq!(editor.add("c".to_owned()), 1);
        assert_eq!(editor.add("a".to_owned()), 0, "a track is in a set once");
        assert_eq!(editor.insert_after(Some(0), "b".to_owned()), 1);
        assert_eq!(tracks(&editor), ["a", "b", "c"]);
        assert_eq!(editor.insert_after(Some(2), "a".to_owned()), 2);
        assert_eq!(tracks(&editor), ["b", "c", "a"]);
        assert_eq!(editor.insert_after(None, "z".to_owned()), 0);
        assert_eq!(tracks(&editor), ["z", "b", "c", "a"]);
    }

    #[test]
    fn tracks_move_and_leave_and_every_edit_undoes() {
        let mut editor = editor();
        for track in ["a", "b", "c"] {
            editor.add(track.to_owned());
        }
        assert_eq!(editor.move_track(2, 0), 0);
        assert_eq!(tracks(&editor), ["c", "a", "b"]);
        assert_eq!(editor.move_track(0, 9), 2);
        assert_eq!(tracks(&editor), ["a", "b", "c"]);
        editor.remove(1);
        assert_eq!(tracks(&editor), ["a", "c"]);
        assert!(editor.undo());
        assert_eq!(tracks(&editor), ["a", "b", "c"]);
        assert!(editor.undo() && editor.undo());
        assert_eq!(tracks(&editor), ["a", "b", "c"]);
        while editor.undo() {}
        assert!(editor.set().entries.is_empty());
    }

    #[test]
    fn seams_are_planned_or_free_and_cues_are_kept() {
        let mut editor = editor();
        editor.add("a".to_owned());
        editor.toggle_free(0);
        assert!(!editor.set().entries[0].free_into_next, "the last track has no seam");
        editor.add("b".to_owned());
        editor.toggle_free(0);
        assert!(editor.set().entries[0].free_into_next);
        let cues = MixCues {
            mix_in_ms: Some(32_000),
            mix_out_ms: Some(300_000),
        };
        editor.set_cues(1, cues);
        assert_eq!(editor.set().entries[1].cues, cues);
        editor.toggle_backup("x".to_owned());
        editor.toggle_backup("y".to_owned());
        editor.toggle_backup("x".to_owned());
        assert_eq!(editor.set().backups, ["y"]);
    }

    #[test]
    fn every_edit_is_saved_at_once() {
        #[derive(Default)]
        struct Recorder(Rc<RefCell<Vec<usize>>>);
        impl SetStorage for Recorder {
            fn load(&self) -> Option<DjSet> {
                None
            }
            fn save(&mut self, set: &DjSet) {
                self.0.borrow_mut().push(set.entries.len());
            }
        }
        let saves = Rc::new(RefCell::new(Vec::new()));
        let mut editor = SetEditor::open(Box::new(Recorder(Rc::clone(&saves))));
        editor.add("a".to_owned());
        editor.add("a".to_owned());
        editor.add("b".to_owned());
        editor.undo();
        assert_eq!(*saves.borrow(), [1, 2, 1]);
    }

    #[test]
    fn running_times_read_like_a_clock() {
        assert_eq!(running_time(Duration::from_secs(59)), "0:59");
        assert_eq!(running_time(Duration::from_secs(61 * 60 + 5)), "1:01:05");
    }
}
