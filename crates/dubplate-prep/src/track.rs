use euphoria_core::harmony::MixTrack;
use euphoria_core::key::Key;

/// A track as set preparation sees it: who it is in rekordbox and what mixing needs to know.
#[derive(Debug, Clone, PartialEq)]
pub struct PrepTrack {
    /// rekordbox's identifier for the track.
    pub rekordbox_id: String,
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
    /// Reads the mixing facts of a rekordbox track. rekordbox has no energy rating, so
    /// `energy` comes from analysis or from the DJ.
    pub fn from_rekordbox(track: &dubplate_rekordbox::Track, energy: Option<f32>) -> Self {
        Self {
            rekordbox_id: track.id.clone(),
            title: track.title.clone().unwrap_or_default(),
            artist: track.artist.clone().unwrap_or_default(),
            key: track.key.as_deref().and_then(Key::parse_file_key),
            bpm: track.bpm.map(|bpm| bpm as f32),
            energy,
        }
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
