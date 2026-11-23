//! The cues the DJ made in Dubplate for each track: its hot cues, memory cues and saved loops,
//! kept as the track's whole list so that a cue deleted in Dubplate stays deleted, and taking
//! the place of the cues rekordbox has for that track.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use dubplate_rekordbox::{HotCue, HotCuePad, MemoryCue, MemoryCueColor, PaletteColor, TrackCues};
use rusqlite::{OptionalExtension, Transaction};

use super::{SetStore, StoreError, now_unix_millis};
use crate::identity::TrackIdentity;

/// The cues of one track as they are read, before they become its `TrackCues`.
#[derive(Default)]
struct ReadCues {
    hot: [Option<HotCue>; HotCuePad::ALL.len()],
    memory: Vec<MemoryCue>,
}

impl SetStore {
    /// The cues the DJ made in Dubplate, by track. A track with an empty list is there too:
    /// the DJ deleted every cue it had.
    pub fn track_cues(&self) -> Result<HashMap<TrackIdentity, TrackCues>, StoreError> {
        let mut tracks: HashMap<(String, String), ReadCues> = self
            .connection
            .prepare("SELECT identity_kind, identity FROM track_cues")?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .map(|key| key.map(|key| (key, ReadCues::default())))
            .collect::<Result<_, _>>()?;
        let mut statement = self.connection.prepare(
            "SELECT identity_kind, identity, pad, start_ms, loop_end_ms, color_table_index,
                memory_color, name
             FROM track_cue
             ORDER BY identity_kind, identity, start_ms, rowid",
        )?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let key: (String, String) = (row.get(0)?, row.get(1)?);
            let Some(read) = tracks.get_mut(&key) else {
                continue;
            };
            let Some(start_ms) = milliseconds(row.get(3)?) else {
                continue;
            };
            let loop_end_ms = row
                .get::<_, Option<i64>>(4)?
                .and_then(milliseconds)
                .filter(|end| *end > start_ms);
            let name: String = row.get(7)?;
            let pad = row
                .get::<_, Option<i64>>(2)?
                .map(|index| usize::try_from(index).ok().and_then(pad_at));
            match pad {
                Some(Some(pad)) => {
                    read.hot[pad.index()] = Some(HotCue {
                        start_ms,
                        loop_end_ms,
                        color: row
                            .get::<_, Option<i64>>(5)?
                            .and_then(PaletteColor::from_index),
                        name,
                    });
                }
                Some(None) => {}
                None => read.memory.push(MemoryCue {
                    start_ms,
                    loop_end_ms,
                    color: row
                        .get::<_, Option<i64>>(6)?
                        .and_then(MemoryCueColor::from_rekordbox),
                    name,
                }),
            }
        }
        Ok(tracks
            .into_iter()
            .filter_map(|((kind, key), read)| {
                let identity = TrackIdentity::from_storage_key(&kind, key)?;
                Some((identity, TrackCues::new(read.hot, read.memory)))
            })
            .collect())
    }

    /// Keeps `cues` as the whole list of the track `identity`, whose file is at `track_path`,
    /// in place of what was kept for it.
    pub fn save_track_cues(
        &mut self,
        identity: &TrackIdentity,
        track_path: Option<&Path>,
        cues: &TrackCues,
    ) -> Result<(), StoreError> {
        let transaction = self.transaction()?;
        let (kind, key) = identity.storage_key();
        transaction.execute(
            "INSERT INTO track_cues (identity_kind, identity, track_path, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (identity_kind, identity) DO UPDATE SET
                 track_path = excluded.track_path,
                 updated_at = excluded.updated_at",
            (
                kind,
                &key,
                track_path.map(Path::to_string_lossy),
                now_unix_millis(),
            ),
        )?;
        transaction.execute(
            "DELETE FROM track_cue WHERE identity_kind = ?1 AND identity = ?2",
            (kind, &key),
        )?;
        {
            let mut insert = transaction.prepare(
                "INSERT INTO track_cue
                    (identity_kind, identity, pad, start_ms, loop_end_ms, color_table_index,
                     memory_color, name)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            for (pad, cue) in cues.hot_cues() {
                insert.execute((
                    kind,
                    &key,
                    pad.index() as i64,
                    cue.start_ms,
                    cue.loop_end_ms,
                    cue.color.map(PaletteColor::index),
                    None::<u8>,
                    &cue.name,
                ))?;
            }
            for cue in cues.memory_cues() {
                insert.execute((
                    kind,
                    &key,
                    None::<i64>,
                    cue.start_ms,
                    cue.loop_end_ms,
                    None::<u8>,
                    cue.color.map(MemoryCueColor::rekordbox_value),
                    &cue.name,
                ))?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    /// Points what is kept for files that moved at their new places, each keyed by its old
    /// place: the cues and playlist places of a file rekordbox does not hold move with it, and
    /// every track's last known file follows.
    pub fn follow_moved_files(
        &mut self,
        moves: &HashMap<PathBuf, PathBuf>,
    ) -> Result<(), StoreError> {
        if moves.is_empty() {
            return Ok(());
        }
        let transaction = self.transaction()?;
        for (from, to) in moves {
            move_file_cues(&transaction, from, to)?;
            super::playlists::move_file_tracks(&transaction, from, to)?;
        }
        transaction.commit()?;
        Ok(())
    }
}

/// Moves the cues kept for the file `from` to the file `to`, in place of any kept for `to`.
fn move_file_cues(transaction: &Transaction<'_>, from: &Path, to: &Path) -> Result<(), StoreError> {
    let moved = TrackIdentity::File(from.to_path_buf());
    let (kind, from_key) = moved.storage_key();
    let to_key = to.to_string_lossy();
    let kept: Option<i64> = transaction
        .query_row(
            "SELECT 1 FROM track_cues WHERE identity_kind = ?1 AND identity = ?2",
            (kind, &from_key),
            |row| row.get(0),
        )
        .optional()?;
    if kept.is_some() {
        transaction.execute(
            "DELETE FROM track_cues WHERE identity_kind = ?1 AND identity = ?2",
            (kind, &to_key),
        )?;
        transaction.execute(
            "UPDATE track_cues SET identity = ?3 WHERE identity_kind = ?1 AND identity = ?2",
            (kind, &from_key, &to_key),
        )?;
    }
    transaction.execute(
        "UPDATE track_cues SET track_path = ?2 WHERE track_path = ?1",
        (from.to_string_lossy(), &to_key),
    )?;
    Ok(())
}

/// A pad by its place from 0 for A.
fn pad_at(index: usize) -> Option<HotCuePad> {
    HotCuePad::ALL.get(index).copied()
}

fn milliseconds(value: i64) -> Option<u32> {
    u32::try_from(value).ok()
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::super::{MIGRATIONS, SCHEMA_VERSION, SetId};
    use super::*;

    #[test]
    fn a_database_from_before_track_cues_upgrades_and_keeps_its_sets() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("dubplate.db");
        let connection = Connection::open(&path).unwrap();
        for migration in &MIGRATIONS[..3] {
            connection.execute_batch(migration).unwrap();
        }
        connection
            .execute_batch(
                "INSERT INTO sets (id, name, target_minutes, created_at, updated_at)
                 VALUES (1, 'Friday', 90, 0, 0);
                 INSERT INTO set_entry (set_id, position, track_path) VALUES (1, 0, '/Music/a.mp3');",
            )
            .unwrap();
        drop(connection);

        let mut store = SetStore::open(&path).unwrap();
        let version: i64 = store
            .connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, i64::from(SCHEMA_VERSION));
        let set = store.load_set(SetId(1)).unwrap();
        assert_eq!(set.main_entries().count(), 1);
        assert!(store.track_cues().unwrap().is_empty());
        let backups = std::fs::read_dir(directory.path().join("backups")).unwrap();
        assert!(
            backups
                .filter_map(Result::ok)
                .any(|backup| backup.file_name().to_string_lossy().starts_with("v3-")),
            "the version 3 file is backed up before the upgrade"
        );

        let identity = TrackIdentity::Rekordbox("7".to_owned());
        let mut cues = TrackCues::default();
        cues.set_hot_cue(HotCuePad::A, HotCue::at(1_000));
        store.save_track_cues(&identity, None, &cues).unwrap();
        assert_eq!(store.track_cues().unwrap().get(&identity), Some(&cues));
    }
}
