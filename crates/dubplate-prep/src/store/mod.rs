//! Dubplate's own database: sets, their ordered entries, seam plans, snapshots,
//! play history, and the cues the DJ made for each track.
//!
//! The file lives where Dubplate owns it, never inside rekordbox's folders, so
//! every write Dubplate makes stays out of rekordbox's files. A guard refuses
//! any path that is inside rekordbox's; migrations are tracked by `user_version`
//! and an existing file is backed up before any migration runs on it.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};
use thiserror::Error;

use crate::grid::{GridCorrection, TrackGrid};
use crate::sql::{execute_cached, query_row_cached};

mod cues;
mod history;
mod types;

pub use history::{Play, PlayDeck, PlayId, RecordedPlay};
pub use types::{CueSource, EqIntent, Seam, SeamStatus, Set, SetEntry, SetId, SetList};

/// The highest schema version `MIGRATIONS` can bring a database to.
const SCHEMA_VERSION: u32 = 5;

/// One migration per schema version, index = target version - 1.
const MIGRATIONS: &[&str] = &[
    r#"
CREATE TABLE sets (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    target_minutes INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE set_entry (
    set_id INTEGER NOT NULL REFERENCES sets(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    track_path TEXT NOT NULL,
    rekordbox_id TEXT,
    mix_in_ms INTEGER,
    mix_out_ms INTEGER,
    cue_source TEXT,
    grid_confirmed INTEGER NOT NULL DEFAULT 0,
    energy_override REAL,
    vocal_in_ms INTEGER,
    vocal_out_ms INTEGER,
    note TEXT NOT NULL DEFAULT '',
    is_backup INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (set_id, is_backup, position)
);

CREATE TABLE seam (
    set_id INTEGER NOT NULL REFERENCES sets(id) ON DELETE CASCADE,
    from_track_path TEXT NOT NULL,
    to_track_path TEXT NOT NULL,
    overlap_bars INTEGER NOT NULL,
    eq_intent TEXT NOT NULL,
    status TEXT NOT NULL,
    note TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (set_id, from_track_path, to_track_path)
);

CREATE TABLE set_revision (
    id INTEGER PRIMARY KEY,
    set_id INTEGER NOT NULL REFERENCES sets(id) ON DELETE CASCADE,
    saved_at INTEGER NOT NULL,
    reason TEXT NOT NULL,
    json TEXT NOT NULL
);

CREATE TABLE play_history (
    set_id INTEGER NOT NULL REFERENCES sets(id) ON DELETE CASCADE,
    track_path TEXT NOT NULL,
    rekordbox_id TEXT,
    played_at INTEGER NOT NULL,
    deck TEXT NOT NULL,
    off_plan INTEGER NOT NULL DEFAULT 0
);

PRAGMA user_version = 1;
"#,
    r#"
ALTER TABLE set_entry ADD COLUMN grid_offset_ms INTEGER NOT NULL DEFAULT 0;
ALTER TABLE set_entry ADD COLUMN downbeat_shift INTEGER NOT NULL DEFAULT 0;

PRAGMA user_version = 2;
"#,
    r#"
CREATE TABLE track_grid (
    track_path TEXT PRIMARY KEY,
    offset_ms INTEGER NOT NULL DEFAULT 0,
    downbeat_shift INTEGER NOT NULL DEFAULT 0,
    confirmed INTEGER NOT NULL DEFAULT 0
);

INSERT OR REPLACE INTO track_grid (track_path, offset_ms, downbeat_shift, confirmed)
    SELECT entry.track_path, entry.grid_offset_ms, entry.downbeat_shift, entry.grid_confirmed
    FROM set_entry AS entry JOIN sets ON sets.id = entry.set_id
    WHERE entry.grid_offset_ms != 0 OR entry.downbeat_shift != 0 OR entry.grid_confirmed != 0
    ORDER BY sets.updated_at;

PRAGMA user_version = 3;
"#,
    r#"
CREATE TABLE track_cues (
    identity_kind TEXT NOT NULL,
    identity TEXT NOT NULL,
    track_path TEXT,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (identity_kind, identity)
);

CREATE TABLE track_cue (
    identity_kind TEXT NOT NULL,
    identity TEXT NOT NULL,
    pad INTEGER CHECK (pad BETWEEN 0 AND 7),
    start_ms INTEGER NOT NULL CHECK (start_ms >= 0),
    loop_end_ms INTEGER CHECK (loop_end_ms > start_ms),
    color_table_index INTEGER,
    memory_color INTEGER,
    name TEXT NOT NULL DEFAULT '',
    FOREIGN KEY (identity_kind, identity) REFERENCES track_cues (identity_kind, identity)
        ON DELETE CASCADE ON UPDATE CASCADE
);

CREATE UNIQUE INDEX track_cue_pad ON track_cue (identity_kind, identity, pad)
    WHERE pad IS NOT NULL;
CREATE INDEX track_cue_track ON track_cue (identity_kind, identity);

PRAGMA user_version = 4;
"#,
    r#"
CREATE TABLE play_history_v4 (
    id INTEGER PRIMARY KEY,
    set_id INTEGER REFERENCES sets(id) ON DELETE SET NULL,
    track_path TEXT NOT NULL,
    rekordbox_id TEXT,
    title TEXT NOT NULL DEFAULT '',
    artist TEXT NOT NULL DEFAULT '',
    started_at INTEGER NOT NULL,
    seconds_played REAL NOT NULL DEFAULT 0,
    deck TEXT NOT NULL
);

INSERT INTO play_history_v4 (set_id, track_path, rekordbox_id, started_at, deck)
SELECT
    CASE WHEN off_plan OR set_id NOT IN (SELECT id FROM sets) THEN NULL ELSE set_id END,
    track_path,
    rekordbox_id,
    played_at,
    deck
FROM play_history
ORDER BY played_at;

DROP TABLE play_history;
ALTER TABLE play_history_v4 RENAME TO play_history;
CREATE INDEX play_history_started_at ON play_history (started_at);

PRAGMA user_version = 5;
"#,
];

/// A position no real entry can hold while one is in the middle of moving.
const MOVE_SENTINEL_POSITION: i64 = 1 << 62;

/// How many pre-migration backups to keep in the `backups` folder.
const MAX_BACKUPS: usize = 7;

/// A path component that marks a folder as belonging to rekordbox.
const REKORDBOX_FOLDER_NAMES: &[&str] = &["Pioneer", "rekordbox"];

/// Whether any component of `path` is a rekordbox folder, as every
/// Dubplate-owned file is refused inside one.
pub(crate) fn is_inside_rekordbox(path: &Path) -> bool {
    path.components().any(|component| {
        matches!(
            component,
            std::path::Component::Normal(name)
                if REKORDBOX_FOLDER_NAMES
                    .iter()
                    .any(|known| name.eq_ignore_ascii_case(known))
        )
    })
}

/// Errors opening or using Dubplate's own set database.
#[derive(Debug, Error)]
pub enum StoreError {
    /// The path is inside a rekordbox folder; Dubplate's writes stay out of it.
    #[error("path {0} is inside a rekordbox folder; Dubplate's database must live elsewhere")]
    InsideRekordbox(PathBuf),
    /// The database's `user_version` is newer than the migrations this version knows.
    #[error(
        "database schema is newer than this build: user_version {found}, this build knows up to {known}"
    )]
    NewerSchema {
        /// The `user_version` the database carries.
        found: u32,
        /// The highest version this build can migrate to.
        known: u32,
    },
    /// A set the operation referred to is not in the database.
    #[error("set {0} does not exist")]
    SetNotFound(SetId),
    /// An entry position the operation referred to is not in the set.
    #[error("position {0} is not in the set")]
    PositionNotFound(usize),
    /// A play the operation referred to is not in the history.
    #[error("play {0} is not in the history")]
    PlayNotFound(PlayId),
    /// A SQLite operation failed.
    #[error("set store error: {0}")]
    Database(#[from] rusqlite::Error),
    /// The snapshot's JSON could not be written or read back.
    #[error("snapshot json error: {0}")]
    Json(#[from] serde_json::Error),
    /// A folder or file could not be created.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Dubplate's own database of sets, seam plans, snapshots, play history and track cues.
///
/// One `SetStore` owns one SQLite file in WAL mode. Every mutating method runs
/// in a transaction and refreshes the set's `updated_at`.
pub struct SetStore {
    connection: Connection,
    database_path: PathBuf,
}

/// Room for every statement the store runs, so none is compiled again because another pushed
/// it out of the cache.
const STATEMENT_CACHE_CAPACITY: usize = 64;

/// The folder next to the database that keeps pre-migration backups.
const BACKUPS_FOLDER_NAME: &str = "backups";

impl SetStore {
    /// Opens the database at `path`, creating parent folders and the file if
    /// needed, and migrating it to the latest known schema.
    ///
    /// Refuses with `StoreError::InsideRekordbox` when any component of the
    /// path is a rekordbox folder, and with `StoreError::NewerSchema` when the
    /// file's `user_version` is newer than this build knows.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if is_inside_rekordbox(path) {
            return Err(StoreError::InsideRekordbox(path.to_path_buf()));
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let existing = path.exists();
        let mut connection = Connection::open(path)?;
        connection.pragma_update(None, "journal_mode", "wal")?;
        connection.set_prepared_statement_cache_capacity(STATEMENT_CACHE_CAPACITY);

        let user_version: i64 =
            connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        let user_version = u32::try_from(user_version).map_err(|_| StoreError::NewerSchema {
            found: u32::MAX,
            known: SCHEMA_VERSION,
        })?;
        if user_version > SCHEMA_VERSION {
            return Err(StoreError::NewerSchema {
                found: user_version,
                known: SCHEMA_VERSION,
            });
        }

        let migrations_pending = user_version < SCHEMA_VERSION;
        if existing && migrations_pending {
            let file_name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "database".to_owned());
            let timestamp = now_unix_millis();
            let backup_name = format!("v{user_version}-{timestamp}-{file_name}");
            let backups_folder = path
                .parent()
                .map(|parent| parent.join(BACKUPS_FOLDER_NAME))
                .unwrap_or_default();
            fs::create_dir_all(&backups_folder)?;
            connection.backup(
                rusqlite::MAIN_DB,
                backups_folder.join(backup_name),
                Some(|_progress: rusqlite::backup::Progress| ()),
            )?;
            trim_backups(&backups_folder, MAX_BACKUPS)?;
        }

        for (index, migration) in MIGRATIONS.iter().enumerate() {
            let target_version = u32::try_from(index).unwrap_or(u32::MAX).saturating_add(1);
            if target_version > user_version {
                let transaction = connection.transaction()?;
                transaction.execute_batch(migration)?;
                transaction.commit()?;
            }
        }

        Ok(Self {
            connection,
            database_path: path.to_path_buf(),
        })
    }

    /// The path of the file this store is open on.
    pub fn path(&self) -> &Path {
        &self.database_path
    }

    /// Creates a new empty set and returns its id.
    pub fn create_set(&mut self, name: &str, target_minutes: u32) -> Result<SetId, StoreError> {
        let transaction = self.transaction()?;
        let timestamp = now_unix_millis();
        execute_cached(
            &transaction,
            "INSERT INTO sets (name, target_minutes, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
            (name, target_minutes as i64, timestamp),
        )?;
        let id = transaction.last_insert_rowid();
        transaction.commit()?;
        Ok(SetId(id))
    }

    /// Renames a set.
    pub fn rename_set(&mut self, set: SetId, name: &str) -> Result<(), StoreError> {
        let mut transaction = self.transaction()?;
        update_set_name(&mut transaction, set, name)?;
        transaction.commit()?;
        Ok(())
    }

    /// Deletes a set and everything that belongs to it: entries, seams and
    /// snapshots. Its plays stay in the history, no longer tied to a set.
    pub fn delete_set(&mut self, set: SetId) -> Result<(), StoreError> {
        let transaction = self.transaction()?;
        let deleted = execute_cached(
            &transaction,
            "DELETE FROM sets WHERE id = ?1",
            (set.as_i64(),),
        )?;
        if deleted == 0 {
            return Err(StoreError::SetNotFound(set));
        }
        transaction.commit()?;
        Ok(())
    }

    /// Lists the sets: their id, name and track count.
    pub fn list_sets(&self) -> Result<Vec<(SetId, String, usize)>, StoreError> {
        let mut statement = self.connection.prepare_cached(
            "SELECT sets.id, sets.name,
                (SELECT COUNT(*) FROM set_entry WHERE set_entry.set_id = sets.id)
             FROM sets
             ORDER BY sets.id",
        )?;
        let rows = statement
            .query_map([], |row| {
                let id: i64 = row.get(0)?;
                let name: String = row.get(1)?;
                let track_count: i64 = row.get(2)?;
                Ok((SetId(id), name, usize::try_from(track_count).unwrap_or(0)))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Loads a whole set: its metadata, ordered entries and every stored seam.
    pub fn load_set(&self, set: SetId) -> Result<Set, StoreError> {
        let (name, target_minutes, created_at, updated_at) = query_row_cached(
            &self.connection,
            "SELECT name, target_minutes, created_at, updated_at FROM sets WHERE id = ?1",
            (set.as_i64(),),
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .optional()?
        .ok_or(StoreError::SetNotFound(set))?;
        let target_minutes = u32::try_from(target_minutes).unwrap_or(0);

        let mut entries_statement = self.connection.prepare_cached(
            "SELECT entry.track_path, entry.rekordbox_id, entry.mix_in_ms, entry.mix_out_ms,
                entry.cue_source, COALESCE(grid.confirmed, 0), entry.energy_override,
                entry.vocal_in_ms, entry.vocal_out_ms, entry.note, entry.is_backup,
                COALESCE(grid.offset_ms, 0), COALESCE(grid.downbeat_shift, 0)
             FROM set_entry AS entry
             LEFT JOIN track_grid AS grid ON grid.track_path = entry.track_path
             WHERE entry.set_id = ?1
             ORDER BY entry.is_backup, entry.position",
        )?;
        let entries = entries_statement
            .query_map((set.as_i64(),), entry_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        let mut seams_statement = self.connection.prepare_cached(
            "SELECT from_track_path, to_track_path, overlap_bars, eq_intent, status, note
             FROM seam WHERE set_id = ?1
             ORDER BY from_track_path, to_track_path",
        )?;
        let seams = seams_statement
            .query_map((set.as_i64(),), seam_from_row)?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Set {
            id: set,
            name,
            target_minutes,
            created_at,
            updated_at,
            entries,
            seams,
        })
    }

    /// Replaces a set's metadata, entries and seams atomically, returning the saved set.
    ///
    /// A failed write leaves the previous set intact. Revisions and plays remain
    /// attached to the same set; its creation time does not change.
    pub fn replace_set(&mut self, set: &Set) -> Result<Set, StoreError> {
        let mut transaction = self.transaction()?;
        let updated_at = now_unix_millis();
        let changed = execute_cached(
            &transaction,
            "UPDATE sets SET name = ?2, target_minutes = ?3, updated_at = ?4 WHERE id = ?1",
            (set.id.as_i64(), &set.name, set.target_minutes, updated_at),
        )?;
        if changed == 0 {
            return Err(StoreError::SetNotFound(set.id));
        }
        execute_cached(
            &transaction,
            "DELETE FROM set_entry WHERE set_id = ?1",
            (set.id.as_i64(),),
        )?;
        execute_cached(
            &transaction,
            "DELETE FROM seam WHERE set_id = ?1",
            (set.id.as_i64(),),
        )?;
        for list in [SetList::Main, SetList::Backups] {
            for (position, entry) in set
                .entries
                .iter()
                .filter(|entry| entry.is_backup == list.is_backup())
                .enumerate()
            {
                insert_entry_row(&mut transaction, set.id, list, position, entry)?;
                upsert_track_grid(&mut transaction, &entry.track_path, TrackGrid::of(entry))?;
            }
        }
        for seam in &set.seams {
            set_seam_row(&mut transaction, set.id, seam)?;
        }
        let created_at = query_row_cached(
            &transaction,
            "SELECT created_at FROM sets WHERE id = ?1",
            (set.id.as_i64(),),
            |row| row.get(0),
        )?;
        transaction.commit()?;
        Ok(Set {
            updated_at,
            created_at,
            ..set.clone()
        })
    }

    /// Inserts an entry into the list's order at `position`, shifting every
    /// later position of that list up by one. An entry that carries a grid fix
    /// keeps it for its track; one without leaves the track's fix as it was.
    pub fn insert_entry(
        &mut self,
        set: SetId,
        list: SetList,
        position: usize,
        entry: SetEntry,
    ) -> Result<(), StoreError> {
        let mut transaction = self.transaction()?;
        touch_set(&mut transaction, set)?;
        shift_entry_positions(&mut transaction, set, list, position, usize::MAX, 1)?;
        insert_entry_row(&mut transaction, set, list, position, &entry)?;
        let grid = TrackGrid::of(&entry);
        if grid != TrackGrid::default() {
            upsert_track_grid(&mut transaction, &entry.track_path, grid)?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Moves the entry at `from` to `to` within its list, shifting the
    /// positions in between of that list.
    pub fn move_entry(
        &mut self,
        set: SetId,
        list: SetList,
        from: usize,
        to: usize,
    ) -> Result<(), StoreError> {
        let mut transaction = self.transaction()?;
        if entry_at(&transaction, set, list, from)? == 0 {
            return Err(StoreError::PositionNotFound(from));
        }
        touch_set(&mut transaction, set)?;

        execute_cached(
            &transaction,
            "UPDATE set_entry SET position = ?4 WHERE set_id = ?1 AND is_backup = ?3 AND position = ?2",
            (
                set.as_i64(),
                from as i64,
                list.is_backup(),
                MOVE_SENTINEL_POSITION,
            ),
        )?;
        if from < to {
            shift_entry_positions(&mut transaction, set, list, from + 1, to + 1, -1)?;
        } else {
            shift_entry_positions(&mut transaction, set, list, to, from, 1)?;
        }
        execute_cached(
            &transaction,
            "UPDATE set_entry SET position = ?4 WHERE set_id = ?1 AND is_backup = ?3 AND position = ?2",
            (
                set.as_i64(),
                MOVE_SENTINEL_POSITION,
                list.is_backup(),
                to as i64,
            ),
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Removes the entry at `position` in the list, shifting every later
    /// position of that list down by one.
    pub fn remove_entry(
        &mut self,
        set: SetId,
        list: SetList,
        position: usize,
    ) -> Result<(), StoreError> {
        let mut transaction = self.transaction()?;
        if entry_at(&transaction, set, list, position)? == 0 {
            return Err(StoreError::PositionNotFound(position));
        }
        touch_set(&mut transaction, set)?;
        execute_cached(
            &transaction,
            "DELETE FROM set_entry WHERE set_id = ?1 AND is_backup = ?2 AND position = ?3",
            (set.as_i64(), list.is_backup(), position as i64),
        )?;
        shift_entry_positions(&mut transaction, set, list, position + 1, usize::MAX, -1)?;
        transaction.commit()?;
        Ok(())
    }

    /// Replaces the entry at `position` in the list with a new one.
    pub fn update_entry(
        &mut self,
        set: SetId,
        list: SetList,
        position: usize,
        entry: SetEntry,
    ) -> Result<(), StoreError> {
        let mut transaction = self.transaction()?;
        if entry_at(&transaction, set, list, position)? == 0 {
            return Err(StoreError::PositionNotFound(position));
        }
        touch_set(&mut transaction, set)?;
        execute_cached(
            &transaction,
            "UPDATE set_entry
             SET track_path = ?3, rekordbox_id = ?4, mix_in_ms = ?5, mix_out_ms = ?6, cue_source = ?7,
                 grid_confirmed = ?8, energy_override = ?9, vocal_in_ms = ?10,
                 vocal_out_ms = ?11, note = ?12, grid_offset_ms = ?14, downbeat_shift = ?15
             WHERE set_id = ?1 AND is_backup = ?2 AND position = ?13",
            (
                set.as_i64(),
                list.is_backup(),
                &entry.track_path.to_string_lossy(),
                &entry.rekordbox_id,
                entry.mix_in_ms,
                entry.mix_out_ms,
                entry
                    .cue_source
                    .map(|source| serde_json::to_string(&source).unwrap_or_default()),
                entry.grid_confirmed,
                entry.energy_override,
                entry.vocal_in_ms,
                entry.vocal_out_ms,
                &entry.note,
                position as i64,
                entry.grid_offset_ms,
                entry.downbeat_shift,
            ),
        )?;
        upsert_track_grid(&mut transaction, &entry.track_path, TrackGrid::of(&entry))?;
        transaction.commit()?;
        Ok(())
    }

    /// Every track's grid fix, by its file: the one place a track's grid correction and its
    /// confirmation are kept, whatever set or deck changed them.
    pub fn track_grids(&self) -> Result<HashMap<PathBuf, TrackGrid>, StoreError> {
        let mut statement = self.connection.prepare_cached(
            "SELECT track_path, offset_ms, downbeat_shift, confirmed FROM track_grid",
        )?;
        let grids = statement
            .query_map([], |row| {
                let confirmed: i64 = row.get(3)?;
                Ok((
                    PathBuf::from(row.get::<_, String>(0)?),
                    TrackGrid {
                        correction: GridCorrection {
                            offset_ms: row.get(1)?,
                            downbeat_shift: row.get(2)?,
                        },
                        confirmed: confirmed != 0,
                    },
                ))
            })?
            .collect::<Result<HashMap<_, _>, _>>()?;
        Ok(grids)
    }

    /// Keeps `grid` as the grid fix of the track whose file is `path`.
    pub fn set_track_grid(&mut self, path: &Path, grid: TrackGrid) -> Result<(), StoreError> {
        let mut transaction = self.transaction()?;
        upsert_track_grid(&mut transaction, path, grid)?;
        transaction.commit()?;
        Ok(())
    }

    /// Creates or replaces the seam plan between `Seam::from_track_path` and
    /// `Seam::to_track_path`.
    pub fn set_seam(&mut self, set: SetId, seam: Seam) -> Result<(), StoreError> {
        let mut transaction = self.transaction()?;
        touch_set(&mut transaction, set)?;
        set_seam_row(&mut transaction, set, &seam)?;
        transaction.commit()?;
        Ok(())
    }

    /// Saves the whole set as JSON under a reason and returns the revision id.
    pub fn snapshot(&mut self, set: SetId, reason: &str) -> Result<i64, StoreError> {
        let loaded = self.load_set(set)?;
        let json = serde_json::to_string(&loaded)?;
        let mut transaction = self.transaction()?;
        touch_set(&mut transaction, set)?;
        execute_cached(
            &transaction,
            "INSERT INTO set_revision (set_id, saved_at, reason, json) VALUES (?1, ?2, ?3, ?4)",
            (set.as_i64(), now_unix_millis(), reason, json),
        )?;
        let revision_id = transaction.last_insert_rowid();
        transaction.commit()?;
        Ok(revision_id)
    }

    fn transaction(&mut self) -> Result<Transaction<'_>, StoreError> {
        Ok(self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?)
    }
}

/// The current time as Unix milliseconds.
fn now_unix_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

/// Keeps only the newest `max_backups` files in the backups folder, deleting
/// the oldest by modification time.
fn trim_backups(backups_folder: &Path, max_backups: usize) -> Result<(), StoreError> {
    let mut backups: Vec<(PathBuf, std::time::SystemTime)> = fs::read_dir(backups_folder)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((entry.path(), modified))
        })
        .collect();
    backups.sort_by_key(|(_, modified)| *modified);
    while backups.len() > max_backups {
        let (oldest, _) = backups.remove(0);
        fs::remove_file(oldest)?;
    }
    Ok(())
}

/// Refreshes a set's `updated_at`, failing if the set is gone.
fn touch_set(transaction: &mut Transaction<'_>, set: SetId) -> Result<(), StoreError> {
    let deleted = execute_cached(
        transaction,
        "UPDATE sets SET updated_at = ?2 WHERE id = ?1",
        (set.as_i64(), now_unix_millis()),
    )?;
    if deleted == 0 {
        return Err(StoreError::SetNotFound(set));
    }
    Ok(())
}

/// Changes a set's name, failing if the set is gone.
fn update_set_name(
    transaction: &mut Transaction<'_>,
    set: SetId,
    name: &str,
) -> Result<(), StoreError> {
    let updated = execute_cached(
        transaction,
        "UPDATE sets SET name = ?2, updated_at = ?3 WHERE id = ?1",
        (set.as_i64(), name, now_unix_millis()),
    )?;
    if updated == 0 {
        return Err(StoreError::SetNotFound(set));
    }
    Ok(())
}

/// Whether a set's list has an entry at a position.
fn entry_at(
    transaction: &Transaction<'_>,
    set: SetId,
    list: SetList,
    position: usize,
) -> Result<usize, StoreError> {
    let count: i64 = query_row_cached(
        transaction,
        "SELECT COUNT(*) FROM set_entry
         WHERE set_id = ?1 AND is_backup = ?2 AND position = ?3",
        (set.as_i64(), list.is_backup(), position as i64),
        |row| row.get(0),
    )?;
    Ok(usize::try_from(count).unwrap_or(0))
}

/// Shifts every entry position in `[start, end)` of the list by `delta`, where
/// `delta` is positive for making room and negative for closing a gap.
fn shift_entry_positions(
    transaction: &mut Transaction<'_>,
    set: SetId,
    list: SetList,
    start: usize,
    end: usize,
    delta: i64,
) -> Result<(), StoreError> {
    // An open end is bound with a value no real position can hold.
    let end = match end {
        usize::MAX => i64::MAX,
        bounded => i64::try_from(bounded).unwrap_or(i64::MAX),
    };
    // SQLite checks a unique index row by row during an UPDATE, so shifting in place collides
    // with the neighbour not yet moved. The rows go to negative positions first, where no
    // entry sits, then back.
    execute_cached(
        transaction,
        "UPDATE set_entry SET position = -(position + ?5) - 1
         WHERE set_id = ?1 AND is_backup = ?2 AND position >= ?3 AND position < ?4",
        (set.as_i64(), list.is_backup(), start as i64, end, delta),
    )?;
    execute_cached(
        transaction,
        "UPDATE set_entry SET position = -position - 1
         WHERE set_id = ?1 AND is_backup = ?2 AND position < 0",
        (set.as_i64(), list.is_backup()),
    )?;
    Ok(())
}

/// Writes a track's grid fix within the caller's transaction.
fn upsert_track_grid(
    transaction: &mut Transaction<'_>,
    path: &Path,
    grid: TrackGrid,
) -> Result<(), StoreError> {
    execute_cached(
        transaction,
        "INSERT INTO track_grid (track_path, offset_ms, downbeat_shift, confirmed)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (track_path) DO UPDATE SET
             offset_ms = excluded.offset_ms,
             downbeat_shift = excluded.downbeat_shift,
             confirmed = excluded.confirmed",
        (
            &path.to_string_lossy(),
            grid.correction.offset_ms,
            grid.correction.downbeat_shift,
            grid.confirmed,
        ),
    )?;
    Ok(())
}

/// Inserts one entry row into the list at a position, taking its backup flag
/// from the list.
fn insert_entry_row(
    transaction: &mut Transaction<'_>,
    set: SetId,
    list: SetList,
    position: usize,
    entry: &SetEntry,
) -> Result<(), StoreError> {
    execute_cached(
        transaction,
        "INSERT INTO set_entry
            (set_id, position, track_path, rekordbox_id, mix_in_ms, mix_out_ms, cue_source,
             grid_confirmed, energy_override, vocal_in_ms, vocal_out_ms, note, is_backup,
             grid_offset_ms, downbeat_shift)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        (
            set.as_i64(),
            position as i64,
            &entry.track_path.to_string_lossy(),
            &entry.rekordbox_id,
            entry.mix_in_ms,
            entry.mix_out_ms,
            entry
                .cue_source
                .map(|source| serde_json::to_string(&source).unwrap_or_default()),
            entry.grid_confirmed,
            entry.energy_override,
            entry.vocal_in_ms,
            entry.vocal_out_ms,
            &entry.note,
            list.is_backup(),
            entry.grid_offset_ms,
            entry.downbeat_shift,
        ),
    )?;
    Ok(())
}

/// Inserts or updates a seam within the caller's transaction.
fn set_seam_row(
    transaction: &mut Transaction<'_>,
    set: SetId,
    seam: &Seam,
) -> Result<(), StoreError> {
    execute_cached(
            transaction,
        "INSERT INTO seam (set_id, from_track_path, to_track_path, overlap_bars, eq_intent, status, note)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT (set_id, from_track_path, to_track_path) DO UPDATE SET
             overlap_bars = excluded.overlap_bars,
             eq_intent = excluded.eq_intent,
             status = excluded.status,
             note = excluded.note",
        (
            set.as_i64(),
            &seam.from_track_path.to_string_lossy(),
            &seam.to_track_path.to_string_lossy(),
            seam.overlap_bars,
            serde_json::to_string(&seam.eq_intent)?,
            serde_json::to_string(&seam.status)?,
            &seam.note,
        ),
    )?;
    Ok(())
}

/// Reads a set_entry row into its type.
fn entry_from_row(row: &rusqlite::Row<'_>) -> Result<SetEntry, rusqlite::Error> {
    let grid_confirmed: i64 = row.get(5)?;
    let is_backup: i64 = row.get(10)?;
    Ok(SetEntry {
        track_path: PathBuf::from(row.get::<_, String>(0)?),
        rekordbox_id: row.get(1)?,
        mix_in_ms: row
            .get::<_, Option<i64>>(2)?
            .and_then(|value| u32::try_from(value).ok()),
        mix_out_ms: row
            .get::<_, Option<i64>>(3)?
            .and_then(|value| u32::try_from(value).ok()),
        cue_source: row
            .get::<_, Option<String>>(4)?
            .and_then(|raw| serde_json::from_str(&raw).ok()),
        grid_confirmed: grid_confirmed != 0,
        energy_override: row.get(6)?,
        vocal_in_ms: row
            .get::<_, Option<i64>>(7)?
            .and_then(|value| u32::try_from(value).ok()),
        vocal_out_ms: row
            .get::<_, Option<i64>>(8)?
            .and_then(|value| u32::try_from(value).ok()),
        note: row.get(9)?,
        is_backup: is_backup != 0,
        grid_offset_ms: row.get(11)?,
        downbeat_shift: row.get(12)?,
    })
}

/// Reads a seam row into its type.
fn seam_from_row(row: &rusqlite::Row<'_>) -> Result<Seam, rusqlite::Error> {
    let overlap_bars: i64 = row.get(2)?;
    Ok(Seam {
        from_track_path: PathBuf::from(row.get::<_, String>(0)?),
        to_track_path: PathBuf::from(row.get::<_, String>(1)?),
        overlap_bars: u32::try_from(overlap_bars).unwrap_or(0),
        eq_intent: serde_json::from_str(&row.get::<_, String>(3)?).unwrap_or(EqIntent::None),
        status: serde_json::from_str(&row.get::<_, String>(4)?).unwrap_or(SeamStatus::Planned),
        note: row.get(5)?,
    })
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacing_a_set_rolls_back_metadata_and_entries_when_a_seam_fails() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = SetStore::open(&directory.path().join("sets.sqlite")).unwrap();
        let id = store.create_set("Original", 60).unwrap();
        store
            .insert_entry(
                id,
                SetList::Main,
                0,
                SetEntry {
                    track_path: PathBuf::from("/Music/a.mp3"),
                    ..SetEntry::default()
                },
            )
            .unwrap();
        let before = store.load_set(id).unwrap();
        let mut replacement = before.clone();
        replacement.name = "Replacement".to_owned();
        replacement.entries.clear();
        replacement.seams.push(Seam {
            from_track_path: PathBuf::from("/Music/a.mp3"),
            to_track_path: PathBuf::from("/Music/b.mp3"),
            overlap_bars: 16,
            eq_intent: EqIntent::None,
            status: SeamStatus::Planned,
            note: String::new(),
        });
        store
            .connection
            .execute_batch(
                "CREATE TRIGGER fail_seam BEFORE INSERT ON seam BEGIN
             SELECT RAISE(ABORT, 'injected seam failure'); END;",
            )
            .unwrap();
        assert!(store.replace_set(&replacement).is_err());
        assert_eq!(store.load_set(id).unwrap(), before);
    }

    #[test]
    fn refuses_paths_inside_a_rekordbox_folder() {
        let result = SetStore::open(Path::new("/home/dj/Pioneer/rekordbox/dubplate.db"));
        assert!(matches!(result, Err(StoreError::InsideRekordbox(_))));
    }
}
