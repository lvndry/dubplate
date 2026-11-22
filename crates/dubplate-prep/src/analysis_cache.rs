//! Dubplate's own cache of analysed tracks.
//!
//! One SQLite file, kept where Dubplate owns it and never inside rekordbox's folders. A row is
//! answered to a caller only while the file it was analysed from is unchanged in size and
//! modification time and the analysis version that made it is the one in use; otherwise the
//! row is stale.

use std::fs;
use std::path::{Path, PathBuf};

use euphoria_core::Key;
use rusqlite::{Connection, OptionalExtension, Row, params};
use thiserror::Error;

use crate::analysis::{AnalysedGrid, AudioAnalysis, analysis_version, millis_i64};
use crate::energy::TrackEnergy;
use crate::loudness::Lufs;


use crate::sql::{execute_cached, now_unix_millis, query_row_cached};
use crate::store::is_inside_rekordbox;

/// The highest schema version the cache can bring a database to.
const SCHEMA_VERSION: u32 = 2;

/// One migration per schema version, index = target version - 1.
const MIGRATIONS: &[&str] = &[
    r#"
CREATE TABLE analysis (
    track_path TEXT PRIMARY KEY,
    file_size INTEGER NOT NULL,
    modified_ms INTEGER NOT NULL,
    analysis_version TEXT NOT NULL,
    duration_seconds REAL NOT NULL,
    bpm REAL,
    grid_bpm REAL,
    grid_first_downbeat_seconds REAL,
    camelot_key TEXT,
    energy_rating INTEGER,
    energy_score REAL,
    energy_curve_json TEXT,
    analysed_at INTEGER NOT NULL
);

PRAGMA user_version = 1;
"#,
    r#"
ALTER TABLE analysis ADD COLUMN loudness_lufs REAL;

PRAGMA user_version = 2;
"#,
];

/// The columns a cached analysis is read back from, in the order [`analysis_columns_of_row`] reads them.
/// The cached analysis of a file, when it was made from the file as it is now by the analysis
/// version in use.
const SELECT_FRESH_ANALYSIS: &str = "SELECT duration_seconds, bpm, grid_bpm, \
    grid_first_downbeat_seconds, camelot_key, energy_rating, energy_score, energy_curve_json,
    loudness_lufs
    FROM analysis
    WHERE track_path = ?1 AND file_size = ?2 AND modified_ms = ?3 AND analysis_version = ?4";

/// Errors opening or using Dubplate's analysis cache.
#[derive(Debug, Error)]
pub enum AnalysisCacheError {
    /// The path is inside a rekordbox folder; Dubplate's cache must live elsewhere.
    #[error("path {0} is inside a rekordbox folder; Dubplate's analysis cache must live elsewhere")]
    InsideRekordbox(PathBuf),
    /// The database's `user_version` is newer than the schema this version knows.
    #[error(
        "analysis cache schema is newer than this build: user_version {found}, this build knows up to {known}"
    )]
    NewerSchema {
        /// The `user_version` the database carries.
        found: u32,
        /// The highest version this build can migrate to.
        known: u32,
    },
    /// A SQLite operation failed.
    #[error("analysis cache error: {0}")]
    Database(#[from] rusqlite::Error),
    /// A cached energy curve could not be written to or read from JSON.
    #[error("analysis cache json error: {0}")]
    Json(#[from] serde_json::Error),
    /// A folder or file could not be created.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Dubplate's own cache of analysed tracks, one SQLite file in WAL mode.
pub struct AnalysisCache {
    connection: Connection,
    version: String,
}

impl AnalysisCache {
    /// Opens the cache at `path`, creating parent folders and the file if needed and migrating
    /// it to the latest known schema.
    ///
    /// Refuses with `AnalysisCacheError::InsideRekordbox` when any component of the path is a
    /// rekordbox folder, and with `AnalysisCacheError::NewerSchema` when the file's
    /// `user_version` is newer than this build knows.
    pub fn open(path: &Path) -> Result<Self, AnalysisCacheError> {
        if is_inside_rekordbox(path) {
            return Err(AnalysisCacheError::InsideRekordbox(path.to_path_buf()));
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let connection = Connection::open(path)?;
        connection.pragma_update(None, "journal_mode", "wal")?;

        let user_version: i64 =
            connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        let user_version =
            u32::try_from(user_version).map_err(|_| AnalysisCacheError::NewerSchema {
                found: u32::MAX,
                known: SCHEMA_VERSION,
            })?;
        if user_version > SCHEMA_VERSION {
            return Err(AnalysisCacheError::NewerSchema {
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

        Ok(Self {
            connection,
            version: analysis_version(),
        })
    }

    /// The analysis of the file at `path`, when the cache holds one that is still valid: the
    /// file's current size and modification time match the ones it was analysed from, and the
    /// analysis version that made it is the one in use. Any mismatch is answered as no cache.
    pub fn get(&self, path: &Path) -> Result<Option<AudioAnalysis>, AnalysisCacheError> {
        let Some((file_size, modified_ms)) = file_fingerprint(path)? else {
            return Ok(None);
        };
        let row = query_row_cached(
            &self.connection,
            SELECT_FRESH_ANALYSIS,
            params![path.to_string_lossy(), file_size, modified_ms, self.version],
            analysis_columns_of_row,
        )
        .optional()?;
        row.map(analysis_of_columns).transpose()
    }

    /// Stores or replaces the analysis of the file at `path`, recording the file's current size
    /// and modification time so a later change of it shows as stale.
    pub fn put(&mut self, path: &Path, analysis: &AudioAnalysis) -> Result<(), AnalysisCacheError> {
        let Some((file_size, modified_ms)) = file_fingerprint(path)? else {
            return Ok(());
        };
        let energy = analysis.energy.as_ref();
        let curve_json = energy
            .map(|energy| serde_json::to_string(&energy.curve))
            .transpose()?;
        execute_cached(
            &self.connection,
            "INSERT INTO analysis (track_path, file_size, modified_ms, analysis_version,
                 duration_seconds, bpm, grid_bpm, grid_first_downbeat_seconds, camelot_key,
                 energy_rating, energy_score, energy_curve_json, loudness_lufs, analysed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
             ON CONFLICT (track_path) DO UPDATE SET
                 file_size = excluded.file_size,
                 modified_ms = excluded.modified_ms,
                 analysis_version = excluded.analysis_version,
                 duration_seconds = excluded.duration_seconds,
                 bpm = excluded.bpm,
                 grid_bpm = excluded.grid_bpm,
                 grid_first_downbeat_seconds = excluded.grid_first_downbeat_seconds,
                 camelot_key = excluded.camelot_key,
                 energy_rating = excluded.energy_rating,
                 energy_score = excluded.energy_score,
                 energy_curve_json = excluded.energy_curve_json,
                 loudness_lufs = excluded.loudness_lufs,
                 analysed_at = excluded.analysed_at",
            params![
                path.to_string_lossy(),
                file_size,
                modified_ms,
                self.version,
                f64::from(analysis.duration_seconds),
                analysis.bpm.map(f64::from),
                analysis.beat_grid.map(|grid| grid.bpm),
                analysis.beat_grid.map(|grid| grid.first_downbeat_seconds),
                analysis.key.map(|key| key.camelot()),
                energy.map(|energy| energy.rating),
                energy.map(|energy| f64::from(energy.score)),
                curve_json,
                analysis.loudness.map(|loudness| f64::from(loudness.0)),
                now_unix_millis(),
            ],
        )?;
        Ok(())
    }

    /// Forgets the file at `path`, so it is analysed again next time it is asked.
    pub fn forget(&mut self, path: &Path) -> Result<(), AnalysisCacheError> {
        execute_cached(
            &self.connection,
            "DELETE FROM analysis WHERE track_path = ?1",
            params![path.to_string_lossy()],
        )?;
        Ok(())
    }
}

/// A cached row's columns, as SQLite holds them.
struct AnalysisColumns {
    duration_seconds: f64,
    bpm: Option<f64>,
    grid_bpm: Option<f64>,
    grid_first_downbeat_seconds: Option<f64>,
    camelot_key: Option<String>,
    energy_rating: Option<u8>,
    energy_score: Option<f64>,
    energy_curve_json: Option<String>,
    loudness_lufs: Option<f64>,
}

fn analysis_columns_of_row(row: &Row<'_>) -> rusqlite::Result<AnalysisColumns> {
    Ok(AnalysisColumns {
        duration_seconds: row.get(0)?,
        bpm: row.get(1)?,
        grid_bpm: row.get(2)?,
        grid_first_downbeat_seconds: row.get(3)?,
        camelot_key: row.get(4)?,
        energy_rating: row.get(5)?,
        energy_score: row.get(6)?,
        energy_curve_json: row.get(7)?,
        loudness_lufs: row.get(8)?,
    })
}

fn analysis_of_columns(columns: AnalysisColumns) -> Result<AudioAnalysis, AnalysisCacheError> {
    let energy = match (
        columns.energy_rating,
        columns.energy_score,
        columns.energy_curve_json,
    ) {
        (Some(rating), Some(score), Some(curve_json)) => Some(TrackEnergy {
            rating,
            score: score as f32,
            curve: serde_json::from_str(&curve_json)?,
        }),
        _ => None,
    };
    let beat_grid = columns
        .grid_bpm
        .zip(columns.grid_first_downbeat_seconds)
        .map(|(bpm, first_downbeat_seconds)| AnalysedGrid {
            bpm,
            first_downbeat_seconds,
        });
    Ok(AudioAnalysis {
        duration_seconds: columns.duration_seconds as f32,
        bpm: columns.bpm.map(|bpm| bpm as f32),
        beat_grid,
        key: columns.camelot_key.as_deref().and_then(Key::parse_file_key),
        energy,
        loudness: columns.loudness_lufs.map(|lufs| Lufs(lufs as f32)),
    })
}


/// The file's current size and modification time in milliseconds, or None when it has no
/// readable metadata.
fn file_fingerprint(path: &Path) -> Result<Option<(i64, i64)>, AnalysisCacheError> {
    let Some(metadata) = fs::metadata(path).ok() else {
        return Ok(None);
    };
    let size = i64::try_from(metadata.len()).unwrap_or(i64::MAX);
    let modified = metadata
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)
        .map(millis_i64)
        .unwrap_or(0);