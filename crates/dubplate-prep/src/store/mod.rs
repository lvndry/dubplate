//! Dubplate's own database: sets, their ordered entries, seam plans, snapshots
//! and play history.
//!
//! The file lives where Dubplate owns it, never inside rekordbox's folders, so
//! every write Dubplate makes stays out of rekordbox's files. A guard refuses
//! any path that is inside rekordbox's; migrations are tracked by `user_version`
//! and an existing file is backed up before any migration runs on it.

use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};
use thiserror::Error;

mod types;

pub use types::{CueSource, EqIntent, Seam, SeamStatus, Set, SetEntry, SetId};

/// The highest schema version `MIGRATIONS` can bring a database to.
const SCHEMA_VERSION: u32 = 1;

/// One migration per schema version, index = target version - 1.
const MIGRATIONS: &[&str] = &[r#"
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
    rekordbox_id TEXT NOT NULL,
    mix_in_ms INTEGER,
    mix_out_ms INTEGER,
    cue_source TEXT,
    grid_confirmed INTEGER NOT NULL DEFAULT 0,
    energy_override REAL,
    vocal_in_ms INTEGER,
    vocal_out_ms INTEGER,
    note TEXT NOT NULL DEFAULT '',
    is_backup INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (set_id, position, is_backup)
);

CREATE TABLE seam (
    set_id INTEGER NOT NULL REFERENCES sets(id) ON DELETE CASCADE,
    from_position INTEGER NOT NULL,
    overlap_bars INTEGER NOT NULL,
    eq_intent TEXT NOT NULL,
    status TEXT NOT NULL,
    note TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (set_id, from_position)
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
    rekordbox_id TEXT NOT NULL,
    played_at INTEGER NOT NULL,
    deck TEXT NOT NULL,
    off_plan INTEGER NOT NULL DEFAULT 0
);

PRAGMA user_version = 1;
"#];

/// A position no real entry can hold while one is in the middle of moving.
const MOVE_SENTINEL_POSITION: i64 = 1 << 62;

/// How many pre-migration backups to keep in the `backups` folder.
const MAX_BACKUPS: usize = 7;

/// A path component that marks a folder as belonging to rekordbox.
const REKORDBOX_FOLDER_NAMES: &[&str] = &["Pioneer", "rekordbox"];

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

/// Dubplate's own database of sets, seam plans, snapshots and play history.
///
/// One `SetStore` owns one SQLite file in WAL mode. Every mutating method runs
/// in a transaction and refreshes the set's `updated_at`.
pub struct SetStore {
    connection: Connection,
    database_path: PathBuf,
}

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
        if path.components().any(|component| {
            matches!(
                component,
                std::path::Component::Normal(name)
                    if REKORDBOX_FOLDER_NAMES
                        .iter()
                        .any(|known| name.eq_ignore_ascii_case(known))
            )
        }) {
            return Err(StoreError::InsideRekordbox(path.to_path_buf()));
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let existing = path.exists();
        let connection = Connection::open(path)?;
        connection.pragma_update(None, "journal_mode", "wal")?;

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
                connection.execute_batch(migration)?;
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
        transaction.execute(
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

    /// Deletes a set and everything that belongs to it: entries, seams,
    /// snapshots and play history.
    pub fn delete_set(&mut self, set: SetId) -> Result<(), StoreError> {
        let transaction = self.transaction()?;
        let deleted = transaction.execute("DELETE FROM sets WHERE id = ?1", (set.as_i64(),))?;
        if deleted == 0 {
            return Err(StoreError::SetNotFound(set));
        }
        transaction.commit()?;
        Ok(())
    }

    /// Lists the sets: their id, name and track count.
    pub fn list_sets(&self) -> Result<Vec<(SetId, String, usize)>, StoreError> {
        let mut statement = self.connection.prepare(
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

    /// Loads a whole set: its metadata, ordered entries and seams.
    pub fn load_set(&self, set: SetId) -> Result<Set, StoreError> {
        let (name, target_minutes, created_at, updated_at) = self
            .connection
            .query_row(
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

        let mut entries_statement = self.connection.prepare(
            "SELECT rekordbox_id, mix_in_ms, mix_out_ms, cue_source, grid_confirmed,
                energy_override, vocal_in_ms, vocal_out_ms, note, is_backup
             FROM set_entry WHERE set_id = ?1
             ORDER BY is_backup, position",
        )?;
        let entries = entries_statement
            .query_map((set.as_i64(),), entry_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        let mut seams_statement = self.connection.prepare(
            "SELECT from_position, overlap_bars, eq_intent, status, note
             FROM seam WHERE set_id = ?1
             ORDER BY from_position",
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

    /// Inserts an entry at `position`, shifting every later position up by one.
    pub fn insert_entry(
        &mut self,
        set: SetId,
        position: usize,
        entry: SetEntry,
    ) -> Result<(), StoreError> {
        let mut transaction = self.transaction()?;
        touch_set(&mut transaction, set)?;
        shift_entry_positions(&mut transaction, set, position, usize::MAX, 1)?;
        insert_entry_row(&mut transaction, set, position, &entry)?;
        transaction.commit()?;
        Ok(())
    }

    /// Moves the entry at `from` to `to`, shifting the positions in between.
    pub fn move_entry(&mut self, set: SetId, from: usize, to: usize) -> Result<(), StoreError> {
        let mut transaction = self.transaction()?;
        if entry_at(&transaction, set, from)? == 0 {
            return Err(StoreError::PositionNotFound(from));
        }
        touch_set(&mut transaction, set)?;

        transaction.execute(
            "UPDATE set_entry SET position = ?3 WHERE set_id = ?1 AND position = ?2",
            (set.as_i64(), from as i64, MOVE_SENTINEL_POSITION),
        )?;
        if from < to {
            shift_entry_positions(&mut transaction, set, from + 1, to + 1, -1)?;
        } else {
            shift_entry_positions(&mut transaction, set, to, from, 1)?;
        }
        transaction.execute(
            "UPDATE set_entry SET position = ?3 WHERE set_id = ?1 AND position = ?2",
            (set.as_i64(), MOVE_SENTINEL_POSITION, to as i64),
        )?;
        touch_set(&mut transaction, set)?;
        transaction.commit()?;
        Ok(())
    }

    /// Removes the entry at `position`, shifting every later position down by one.
    pub fn remove_entry(&mut self, set: SetId, position: usize) -> Result<(), StoreError> {
        let mut transaction = self.transaction()?;
        if entry_at(&transaction, set, position)? == 0 {
            return Err(StoreError::PositionNotFound(position));
        }
        transaction.execute(
            "DELETE FROM set_entry WHERE set_id = ?1 AND position = ?2",
            (set.as_i64(), position as i64),
        )?;
        shift_entry_positions(&mut transaction, set, position + 1, usize::MAX, -1)?;
        transaction.commit()?;
        Ok(())
    }

    /// Replaces the entry at `position` with a new one.
    pub fn update_entry(
        &mut self,
        set: SetId,
        position: usize,
        entry: SetEntry,
    ) -> Result<(), StoreError> {
        let transaction = self.transaction()?;
        if entry_at(&transaction, set, position)? == 0 {
            return Err(StoreError::PositionNotFound(position));
        }
        transaction.execute(
            "UPDATE set_entry
             SET rekordbox_id = ?3, mix_in_ms = ?4, mix_out_ms = ?5, cue_source = ?6,
                 grid_confirmed = ?7, energy_override = ?8, vocal_in_ms = ?9,
                 vocal_out_ms = ?10, note = ?11
             WHERE set_id = ?1 AND position = ?2",
            (
                set.as_i64(),
                position as i64,
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
            ),
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Creates or replaces the seam after the track at `Seam::from_position`.
    pub fn set_seam(&mut self, set: SetId, seam: Seam) -> Result<(), StoreError> {
        let mut transaction = self.transaction()?;
        touch_set(&mut transaction, set)?;
        transaction.execute(
            "INSERT INTO seam (set_id, from_position, overlap_bars, eq_intent, status, note)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (set_id, from_position) DO UPDATE SET
                 overlap_bars = excluded.overlap_bars,
                 eq_intent = excluded.eq_intent,
                 status = excluded.status,
                 note = excluded.note",
            (
                set.as_i64(),
                seam.from_position as i64,
                seam.overlap_bars,
                serde_json::to_string(&seam.eq_intent).unwrap_or_default(),
                serde_json::to_string(&seam.status).unwrap_or_default(),
                seam.note,
            ),
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Saves the whole set as JSON under a reason and returns the revision id.
    pub fn snapshot(&mut self, set: SetId, reason: &str) -> Result<i64, StoreError> {
        let loaded = self.load_set(set)?;
        let json = serde_json::to_string(&loaded)?;
        let mut transaction = self.transaction()?;
        touch_set(&mut transaction, set)?;
        transaction.execute(
            "INSERT INTO set_revision (set_id, saved_at, reason, json) VALUES (?1, ?2, ?3, ?4)",
            (set.as_i64(), now_unix_millis(), reason, json),
        )?;
        let revision_id = transaction.last_insert_rowid();
        transaction.commit()?;
        Ok(revision_id)
    }

    /// Records a track being played for a set, on a deck, planned or off plan.
    pub fn record_play(
        &mut self,
        set: SetId,
        rekordbox_id: &str,
        deck_label: &str,
        off_plan: bool,
    ) -> Result<(), StoreError> {
        let mut transaction = self.transaction()?;
        touch_set(&mut transaction, set)?;
        transaction.execute(
            "INSERT INTO play_history (set_id, rekordbox_id, played_at, deck, off_plan)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            (
                set.as_i64(),
                rekordbox_id,
                now_unix_millis(),
                deck_label,
                off_plan,
            ),
        )?;
        transaction.commit()?;
        Ok(())
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
    let deleted = transaction.execute(
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
    let updated = transaction.execute(
        "UPDATE sets SET name = ?2, updated_at = ?3 WHERE id = ?1",
        (set.as_i64(), name, now_unix_millis()),
    )?;
    if updated == 0 {
        return Err(StoreError::SetNotFound(set));
    }
    Ok(())
}

/// Whether a set has an entry at a position.
/// Whether a set has an entry at a position.
fn entry_at(
    transaction: &Transaction<'_>,
    set: SetId,
    position: usize,
) -> Result<usize, StoreError> {
    let count: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM set_entry WHERE set_id = ?1 AND position = ?2",
        (set.as_i64(), position as i64),
        |row| row.get(0),
    )?;
    Ok(usize::try_from(count).unwrap_or(0))
}

/// Shifts every entry position in `[start, end)` by `delta`, where
/// `delta` is positive for making room and negative for closing a gap.
fn shift_entry_positions(
    transaction: &mut Transaction<'_>,
    set: SetId,
    start: usize,
    end: usize,
    delta: i64,
) -> Result<(), StoreError> {
    transaction.execute(
        "UPDATE set_entry SET position = position + ?4
         WHERE set_id = ?1 AND position >= ?2 AND position < ?3",
        (set.as_i64(), start as i64, end as i64, delta),
    )?;
    Ok(())
}
/// Inserts one entry row at a position.
fn insert_entry_row(
    transaction: &mut Transaction<'_>,
    set: SetId,
    position: usize,
    entry: &SetEntry,
) -> Result<(), StoreError> {
    transaction.execute(
        "INSERT INTO set_entry
            (set_id, position, rekordbox_id, mix_in_ms, mix_out_ms, cue_source,
             grid_confirmed, energy_override, vocal_in_ms, vocal_out_ms, note, is_backup)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        (
            set.as_i64(),
            position as i64,
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
            entry.is_backup,
        ),
    )?;
    Ok(())
}

/// Reads a set_entry row into its type.
fn entry_from_row(row: &rusqlite::Row<'_>) -> Result<SetEntry, rusqlite::Error> {
    let grid_confirmed: i64 = row.get(4)?;
    let is_backup: i64 = row.get(9)?;
    Ok(SetEntry {
        rekordbox_id: row.get(0)?,
        mix_in_ms: row
            .get::<_, Option<i64>>(1)?
            .and_then(|value| u32::try_from(value).ok()),
        mix_out_ms: row
            .get::<_, Option<i64>>(2)?
            .and_then(|value| u32::try_from(value).ok()),
        cue_source: row
            .get::<_, Option<String>>(3)?
            .and_then(|raw| serde_json::from_str(&raw).ok()),
        grid_confirmed: grid_confirmed != 0,
        energy_override: row.get(5)?,
        vocal_in_ms: row
            .get::<_, Option<i64>>(6)?
            .and_then(|value| u32::try_from(value).ok()),
        vocal_out_ms: row
            .get::<_, Option<i64>>(7)?
            .and_then(|value| u32::try_from(value).ok()),
        note: row.get(8)?,
        is_backup: is_backup != 0,
    })
}

/// Reads a seam row into its type.
fn seam_from_row(row: &rusqlite::Row<'_>) -> Result<Seam, rusqlite::Error> {
    let overlap_bars: i64 = row.get(1)?;
    Ok(Seam {
        from_position: usize::try_from(row.get::<_, i64>(0)?).unwrap_or(0),
        overlap_bars: u32::try_from(overlap_bars).unwrap_or(0),
        eq_intent: serde_json::from_str(&row.get::<_, String>(2)?).unwrap_or(EqIntent::None),
        status: serde_json::from_str(&row.get::<_, String>(3)?).unwrap_or(SeamStatus::Planned),
        note: row.get(4)?,
    })
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_paths_inside_a_rekordbox_folder() {
        let result = SetStore::open(Path::new("/home/dj/Pioneer/rekordbox/dubplate.db"));
        assert!(matches!(result, Err(StoreError::InsideRekordbox(_))));
    }
}
