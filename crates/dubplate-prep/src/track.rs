use std::path::PathBuf;

use euphoria_core::harmony::MixTrack;
use euphoria_core::key::Key;

/// A track as set preparation sees it: which file it is and what mixing needs to know.
#[derive(Debug, Clone, PartialEq)]
pub struct PrepTrack {
    /// The audio file, the track's identity in a set: tracks from the DJ's own folders have no
    /// rekordbox id.
    pub track_path: PathBuf,
    /// rekordbox's identifier, when the track is in the rekordbox collection.
    pub rekordbox_id: Option<String>,
    /// The title.
    pub title: String,
    /// The artist.
    pub artist: String,
    /// The key rekordbox shows, the DJ's trusted label.
    pub key: Option<Key>,
    /// The tempo in beats per minute.
    pub bpm: Option<f32>,
    /// The energy from 1 to 10, when it has been rated.
    pub energy: Option<f32>,
}

impl PrepTrack {
    /// Reads the mixing facts of a rekordbox track, or None when rekordbox has no file for it.
    /// rekordbox has no energy rating, so `energy` comes from analysis or from the DJ.
    pub fn from_rekordbox(track: &dubplate_rekordbox::Track, energy: Option<f32>) -> Option<Self> {
        Some(Self {
            track_path: track.file_path.clone()?,
            rekordbox_id: Some(track.id.clone()),
            title: track.title.clone().unwrap_or_default(),
            artist: track.artist.clone().unwrap_or_default(),
            key: track.key.as_deref().and_then(Key::parse_file_key),
            bpm: track.bpm.map(|bpm| bpm as f32),
            energy,
        })
    }
}

impl MixTrack for PrepTrack {
    fn key(&self) -> Option<Key> {
        self.key
    }

    fn bpm(&self) -> Option<f32> {
        self.bpm
    }

    fn energy(&self) -> Option<f32> {
        self.energy
    }
}

/// The tempo or key a track shows when both its library and Dubplate have a value.
pub fn preferred_track_value<Value>(
    in_rekordbox: bool,
    source: Option<Value>,
    analysed: Option<Value>,
) -> Option<Value> {
    if in_rekordbox {
        source.or(analysed)
    } else {
        analysed.or(source)
    }
}
