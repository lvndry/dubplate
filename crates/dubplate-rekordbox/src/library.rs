use std::fmt;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, ErrorCode, OptionalExtension, Row};
use serde::Serialize;
use tempfile::TempDir;

use crate::analysis::TrackAnalysis;
use crate::cue_colors::{MemoryCueColor, PaletteColor};
use crate::error::Error;
use crate::model::{Cue, CueKind, Playlist, PlaylistKind, Track};

/// The SQLCipher key of a rekordbox library. Its `Debug` output never shows the key.
#[derive(Clone)]
pub struct DatabaseKey(String);

impl DatabaseKey {
    /// Wraps a key read from the user's configuration.
    pub fn new(key: impl Into<String>) -> Self {
        Self(key.into().trim().to_owned())
    }

    pub(crate) fn secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for DatabaseKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DatabaseKey(..)")
    }
}

/// Counts that describe a library at a glance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LibrarySummary {
    /// The schema version rekordbox wrote in `djmdProperty`, such as `6000`.
    pub database_version: Option<String>,
    /// Tracks in the collection.
    pub tracks: u64,
    /// Playlists, folders and smart playlists.
    pub playlists: u64,
    /// Memory cues, hot cues and saved loops.
    pub cues: u64,
}

/// An unlocked copy of a rekordbox library.
///
/// The copy lives in a temporary directory that is removed when the `Library` is dropped.
pub struct Library {
    connection: Connection,
    share_directory: PathBuf,
    _copy_directory: TempDir,
}

const DATABASE_COMPANIONS: [&str; 2] = ["-wal", "-shm"];

impl Library {
    /// Copies `master_db` and its write-ahead log, if any, then unlocks the copy with `key`.
    pub fn open_copy(master_db: &Path, key: &DatabaseKey) -> Result<Self, Error> {
        if !master_db.is_file() {
            return Err(Error::NotFound(master_db.to_path_buf()));
        }
        let copy_directory = tempfile::tempdir().map_err(|source| Error::Copy {
            path: master_db.to_path_buf(),
            source,
        })?;
        let copy_path = copy_directory.path().join("master.db");
        copy_file(master_db, &copy_path)?;
        for suffix in DATABASE_COMPANIONS {
            let companion = with_suffix(master_db, suffix);
            if companion.is_file() {
                copy_file(&companion, &with_suffix(&copy_path, suffix))?;
            }
        }

        let connection = Connection::open(&copy_path)?;
        connection.pragma_update(None, "key", key.secret())?;
        match connection.query_row("SELECT count(*) FROM sqlite_master", [], |row| {
            row.get::<_, i64>(0)
        }) {
            Ok(_) => {}
            Err(rusqlite::Error::SqliteFailure(failure, _))
                if failure.code == ErrorCode::NotADatabase =>
            {
                return Err(Error::WrongKey);
            }
            Err(other) => return Err(other.into()),
        }
        let share_directory = master_db
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .join("share");
        Ok(Self {
            connection,
            share_directory,
            _copy_directory: copy_directory,
        })
    }

    /// rekordbox's `share` directory, next to `master.db`, which holds the analysis files.
    pub fn share_directory(&self) -> &Path {
        &self.share_directory
    }

    /// Reads the beat grid, cues, phrases and waveforms of `track` from its analysis files.
    ///
    /// The files are opened read-only, in place: nothing under rekordbox's directories is
    /// copied, written or locked.
    pub fn analysis(&self, track: &Track) -> Result<TrackAnalysis, Error> {
        let analysis_path = track
            .analysis_path
            .as_deref()
            .filter(|path| !path.is_empty())
            .ok_or_else(|| Error::NoAnalysis(track.id.clone()))?;
        TrackAnalysis::read(&self.share_directory, analysis_path)
    }

    /// Counts the tracks, playlists and cues, and reads the schema version.
    pub fn summary(&self) -> Result<LibrarySummary, Error> {
        let database_version = self
            .connection
            .query_row("SELECT DBVersion FROM djmdProperty LIMIT 1", [], |row| {
                row.get(0)
            })
            .optional()?
            .flatten();
        Ok(LibrarySummary {
            database_version,
            tracks: self.count_live("djmdContent")?,
            playlists: self.count_live("djmdPlaylist")?,
            cues: self.count_live("djmdCue")?,
        })
    }

    /// Reads every track in the collection, ordered by title.
    pub fn tracks(&self) -> Result<Vec<Track>, Error> {
        let mut statement = self.connection.prepare(
            "SELECT content.ID, content.Title, artist.Name, album.Name, genre.Name, label.Name,
                    musical_key.ScaleName, content.BPM, content.Length, content.FolderPath,
                    content.FileSize, content.Rating, content.Commnt, content.AnalysisDataPath,
                    content.StockDate,
                    (SELECT (mixer.GainHigh & 65535) << 16 | (mixer.GainLow & 65535)
                     FROM djmdMixerParam AS mixer
                     WHERE mixer.ContentID = content.ID AND mixer.rb_local_deleted = 0
                     ORDER BY mixer.updated_at DESC
                     LIMIT 1)
             FROM djmdContent AS content
             LEFT JOIN djmdArtist AS artist ON artist.ID = content.ArtistID
             LEFT JOIN djmdAlbum AS album ON album.ID = content.AlbumID
             LEFT JOIN djmdGenre AS genre ON genre.ID = content.GenreID
             LEFT JOIN djmdLabel AS label ON label.ID = content.LabelID
             LEFT JOIN djmdKey AS musical_key ON musical_key.ID = content.KeyID
             WHERE content.rb_local_deleted = 0
             ORDER BY content.Title COLLATE NOCASE, content.ID",
        )?;
        let tracks = statement
            .query_map([], track_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(tracks)
    }

    /// Reads every cue and saved loop, ordered by track and position.
    pub fn cues(&self) -> Result<Vec<Cue>, Error> {
        let mut statement = self.connection.prepare(
            "SELECT ID, ContentID, Kind, InMsec, OutMsec, Color, Comment, ColorTableIndex
             FROM djmdCue
             WHERE rb_local_deleted = 0
             ORDER BY ContentID, InMsec, ID",
        )?;
        let cues = statement
            .query_map([], cue_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(cues)
    }

    /// Reads the playlist tree in rekordbox's order, each playlist with its tracks in order.
    pub fn playlists(&self) -> Result<Vec<Playlist>, Error> {
        let mut playlist_statement = self.connection.prepare(
            "SELECT ID, ParentID, Name, Attribute, Seq
             FROM djmdPlaylist
             WHERE rb_local_deleted = 0
             ORDER BY ParentID, Seq, ID",
        )?;
        let mut shells = Vec::new();
        for shell in playlist_statement.query_map([], |row| {
            let parent_id: Option<String> = row.get(1)?;
            Ok(Playlist {
                id: row.get(0)?,
                parent_id: parent_id.filter(|parent| parent != "root"),
                name: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                kind: PlaylistKind::from_attribute(row.get::<_, Option<i64>>(3)?.unwrap_or(0)),
                position: row.get::<_, Option<i64>>(4)?.unwrap_or(0),
                track_ids: Vec::new(),
            })
        })? {
            shells.push(shell?);
        }
        // The entries of every live playlist in one query, instead of one per playlist on
        // every library read.
        let mut entry_statement = self.connection.prepare(
            "SELECT entry.ContentID, playlist.ID
             FROM djmdPlaylist AS playlist
             JOIN djmdSongPlaylist AS entry ON entry.PlaylistID = playlist.ID
             JOIN djmdContent AS content ON content.ID = entry.ContentID
             WHERE playlist.rb_local_deleted = 0
               AND entry.rb_local_deleted = 0
               AND content.rb_local_deleted = 0
             ORDER BY playlist.ID, entry.TrackNo, entry.ID",
        )?;
        let entries: Vec<(String, String)> = entry_statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<_, _>>()?;
        let mut tracks_by_playlist: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        for (content_id, playlist_id) in entries {
            tracks_by_playlist
                .entry(playlist_id)
                .or_default()
                .push(content_id);
        }
        for playlist in &mut shells {
            if playlist.kind == PlaylistKind::Playlist {
                playlist.track_ids = tracks_by_playlist.remove(&playlist.id).unwrap_or_default();
            }
        }
        Ok(shells)
    }

    fn count_live(&self, table: &str) -> Result<u64, Error> {
        let count: i64 = self.connection.query_row(
            &format!("SELECT count(*) FROM {table} WHERE rb_local_deleted = 0"),
            [],
            |row| row.get(0),
        )?;
        Ok(u64::try_from(count).unwrap_or(0))
    }
}

fn track_from_row(row: &Row<'_>) -> rusqlite::Result<Track> {
    let bpm_hundredths: Option<i64> = row.get(7)?;
    let length: Option<i64> = row.get(8)?;
    let file_size: Option<i64> = row.get(10)?;
    let rating: Option<i64> = row.get(11)?;
    Ok(Track {
        id: row.get(0)?,
        title: row.get(1)?,
        artist: row.get(2)?,
        album: row.get(3)?,
        genre: row.get(4)?,
        label: row.get(5)?,
        key: row.get(6)?,
        bpm: bpm_hundredths
            .filter(|value| *value > 0)
            .map(|value| value as f64 / 100.0),
        length_seconds: length.and_then(|value| u32::try_from(value).ok()),
        file_path: row.get::<_, Option<String>>(9)?.map(PathBuf::from),
        file_size: file_size.and_then(|value| u64::try_from(value).ok()),
        rating: rating.and_then(|value| u8::try_from(value).ok()),
        comment: row.get(12)?,
        analysis_path: row.get(13)?,
        date_added: row.get(14)?,
        auto_gain_db: row
            .get::<_, Option<i64>>(15)?
            .and_then(|bits| u32::try_from(bits).ok())
            .and_then(mixer_gain_db),
    })
}

/// A `djmdMixerParam` gain in decibels. rekordbox keeps the linear gain factor as the bits of
/// an IEEE 754 single, its upper 16 bits in `GainHigh` and its lower 16 in `GainLow`.
/// A factor that is not a positive number gives none.
pub(crate) fn mixer_gain_db(bits: u32) -> Option<f32> {
    let factor = f32::from_bits(bits);
    (factor.is_finite() && factor > 0.0).then(|| 20.0 * factor.log10())
}

fn cue_from_row(row: &Row<'_>) -> rusqlite::Result<Cue> {
    let kind: i64 = row.get::<_, Option<i64>>(2)?.unwrap_or(0);
    let start: Option<i64> = row.get(3)?;
    let end: Option<i64> = row.get(4)?;
    let kind = CueKind::from_rekordbox_kind(kind);
    let color: Option<i64> = row.get(5)?;
    let color_table_index: Option<i64> = row.get(7)?;
    Ok(Cue {
        id: row.get(0)?,
        track_id: row.get(1)?,
        kind,
        start_ms: start
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(0),
        loop_end_ms: end
            .filter(|value| *value > 0)
            .and_then(|value| u32::try_from(value).ok()),
        hot_cue_color: match kind {
            CueKind::Hot { .. } => color_table_index.and_then(PaletteColor::from_index),
            CueKind::Memory | CueKind::Unknown { .. } => None,
        },
        memory_color: match kind {
            CueKind::Memory => color.and_then(MemoryCueColor::from_rekordbox),
            CueKind::Hot { .. } | CueKind::Unknown { .. } => None,
        },
        comment: row
            .get::<_, Option<String>>(6)?
            .filter(|comment| !comment.is_empty()),
    })
}

fn copy_file(from: &Path, to: &Path) -> Result<(), Error> {
    std::fs::copy(from, to)
        .map(|_| ())
        .map_err(|source| Error::Copy {
            path: from.to_path_buf(),
            source,
        })
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mixer_gain_is_the_decibels_of_its_single() {
        let decibels = |factor: f32| mixer_gain_db(factor.to_bits());
        assert_eq!(decibels(1.0), Some(0.0));
        assert!((decibels(0.5).unwrap() + 6.020_6).abs() < 1e-3);
        assert!((decibels(2.0).unwrap() - 6.020_6).abs() < 1e-3);
        assert_eq!(decibels(0.0), None);
        assert_eq!(decibels(-1.0), None);
        assert_eq!(mixer_gain_db(f32::NAN.to_bits()), None);
    }
}
