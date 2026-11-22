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
    /// The size of the audio file in bytes, when rekordbox recorded it.
    pub file_size: Option<u64>,
    /// The rating from 0 to 5 stars.
    pub rating: Option<u8>,
    /// The comment.
    pub comment: Option<String>,
    /// The analysis file, relative to rekordbox's `share` directory.
    pub analysis_path: Option<String>,
    /// When the track was added, as rekordbox wrote it.
    pub date_added: Option<String>,
}

/// Whether a cue is a memory cue or a hot cue, and on which pad.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CueKind {
    /// A memory cue, which has no pad.
    Memory,
    /// A hot cue on one of the eight pads.
    Hot {
        /// The pad, A to H.
        pad: HotCuePad,
    },
    /// A `Kind` value this version of Dubplate does not know.
    Unknown {
        /// rekordbox's raw `Kind` value.
        kind: i64,
    },
}

impl CueKind {
    pub(crate) fn from_rekordbox_kind(kind: i64) -> Self {
        if kind == 0 {
            return Self::Memory;
        }
        match HotCuePad::from_rekordbox_kind(kind) {
            Some(pad) => Self::Hot { pad },
            None => Self::Unknown { kind },
        }
    }

    /// The pad of a hot cue, or `None` for a memory cue or an unknown kind.
    pub fn pad(self) -> Option<HotCuePad> {
        match self {
            Self::Hot { pad } => Some(pad),
            Self::Memory | Self::Unknown { .. } => None,
        }
    }
}

/// A hot cue pad.
///
/// rekordbox 7.2.19 stores pads A to H as `Kind` 1, 2, 3, 5, 6, 7, 8 and 9: it never uses 4.
/// Checked by setting every pad on one track in rekordbox and reading the library back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum HotCuePad {
    /// Pad A.
    A,
    /// Pad B.
    B,
    /// Pad C.
    C,
    /// Pad D.
    D,
    /// Pad E.
    E,
    /// Pad F.
    F,
    /// Pad G.
    G,
    /// Pad H.
    H,
}

impl HotCuePad {
    /// Every pad, A to H.
    pub const ALL: [Self; 8] = [
        Self::A,
        Self::B,
        Self::C,
        Self::D,
        Self::E,
        Self::F,
        Self::G,
        Self::H,
    ];

    /// The pad rekordbox means by a `Kind` value, if it is a hot cue.
    pub fn from_rekordbox_kind(kind: i64) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|pad| pad.rekordbox_kind() == kind)
    }

    /// The `Kind` value rekordbox stores for this pad.
    pub fn rekordbox_kind(self) -> i64 {
        match self {
            Self::A => 1,
            Self::B => 2,
            Self::C => 3,
            Self::D => 5,
            Self::E => 6,
            Self::F => 7,
            Self::G => 8,
            Self::H => 9,
        }
    }

    /// The pad's place from 0 for A to 7 for H, as rekordbox XML numbers hot cues.
    pub fn index(self) -> usize {
        match self {
            Self::A => 0,
            Self::B => 1,
            Self::C => 2,
            Self::D => 3,
            Self::E => 4,
            Self::F => 5,
            Self::G => 6,
            Self::H => 7,
        }
    }

    /// The pad's letter.
    pub fn letter(self) -> char {
        match self {
            Self::A => 'A',
            Self::B => 'B',
            Self::C => 'C',
            Self::D => 'D',
            Self::E => 'E',
            Self::F => 'F',
            Self::G => 'G',
            Self::H => 'H',
        }
    }
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
