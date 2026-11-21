use std::fmt;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, ErrorCode, OptionalExtension, Row};
use serde::Serialize;
use tempfile::TempDir;

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
        connection.pragma_update(None, "key", &key.0)?;
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
        Ok(Self {
            connection,
            _copy_directory: copy_directory,
        })
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
                    content.Rating, content.Commnt, content.AnalysisDataPath, content.StockDate
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
            "SELECT ID, ContentID, Kind, InMsec, OutMsec, Color, Comment
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
        let mut entry_statement = self.connection.prepare(
            "SELECT entry.ContentID
             FROM djmdSongPlaylist AS entry
             JOIN djmdContent AS content ON content.ID = entry.ContentID
             WHERE entry.PlaylistID = ?1
               AND entry.rb_local_deleted = 0
               AND content.rb_local_deleted = 0
             ORDER BY entry.TrackNo, entry.ID",
        )?;
        let shells = playlist_statement
            .query_map([], |row| {
                let parent_id: Option<String> = row.get(1)?;
                Ok(Playlist {
                    id: row.get(0)?,
                    parent_id: parent_id.filter(|parent| parent != "root"),
                    name: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    kind: PlaylistKind::from_attribute(row.get::<_, Option<i64>>(3)?.unwrap_or(0)),
                    position: row.get::<_, Option<i64>>(4)?.unwrap_or(0),
                    track_ids: Vec::new(),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut playlists = Vec::with_capacity(shells.len());
        for mut playlist in shells {
            if playlist.kind == PlaylistKind::Playlist {
                playlist.track_ids = entry_statement
                    .query_map([&playlist.id], |row| row.get(0))?
                    .collect::<Result<Vec<String>, _>>()?;
            }
            playlists.push(playlist);
        }
        Ok(playlists)
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
    let rating: Option<i64> = row.get(10)?;
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
        rating: rating.and_then(|value| u8::try_from(value).ok()),
        comment: row.get(11)?,
        analysis_path: row.get(12)?,
        date_added: row.get(13)?,
    })
}

fn cue_from_row(row: &Row<'_>) -> rusqlite::Result<Cue> {
    let kind: i64 = row.get::<_, Option<i64>>(2)?.unwrap_or(0);
    let start: Option<i64> = row.get(3)?;
    let end: Option<i64> = row.get(4)?;
    Ok(Cue {
        id: row.get(0)?,
        track_id: row.get(1)?,
        kind: match u32::try_from(kind) {
            Ok(number) if number > 0 => CueKind::Hot { number },
            _ => CueKind::Memory,
        },
        start_ms: start
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(0),
        loop_end_ms: end
            .filter(|value| *value > 0)
            .and_then(|value| u32::try_from(value).ok()),
        color: row.get(5)?,
        comment: row.get(6)?,
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
