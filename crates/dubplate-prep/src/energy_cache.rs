//! Dubplate's own cache of rated track energy.
//!
//! One SQLite file, kept where Dubplate owns it and never inside rekordbox's
//! folders. A row is answered to a caller only while the file it was rated
//! from is unchanged in size and modification time and the Euphoria energy
//! model that rated it is the one in use; otherwise the row is stale.

use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};

use crate::energy::TrackEnergy;
use crate::store::is_inside_rekordbox;
use thiserror::Error;

/// The highest schema version the cache can bring a database to.
const SCHEMA_VERSION: u32 = 1;

/// One migration per schema version, index = target version - 1.
const MIGRATIONS: &[&str] = &[r#"
CREATE TABLE energy (
    track_path TEXT PRIMARY KEY,
    file_size INTEGER NOT NULL,
    modified_ms INTEGER NOT NULL,
    model_version INTEGER NOT NULL,
    rating INTEGER NOT NULL,
    score REAL NOT NULL,
    curve_json TEXT NOT NULL,
    analysed_at INTEGER NOT NULL
);

PRAGMA user_version = 1;
"#];

/// The current time as Unix milliseconds.
fn now_unix_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

/// Errors opening or using Dubplate's energy cache.
#[derive(Debug, Error)]
pub enum EnergyCacheError {
    /// The path is inside a rekordbox folder; Dubplate's cache must live elsewhere.
    #[error("path {0} is inside a rekordbox folder; Dubplate's energy cache must live elsewhere")]
    InsideRekordbox(PathBuf),
    /// The database's `user_version` is newer than the schema this version knows.
    #[error(
        "energy cache schema is newer than this build: user_version {found}, this build knows up to {known}"
    )]
    NewerSchema {
        /// The `user_version` the database carries.
        found: u32,
        /// The highest version this build can migrate to.
        known: u32,
    },
    /// A SQLite operation failed.
    #[error("energy cache error: {0}")]
    Database(#[from] rusqlite::Error),
    /// A cached curve could not be written to or read from JSON.
    #[error("energy cache json error: {0}")]
    Json(#[from] serde_json::Error),
    /// A folder or file could not be created.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Dubplate's own cache of rated track energy, one SQLite file in WAL mode.
pub struct EnergyCache {
    connection: Connection,
}

impl EnergyCache {
    /// Opens the cache at `path`, creating parent folders and the file if
    /// needed and migrating it to the latest known schema.
    ///
    /// Refuses with `EnergyCacheError::InsideRekordbox` when any component of
    /// the path is a rekordbox folder, and with `EnergyCacheError::NewerSchema`
    /// when the file's `user_version` is newer than this build knows.
    pub fn open(path: &Path) -> Result<Self, EnergyCacheError> {
        if is_inside_rekordbox(path) {
            return Err(EnergyCacheError::InsideRekordbox(path.to_path_buf()));
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let connection = Connection::open(path)?;
        connection.pragma_update(None, "journal_mode", "wal")?;

        let user_version: i64 =
            connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        let user_version =
            u32::try_from(user_version).map_err(|_| EnergyCacheError::NewerSchema {
                found: u32::MAX,
                known: SCHEMA_VERSION,
            })?;
        if user_version > SCHEMA_VERSION {
            return Err(EnergyCacheError::NewerSchema {
                found: user_version,
                known: SCHEMA_VERSION,
            });
        }

        for (index, migration) in MIGRATIONS.iter().enumerate() {
            let target_version = u32::try_from(index).unwrap_or(u32::MAX).saturating_add(1);
            if target_version > user_version {
                connection.execute_batch(migration)?;
            }
        }

        Ok(Self { connection })
    }

    /// The rating of the file at `path`, when the cache holds one that is
    /// still valid: the file's current size and modification time match the
    /// ones the row was rated from, and the model that rated it is the one in
    /// use. Any mismatch is answered as no cache.
    pub fn get(&self, path: &Path) -> Result<Option<TrackEnergy>, EnergyCacheError> {
        let Some((file_size, modified_ms)) = file_fingerprint(path)? else {
            return Ok(None);
        };

        let key = path.to_string_lossy();
        let row: Option<(i64, i64, i64, u8, f64, String)> = self
            .connection
            .query_row(
                "SELECT file_size, modified_ms, model_version, rating, score, curve_json
                 FROM energy WHERE track_path = ?1",
                params![key],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .optional()?;

        let Some(row) = row else {
            return Ok(None);
        };
        let (stored_size, stored_modified, stored_model, rating, score, curve_json) = row;
        if stored_size != file_size
            || stored_modified != modified_ms
            || stored_model != i64::from(euphoria_core::ENERGY_MODEL_VERSION)
        {
            return Ok(None);
        }

        let curve = serde_json::from_str(&curve_json)?;
        Ok(Some(TrackEnergy {
            rating,
            score: score as f32,
            curve,
        }))
    }

    /// Stores or replaces the rating of the file at `path`, recording the
    /// file's current size and modification time so a later change of it
    /// shows as stale.
    pub fn put(&mut self, path: &Path, energy: &TrackEnergy) -> Result<(), EnergyCacheError> {
        let Some((file_size, modified_ms)) = file_fingerprint(path)? else {
            return Ok(());
        };
        let curve_json = serde_json::to_string(&energy.curve)?;
        self.connection.execute(
            "INSERT INTO energy (track_path, file_size, modified_ms, model_version, rating, score, curve_json, analysed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT (track_path) DO UPDATE SET
                 file_size = excluded.file_size,
                 modified_ms = excluded.modified_ms,
                 model_version = excluded.model_version,
                 rating = excluded.rating,
                 score = excluded.score,
                 curve_json = excluded.curve_json,
                 analysed_at = excluded.analysed_at",
            params![
                path.to_string_lossy(),
                file_size,
                modified_ms,
                i64::from(euphoria_core::ENERGY_MODEL_VERSION),
                energy.rating,
                energy.score as f64,
                curve_json,
                now_unix_millis(),
            ],
        )?;
        Ok(())
    }

    /// Forgets the file at `path`, so it is rated again next time it is asked.
    pub fn forget(&mut self, path: &Path) -> Result<(), EnergyCacheError> {
        self.connection.execute(
            "DELETE FROM energy WHERE track_path = ?1",
            params![path.to_string_lossy()],
        )?;
        Ok(())
    }
}

/// The file's current size and modification time in milliseconds, or None
/// when it has no readable metadata.
fn file_fingerprint(path: &Path) -> Result<Option<(i64, i64)>, EnergyCacheError> {
    let Some(metadata) = fs::metadata(path).ok() else {
        return Ok(None);
    };
    let size = i64::try_from(metadata.len()).unwrap_or(i64::MAX);
    let modified = metadata
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0);
    Ok(Some((size, modified)))
}
