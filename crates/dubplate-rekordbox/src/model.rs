use std::path::PathBuf;

use serde::Serialize;

/// A track in the rekordbox collection, from `djmdContent` and the tables it points to.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Track {
    /// rekordbox's identifier for the track.
    pub id: String,
    /// The title.
    pub title: Option<String>,
    /// The artist's name.
    pub artist: Option<String>,
    /// The album's name.
    pub album: Option<String>,
    /// The genre's name.
    pub genre: Option<String>,
    /// The label's name.
    pub label: Option<String>,
    /// The key as rekordbox shows it, such as `8A` or `Am`, depending on the user's setting.
    pub key: Option<String>,
    /// The tempo in beats per minute. rekordbox stores it in hundredths.
    pub bpm: Option<f64>,
    /// The length in whole seconds.
    pub length_seconds: Option<u32>,
    /// The path of the audio file.
    pub file_path: Option<PathBuf>,
    /// The rating from 0 to 5 stars.
    pub rating: Option<u8>,
    /// The comment.
    pub comment: Option<String>,
    /// The analysis file, relative to rekordbox's `share` directory.
    pub analysis_path: Option<String>,
    /// When the track was added, as rekordbox wrote it.
    pub date_added: Option<String>,
}

/// Whether a cue is a memory cue or a hot cue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CueKind {
    /// A memory cue, which has no pad.
    Memory,
    /// A hot cue. `number` is rekordbox's raw `Kind` value, which is 1 or more.
    Hot {
        /// rekordbox's raw `Kind` value.
        number: u32,
    },
}

/// A cue point or saved loop, from `djmdCue`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Cue {
    /// rekordbox's identifier for the cue.
    pub id: String,
    /// The track the cue belongs to.
    pub track_id: String,
    /// Memory cue or hot cue.
    pub kind: CueKind,
    /// Where the cue starts, in milliseconds.
    pub start_ms: u32,
    /// Where the loop ends, in milliseconds, when the cue is a saved loop.
    pub loop_end_ms: Option<u32>,
    /// rekordbox's colour value for the cue.
    pub color: Option<i64>,
    /// The cue's name.
    pub comment: Option<String>,
}

/// What a node in the playlist tree is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaylistKind {
    /// A playlist with tracks in a fixed order.
    Playlist,
    /// A folder of playlists.
    Folder,
    /// A playlist whose tracks come from rules.
    Smart,
    /// A value this version of Dubplate does not know.
    Unknown(i64),
}

impl PlaylistKind {
    pub(crate) fn from_attribute(attribute: i64) -> Self {
        match attribute {
            0 => Self::Playlist,
            1 => Self::Folder,
            4 => Self::Smart,
            other => Self::Unknown(other),
        }
    }
}

/// A playlist, folder or smart playlist, from `djmdPlaylist` and `djmdSongPlaylist`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Playlist {
    /// rekordbox's identifier for the playlist.
    pub id: String,
    /// The folder it sits in, or `None` at the top level.
    pub parent_id: Option<String>,
    /// The name.
    pub name: String,
    /// Playlist, folder or smart playlist.
    pub kind: PlaylistKind,
    /// The position among its siblings.
    pub position: i64,
    /// The tracks in playlist order. Empty for folders and smart playlists.
    pub track_ids: Vec<String>,
}
