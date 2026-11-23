//! The set store survives a session, refuses rekordbox folders and newer schemas,
//! and keeps a backup before every migration.

use std::fs;
use std::path::{Path, PathBuf};

use std::time::{Duration, UNIX_EPOCH};

use dubplate_prep::{
    CueSource, EqIntent, GridCorrection, Play, PlayDeck, Seam, SeamStatus, Set, SetEntry, SetId,
    SetList, SetStore, StoreError, TrackGrid,
};
use rusqlite::Connection;
use tempfile::TempDir;

/// Opens a throwaway database inside `directory` and keeps the folder alive.
#[allow(clippy::unwrap_used)]
fn open_store(directory: &Path) -> (TempDir, PathBuf, SetStore) {
    let directory_holder = TempDir::new_in(directory).unwrap();
    let path = directory_holder.path().join("dubplate.db");
    let store = SetStore::open(&path).unwrap();
    (directory_holder, path, store)
}

fn entry(track_path: &str, is_backup: bool) -> SetEntry {
    SetEntry {
        track_path: PathBuf::from(track_path),
        is_backup,
        ..Default::default()
    }
}

fn seam(from_path: &str, to_path: &str, eq_intent: EqIntent) -> Seam {
    Seam {
        from_track_path: PathBuf::from(from_path),
        to_track_path: PathBuf::from(to_path),
        overlap_bars: 8,
        eq_intent,
        status: SeamStatus::Planned,
        note: String::new(),
    }
}

/// A play of `track_path` on the deck with `letter`, started `minute` minutes into 2026.
#[allow(clippy::unwrap_used)]
fn play(track_path: &str, letter: char, minute: u64) -> Play {
    Play {
        set: None,
        track_path: PathBuf::from(track_path),
        rekordbox_id: None,
        title: format!("{track_path} title"),
        artist: format!("{track_path} artist"),
        started_at: UNIX_EPOCH + Duration::from_secs(1_767_225_600 + minute * 60),
        played: Duration::from_secs(60),
        deck: PlayDeck::new(letter).unwrap(),
    }
}

#[allow(clippy::unwrap_used)]
fn row_count(database_path: &Path, table_name: &str) -> i64 {
    let connection = Connection::open(database_path).unwrap();
    connection
        .query_row(&format!("SELECT COUNT(*) FROM {table_name}"), [], |row| {
            row.get(0)
        })
        .unwrap()
}

#[test]
fn survives_close_and_reopen_with_moves_and_removals() {
    let root = tempfile::tempdir().unwrap();
    let (_directory_holder, database_path, mut store) = open_store(root.path());

    let set_id = store.create_set("Friday peak", 120).unwrap();
    store
        .insert_entry(set_id, SetList::Main, 0, entry("track-a", false))
        .unwrap();
    store
        .insert_entry(set_id, SetList::Main, 1, entry("track-b", false))
        .unwrap();
    store
        .insert_entry(set_id, SetList::Main, 2, entry("track-c", false))
        .unwrap();
    store
        .insert_entry(set_id, SetList::Backups, 0, entry("backup-x", true))
        .unwrap();
    store
        .set_seam(set_id, seam("track-a", "track-b", EqIntent::BassSwap))
        .unwrap();

    store.move_entry(set_id, SetList::Main, 0, 2).unwrap();
    store.remove_entry(set_id, SetList::Main, 2).unwrap();
    drop(store);

    let reopened = SetStore::open(&database_path).unwrap();
    let loaded = reopened.load_set(set_id).unwrap();
    assert_eq!(loaded.name, "Friday peak");
    assert_eq!(loaded.target_minutes, 120);
    assert_eq!(
        loaded
            .entries
            .iter()
            .map(|candidate| candidate.track_path.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        ["track-b", "track-c", "backup-x"]
    );
    assert!(loaded.entries[2].is_backup);
    assert_eq!(loaded.seams.len(), 1);
    assert_eq!(loaded.seams[0].from_track_path, PathBuf::from("track-a"));
    assert_eq!(loaded.seams[0].to_track_path, PathBuf::from("track-b"));
    assert_eq!(loaded.seams[0].eq_intent, EqIntent::BassSwap);
    assert!(loaded.seam_after(0).is_none());

    let listed = reopened.list_sets().unwrap();
    assert_eq!(listed, vec![(set_id, "Friday peak".to_owned(), 3)]);
}

#[test]
fn refuses_a_database_newer_than_this_build() {
    let root = tempfile::tempdir().unwrap();
    let (_directory_holder, database_path, _) = open_store(root.path());

    let connection = Connection::open(&database_path).unwrap();
    connection.pragma_update(None, "user_version", 99).unwrap();
    drop(connection);

    let result = SetStore::open(&database_path);
    assert!(matches!(
        result,
        Err(StoreError::NewerSchema { found: 99, known }) if known == SetStore::SCHEMA_VERSION
    ));
}

#[test]
fn refuses_a_path_inside_a_pioneer_folder() {
    let result = SetStore::open(Path::new("/home/dj/Pioneer/rekordbox/8A/dubplate.db"));
    assert!(matches!(result, Err(StoreError::InsideRekordbox(_))));

    let root = tempfile::tempdir().unwrap();
    let result = SetStore::open(root.path().join("rekordbox").join("dubplate.db").as_path());
    assert!(matches!(result, Err(StoreError::InsideRekordbox(_))));
}

#[test]
fn a_migration_on_an_existing_file_leaves_a_backup() {
    let root = tempfile::tempdir().unwrap();
    let directory_holder = TempDir::new_in(root.path()).unwrap();
    let database_path = directory_holder.path().join("dubplate.db");
    fs::File::create(&database_path).unwrap();

    let mut store = SetStore::open(&database_path).unwrap();
    let set_id = store.create_set("After the upgrade", 90).unwrap();
    store
        .insert_entry(set_id, SetList::Main, 0, entry("track-a", false))
        .unwrap();
    drop(store);

    let backups_folder = database_path.parent().unwrap().join("backups");
    assert_eq!(fs::read_dir(&backups_folder).unwrap().count(), 1);
    let backup_name = fs::read_dir(&backups_folder)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .file_name()
        .to_string_lossy()
        .into_owned();
    assert!(backup_name.starts_with("v0-"));

    let backup_connection = Connection::open(backups_folder.join(&backup_name)).unwrap();
    let user_version: i64 = backup_connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(user_version, 0);

    let reopened = SetStore::open(&database_path).unwrap();
    let loaded = reopened.load_set(set_id).unwrap();
    assert_eq!(loaded.entries.len(), 1);
}

#[test]
fn deleting_a_set_removes_its_entries_and_seams_and_keeps_its_plays() {
    let root = tempfile::tempdir().unwrap();
    let (_directory_holder, database_path, mut store) = open_store(root.path());

    let set_id = store.create_set("Short set", 30).unwrap();
    store
        .insert_entry(set_id, SetList::Main, 0, entry("track-a", false))
        .unwrap();
    store
        .insert_entry(set_id, SetList::Main, 1, entry("track-b", false))
        .unwrap();
    store
        .set_seam(
            set_id,
            Seam {
                from_track_path: PathBuf::from("track-a"),
                to_track_path: PathBuf::from("track-b"),
                overlap_bars: 4,
                eq_intent: EqIntent::HighPassIncoming,
                status: SeamStatus::Free,
                note: String::new(),
            },
        )
        .unwrap();
    store.snapshot(set_id, "lock").unwrap();
    store
        .record_play(&Play {
            set: Some(set_id),
            rekordbox_id: Some("rekordbox-a".to_owned()),
            ..play("track-a", 'A', 0)
        })
        .unwrap();
    store.delete_set(set_id).unwrap();

    assert_eq!(row_count(&database_path, "set_entry"), 0);
    assert_eq!(row_count(&database_path, "seam"), 0);
    assert_eq!(row_count(&database_path, "set_revision"), 0);
    let history = store.play_history().unwrap();
    assert_eq!(history.len(), 1, "a play outlives its set");
    assert_eq!(history[0].play.set, None);
    assert!(matches!(
        store.load_set(set_id),
        Err(StoreError::SetNotFound(_))
    ));
}

#[test]
fn a_snapshot_round_trips_through_json() {
    let root = tempfile::tempdir().unwrap();
    let (_directory_holder, database_path, mut store) = open_store(root.path());

    let set_id = store.create_set("Snapshot set", 60).unwrap();
    store
        .insert_entry(
            set_id,
            SetList::Main,
            0,
            SetEntry {
                track_path: PathBuf::from("track-a"),
                rekordbox_id: Some("rekordbox-a".to_owned()),
                mix_in_ms: Some(32_000),
                mix_out_ms: Some(200_000),
                cue_source: Some(CueSource::Euphoria),
                grid_confirmed: true,
                energy_override: Some(7.5),
                vocal_in_ms: Some(16_000),
                vocal_out_ms: Some(120_000),
                note: "peak energy".to_owned(),
                is_backup: false,
                grid_offset_ms: 8,
                downbeat_shift: -1,
            },
        )
        .unwrap();

    store.rename_set(set_id, "Snapshot set v2").unwrap();
    let revision_id = store.snapshot(set_id, "lock").unwrap();
    assert!(revision_id > 0);

    let reopened = SetStore::open(&database_path).unwrap();
    let connection = Connection::open(&database_path).unwrap();
    let (reason, json): (String, String) = connection
        .query_row(
            "SELECT reason, json FROM set_revision WHERE id = ?1",
            (revision_id,),
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(reason, "lock");
    let from_snapshot: Set = serde_json::from_str(&json).unwrap();
    assert_eq!(reopened.load_set(set_id).unwrap().name, "Snapshot set v2");
    assert_eq!(from_snapshot.id, set_id);
    assert_eq!(from_snapshot.entries.len(), 1);
    assert_eq!(
        from_snapshot.entries[0].track_path,
        PathBuf::from("track-a")
    );
    assert_eq!(from_snapshot.entries[0].mix_in_ms, Some(32_000));
    assert_eq!(
        from_snapshot.entries[0].cue_source,
        Some(CueSource::Euphoria)
    );
    assert!(from_snapshot.entries[0].grid_confirmed);
}

#[test]
fn moving_a_track_keeps_each_seam_plan_on_its_pair() {
    let root = tempfile::tempdir().unwrap();
    let (_directory_holder, _database_path, mut store) = open_store(root.path());

    let set_id = store.create_set("Seam follow", 60).unwrap();
    store
        .insert_entry(set_id, SetList::Main, 0, entry("track-a", false))
        .unwrap();
    store
        .insert_entry(set_id, SetList::Main, 1, entry("track-b", false))
        .unwrap();
    store
        .insert_entry(set_id, SetList::Main, 2, entry("track-c", false))
        .unwrap();
    store
        .set_seam(set_id, seam("track-a", "track-b", EqIntent::BassSwap))
        .unwrap();

    let loaded = store.load_set(set_id).unwrap();
    assert_eq!(loaded.seam_after(0).unwrap().eq_intent, EqIntent::BassSwap);

    store.move_entry(set_id, SetList::Main, 1, 2).unwrap();
    let reordered = store.load_set(set_id).unwrap();
    assert!(reordered.seam_after(0).is_none());
    let stored_plan = &reordered.seams[0];
    assert_eq!(stored_plan.from_track_path, PathBuf::from("track-a"));
    assert_eq!(stored_plan.to_track_path, PathBuf::from("track-b"));
    assert_eq!(stored_plan.eq_intent, EqIntent::BassSwap);

    store.move_entry(set_id, SetList::Main, 2, 1).unwrap();
    let restored = store.load_set(set_id).unwrap();
    assert_eq!(
        restored.seam_after(0).unwrap().eq_intent,
        EqIntent::BassSwap
    );
}

#[test]
fn a_folder_track_without_a_rekordbox_id_round_trips() {
    let root = tempfile::tempdir().unwrap();
    let (_directory_holder, database_path, mut store) = open_store(root.path());

    let set_id = store.create_set("Folder set", 60).unwrap();
    let folder_path = PathBuf::from("/home/dj/Music/afro house/bandcamp-release/a-side.mp3");
    store
        .insert_entry(
            set_id,
            SetList::Main,
            0,
            entry(folder_path.to_str().unwrap(), false),
        )
        .unwrap();
    store
        .insert_entry(set_id, SetList::Main, 1, entry("track-b", false))
        .unwrap();
    store
        .set_seam(
            set_id,
            seam(folder_path.to_str().unwrap(), "track-b", EqIntent::None),
        )
        .unwrap();
    store
        .record_play(&Play {
            set: Some(set_id),
            ..play(folder_path.to_str().unwrap(), 'B', 0)
        })
        .unwrap();
    drop(store);

    let reopened = SetStore::open(&database_path).unwrap();
    let loaded = reopened.load_set(set_id).unwrap();
    assert_eq!(loaded.entries[0].track_path, folder_path);
    assert_eq!(loaded.entries[0].rekordbox_id, None);
    assert_eq!(loaded.seam_after(0).unwrap().from_track_path, folder_path);

    let history = reopened.play_history().unwrap();
    assert_eq!(history[0].play.track_path, folder_path);
    assert_eq!(history[0].play.rekordbox_id, None);
}

#[test]
fn main_and_backups_keep_separate_positions() {
    let root = tempfile::tempdir().unwrap();
    let (_directory_holder, _database_path, mut store) = open_store(root.path());

    let set_id = store.create_set("Two lists", 60).unwrap();
    store
        .insert_entry(set_id, SetList::Main, 0, entry("track-a", false))
        .unwrap();
    store
        .insert_entry(set_id, SetList::Main, 1, entry("track-b", false))
        .unwrap();

    store
        .insert_entry(set_id, SetList::Backups, 0, entry("backup-x", true))
        .unwrap();
    store
        .insert_entry(set_id, SetList::Backups, 1, entry("backup-y", true))
        .unwrap();
    let after_insert = store.load_set(set_id).unwrap();
    assert_eq!(
        after_insert
            .main_entries()
            .map(|candidate| candidate.track_path.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        ["track-a", "track-b"]
    );

    store.remove_entry(set_id, SetList::Backups, 0).unwrap();
    store
        .insert_entry(set_id, SetList::Backups, 1, entry("backup-z", true))
        .unwrap();
    let after_backup_edits = store.load_set(set_id).unwrap();
    assert_eq!(
        after_backup_edits
            .main_entries()
            .map(|candidate| candidate.track_path.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        ["track-a", "track-b"]
    );

    store.remove_entry(set_id, SetList::Main, 0).unwrap();
    store.move_entry(set_id, SetList::Main, 0, 1).unwrap();
    let after_main_edits = store.load_set(set_id).unwrap();
    assert_eq!(
        after_main_edits
            .entries
            .iter()
            .filter(|candidate| candidate.is_backup)
            .map(|candidate| candidate.track_path.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        ["backup-y", "backup-z"]
    );
}

#[test]
fn updated_at_grows_after_removal_and_seam_edits() {
    let root = tempfile::tempdir().unwrap();
    let (_directory_holder, _database_path, mut store) = open_store(root.path());

    let set_id = store.create_set("Touching set", 60).unwrap();
    store
        .insert_entry(set_id, SetList::Main, 0, entry("track-a", false))
        .unwrap();
    store
        .insert_entry(set_id, SetList::Main, 1, entry("track-b", false))
        .unwrap();
    store
        .insert_entry(set_id, SetList::Main, 2, entry("track-c", false))
        .unwrap();

    let before_removal = store.load_set(set_id).unwrap().updated_at;
    std::thread::sleep(Duration::from_millis(2));
    store.remove_entry(set_id, SetList::Main, 0).unwrap();
    let after_removal = store.load_set(set_id).unwrap().updated_at;
    assert!(after_removal > before_removal);

    std::thread::sleep(Duration::from_millis(2));
    store
        .set_seam(
            set_id,
            seam("track-b", "track-c", EqIntent::HighPassIncoming),
        )
        .unwrap();
    let after_seam = store.load_set(set_id).unwrap().updated_at;
    assert!(after_seam > after_removal);
}

#[test]
fn inserting_and_moving_in_the_middle_shift_the_neighbours() {
    let base = tempfile::tempdir().unwrap();
    let (_directory, _path, mut store) = open_store(base.path());
    let set = store.create_set("Middle", 120).unwrap();
    for (position, track_path) in ["/a.mp3", "/b.mp3", "/c.mp3"].into_iter().enumerate() {
        store
            .insert_entry(set, SetList::Main, position, entry(track_path, false))
            .unwrap();
    }
    store
        .insert_entry(set, SetList::Main, 1, entry("/z.mp3", false))
        .unwrap();
    store.move_entry(set, SetList::Main, 3, 0).unwrap();
    store.move_entry(set, SetList::Main, 1, 3).unwrap();
    let order: Vec<PathBuf> = store
        .load_set(set)
        .unwrap()
        .main_entries()
        .map(|candidate| candidate.track_path.clone())
        .collect();
    let expected: Vec<PathBuf> = ["/c.mp3", "/z.mp3", "/b.mp3", "/a.mp3"]
        .into_iter()
        .map(PathBuf::from)
        .collect();
    assert_eq!(order, expected);
}

#[test]
fn a_version_one_file_gains_the_grid_corrections_and_keeps_its_entries() {
    let root = tempfile::tempdir().unwrap();
    let directory_holder = TempDir::new_in(root.path()).unwrap();
    let database_path = directory_holder.path().join("dubplate.db");
    {
        let connection = Connection::open(&database_path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE sets (id INTEGER PRIMARY KEY, name TEXT NOT NULL,
                    target_minutes INTEGER NOT NULL, created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL);
                 CREATE TABLE set_entry (set_id INTEGER NOT NULL, position INTEGER NOT NULL,
                    track_path TEXT NOT NULL, rekordbox_id TEXT, mix_in_ms INTEGER,
                    mix_out_ms INTEGER, cue_source TEXT,
                    grid_confirmed INTEGER NOT NULL DEFAULT 0, energy_override REAL,
                    vocal_in_ms INTEGER, vocal_out_ms INTEGER, note TEXT NOT NULL DEFAULT '',
                    is_backup INTEGER NOT NULL DEFAULT 0,
                    PRIMARY KEY (set_id, is_backup, position));
                 CREATE TABLE seam (set_id INTEGER NOT NULL, from_track_path TEXT NOT NULL,
                    to_track_path TEXT NOT NULL, overlap_bars INTEGER NOT NULL,
                    eq_intent TEXT NOT NULL, status TEXT NOT NULL, note TEXT NOT NULL DEFAULT '',
                    PRIMARY KEY (set_id, from_track_path, to_track_path));
                 CREATE TABLE set_revision (id INTEGER PRIMARY KEY, set_id INTEGER NOT NULL,
                    saved_at INTEGER NOT NULL, reason TEXT NOT NULL, json TEXT NOT NULL);
                 CREATE TABLE play_history (set_id INTEGER NOT NULL, track_path TEXT NOT NULL,
                    rekordbox_id TEXT, played_at INTEGER NOT NULL, deck TEXT NOT NULL,
                    off_plan INTEGER NOT NULL DEFAULT 0);
                 INSERT INTO sets VALUES (1, 'Old', 120, 0, 0);
                 INSERT INTO set_entry (set_id, position, track_path, grid_confirmed)
                    VALUES (1, 0, '/a.mp3', 1);
                 PRAGMA user_version = 1;",
            )
            .unwrap();
    }
    let mut store = SetStore::open(&database_path).unwrap();
    let set = store.load_set(SetId(1)).unwrap();
    let first = set.entries[0].clone();
    assert!(first.grid_confirmed);
    assert_eq!((first.grid_offset_ms, first.downbeat_shift), (0, 0));
    store
        .update_entry(
            SetId(1),
            SetList::Main,
            0,
            SetEntry {
                grid_offset_ms: -12,
                downbeat_shift: 1,
                ..first
            },
        )
        .unwrap();
    let corrected = store.load_set(SetId(1)).unwrap().entries[0].clone();
    assert_eq!(
        (corrected.grid_offset_ms, corrected.downbeat_shift),
        (-12, 1)
    );
}

/// A version 2 file, with the play history table every earlier build made and never wrote.
#[allow(clippy::unwrap_used)]
fn version_two_file(database_path: &Path, play_rows: &str) {
    let connection = Connection::open(database_path).unwrap();
    connection
        .execute_batch(&format!(
            "CREATE TABLE sets (id INTEGER PRIMARY KEY, name TEXT NOT NULL,
                target_minutes INTEGER NOT NULL, created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL);
             CREATE TABLE set_entry (set_id INTEGER NOT NULL REFERENCES sets(id) ON DELETE CASCADE,
                position INTEGER NOT NULL, track_path TEXT NOT NULL, rekordbox_id TEXT,
                mix_in_ms INTEGER, mix_out_ms INTEGER, cue_source TEXT,
                grid_confirmed INTEGER NOT NULL DEFAULT 0, energy_override REAL,
                vocal_in_ms INTEGER, vocal_out_ms INTEGER, note TEXT NOT NULL DEFAULT '',
                is_backup INTEGER NOT NULL DEFAULT 0,
                grid_offset_ms INTEGER NOT NULL DEFAULT 0,
                downbeat_shift INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (set_id, is_backup, position));
             CREATE TABLE seam (set_id INTEGER NOT NULL REFERENCES sets(id) ON DELETE CASCADE,
                from_track_path TEXT NOT NULL, to_track_path TEXT NOT NULL,
                overlap_bars INTEGER NOT NULL, eq_intent TEXT NOT NULL, status TEXT NOT NULL,
                note TEXT NOT NULL DEFAULT '',
                PRIMARY KEY (set_id, from_track_path, to_track_path));
             CREATE TABLE set_revision (id INTEGER PRIMARY KEY,
                set_id INTEGER NOT NULL REFERENCES sets(id) ON DELETE CASCADE,
                saved_at INTEGER NOT NULL, reason TEXT NOT NULL, json TEXT NOT NULL);
             CREATE TABLE play_history (
                set_id INTEGER NOT NULL REFERENCES sets(id) ON DELETE CASCADE,
                track_path TEXT NOT NULL, rekordbox_id TEXT, played_at INTEGER NOT NULL,
                deck TEXT NOT NULL, off_plan INTEGER NOT NULL DEFAULT 0);
             INSERT INTO sets VALUES (1, 'Friday', 120, 0, 0);
             INSERT INTO set_entry (set_id, position, track_path) VALUES (1, 0, '/a.mp3');
             {play_rows}
             PRAGMA user_version = 2;"
        ))
        .unwrap();
}

#[test]
fn a_version_two_file_keeps_its_plays_and_records_plays_outside_a_set() {
    let root = tempfile::tempdir().unwrap();
    let directory_holder = TempDir::new_in(root.path()).unwrap();
    let database_path = directory_holder.path().join("dubplate.db");
    version_two_file(
        &database_path,
        "INSERT INTO play_history VALUES (1, '/a.mp3', 'rb-1', 1767225600000, 'A', 0);
         INSERT INTO play_history VALUES (1, '/b.mp3', NULL, 1767225900000, 'B', 1);",
    );

    let mut store = SetStore::open(&database_path).unwrap();
    assert_eq!(store.load_set(SetId(1)).unwrap().entries.len(), 1);
    let history = store.play_history().unwrap();
    let kept: Vec<(PathBuf, Option<SetId>, char, Option<String>)> = history
        .iter()
        .map(|recorded| {
            (
                recorded.play.track_path.clone(),
                recorded.play.set,
                recorded.play.deck.letter(),
                recorded.play.rekordbox_id.clone(),
            )
        })
        .collect();
    assert_eq!(
        kept,
        [
            (PathBuf::from("/b.mp3"), None, 'B', None),
            (
                PathBuf::from("/a.mp3"),
                Some(SetId(1)),
                'A',
                Some("rb-1".to_owned())
            ),
        ],
        "newest first; an off-plan play is not a set track"
    );
    assert_eq!(
        history[1].play.started_at,
        UNIX_EPOCH + Duration::from_secs(1_767_225_600)
    );
    assert_eq!(history[1].play.played, Duration::ZERO);

    let outside = store.record_play(&play("/c.mp3", 'A', 30)).unwrap();
    store
        .set_play_length(outside, Duration::from_secs(312))
        .unwrap();
    drop(store);

    let reopened = SetStore::open(&database_path).unwrap();
    let newest = reopened.play_history().unwrap()[0].clone();
    assert_eq!(newest.id, outside);
    assert_eq!(newest.play.set, None);
    assert_eq!(newest.play.played, Duration::from_secs(312));
    assert_eq!(newest.play.title, "/c.mp3 title");
}

#[test]
fn an_empty_version_two_history_upgrades_and_keeps_a_backup() {
    let root = tempfile::tempdir().unwrap();
    let directory_holder = TempDir::new_in(root.path()).unwrap();
    let database_path = directory_holder.path().join("dubplate.db");
    version_two_file(&database_path, "");

    let store = SetStore::open(&database_path).unwrap();
    assert!(store.play_history().unwrap().is_empty());
    let backups = fs::read_dir(database_path.parent().unwrap().join("backups"))
        .unwrap()
        .count();
    assert_eq!(backups, 1);
}

#[test]
fn a_failed_migration_rolls_back_alone() {
    let root = tempfile::tempdir().unwrap();
    let directory_holder = TempDir::new_in(root.path()).unwrap();
    let database_path = directory_holder.path().join("dubplate.db");
    version_two_file(
        &database_path,
        "INSERT INTO play_history VALUES (1, '/a.mp3', NULL, 1767225600000, 'A', 0);",
    );
    Connection::open(&database_path)
        .unwrap()
        .execute_batch("CREATE TABLE play_history_v4 (id INTEGER);")
        .unwrap();

    assert!(SetStore::open(&database_path).is_err());
    let connection = Connection::open(&database_path).unwrap();
    let user_version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(user_version, 4, "the migration before it stays applied");
    let off_plan: i64 = connection
        .query_row("SELECT off_plan FROM play_history", [], |row| row.get(0))
        .unwrap();
    assert_eq!(off_plan, 0, "the old play history is untouched");
}

#[test]
fn updating_the_length_of_an_unknown_play_says_so() {
    let root = tempfile::tempdir().unwrap();
    let (_directory_holder, database_path, mut store) = open_store(root.path());
    let recorded = store.record_play(&play("/a.mp3", 'A', 0)).unwrap();
    Connection::open(&database_path)
        .unwrap()
        .execute_batch("DELETE FROM play_history;")
        .unwrap();
    assert!(matches!(
        store.set_play_length(recorded, Duration::from_secs(90)),
        Err(StoreError::PlayNotFound(id)) if id == recorded
    ));
}

#[test]
fn a_track_keeps_one_grid_across_sets_and_decks() {
    let root = tempfile::tempdir().unwrap();
    let (_directory_holder, _database_path, mut store) = open_store(root.path());
    let first_set = store.create_set("Friday", 60).unwrap();
    let second_set = store.create_set("Saturday", 60).unwrap();
    store
        .insert_entry(first_set, SetList::Main, 0, entry("/a.mp3", false))
        .unwrap();
    let moved = TrackGrid {
        correction: GridCorrection {
            offset_ms: 10,
            downbeat_shift: 3,
        },
        confirmed: true,
    };
    store.set_track_grid(Path::new("/a.mp3"), moved).unwrap();
    store
        .insert_entry(second_set, SetList::Main, 0, entry("/a.mp3", false))
        .unwrap();
    for set in [first_set, second_set] {
        let kept = store.load_set(set).unwrap().entries[0].clone();
        assert_eq!(
            TrackGrid::of(&kept),
            moved,
            "a new entry never resets the grid"
        );
    }
    let mut renudged = store.load_set(second_set).unwrap().entries[0].clone();
    renudged.grid_offset_ms = -5;
    store
        .update_entry(second_set, SetList::Main, 0, renudged)
        .unwrap();
    let first = store.load_set(first_set).unwrap().entries[0].clone();
    assert_eq!(first.grid_offset_ms, -5);
    assert_eq!(
        store.track_grids().unwrap()[Path::new("/a.mp3")]
            .correction
            .offset_ms,
        -5
    );
}
