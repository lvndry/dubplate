//! The set store survives a session, refuses rekordbox folders and newer schemas,
//! and keeps a backup before every migration.

use std::fs;
use std::path::{Path, PathBuf};

use dubplate_prep::{CueSource, EqIntent, Seam, SeamStatus, Set, SetEntry, SetStore, StoreError};
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

fn entry(rekordbox_id: &str, is_backup: bool) -> SetEntry {
    SetEntry {
        rekordbox_id: rekordbox_id.to_owned(),
        is_backup,
        ..Default::default()
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
        .insert_entry(set_id, 0, entry("track-a", false))
        .unwrap();
    store
        .insert_entry(set_id, 1, entry("track-b", false))
        .unwrap();
    store
        .insert_entry(set_id, 2, entry("track-c", false))
        .unwrap();
    store
        .insert_entry(set_id, 3, entry("backup-x", true))
        .unwrap();
    store
        .set_seam(
            set_id,
            Seam {
                from_position: 0,
                overlap_bars: 8,
                eq_intent: EqIntent::BassSwap,
                status: SeamStatus::Planned,
                note: String::new(),
            },
        )
        .unwrap();

    store.move_entry(set_id, 0, 2).unwrap();
    store.remove_entry(set_id, 2).unwrap();
    drop(store);

    let reopened = SetStore::open(&database_path).unwrap();
    let loaded = reopened.load_set(set_id).unwrap();
    assert_eq!(loaded.name, "Friday peak");
    assert_eq!(loaded.target_minutes, 120);
    assert_eq!(
        loaded
            .entries
            .iter()
            .map(|entry| entry.rekordbox_id.as_str())
            .collect::<Vec<_>>(),
        ["track-b", "track-c", "backup-x"]
    );
    assert!(loaded.entries[2].is_backup);
    assert_eq!(loaded.seams.len(), 1);
    assert_eq!(loaded.seams[0].eq_intent, EqIntent::BassSwap);

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
        Err(StoreError::NewerSchema {
            found: 99,
            known: 1
        })
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
        .insert_entry(set_id, 0, entry("track-a", false))
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
fn deleting_a_set_removes_its_entries_and_seams() {
    let root = tempfile::tempdir().unwrap();
    let (_directory_holder, database_path, mut store) = open_store(root.path());

    let set_id = store.create_set("Short set", 30).unwrap();
    store
        .insert_entry(set_id, 0, entry("track-a", false))
        .unwrap();
    store
        .insert_entry(set_id, 1, entry("track-b", false))
        .unwrap();
    store
        .set_seam(
            set_id,
            Seam {
                from_position: 0,
                overlap_bars: 4,
                eq_intent: EqIntent::HighPassIncoming,
                status: SeamStatus::Free,
                note: String::new(),
            },
        )
        .unwrap();
    store.snapshot(set_id, "lock").unwrap();
    store.record_play(set_id, "track-a", "A", false).unwrap();
    store.delete_set(set_id).unwrap();

    assert_eq!(row_count(&database_path, "set_entry"), 0);
    assert_eq!(row_count(&database_path, "seam"), 0);
    assert_eq!(row_count(&database_path, "set_revision"), 0);
    assert_eq!(row_count(&database_path, "play_history"), 0);
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
            0,
            SetEntry {
                rekordbox_id: "track-a".to_owned(),
                mix_in_ms: Some(32_000),
                mix_out_ms: Some(200_000),
                cue_source: Some(CueSource::Euphoria),
                grid_confirmed: true,
                energy_override: Some(7.5),
                vocal_in_ms: Some(16_000),
                vocal_out_ms: Some(120_000),
                note: "peak energy".to_owned(),
                is_backup: false,
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
    assert_eq!(from_snapshot.entries[0].rekordbox_id, "track-a");
    assert_eq!(from_snapshot.entries[0].mix_in_ms, Some(32_000));
    assert_eq!(
        from_snapshot.entries[0].cue_source,
        Some(CueSource::Euphoria)
    );
    assert!(from_snapshot.entries[0].grid_confirmed);
}
