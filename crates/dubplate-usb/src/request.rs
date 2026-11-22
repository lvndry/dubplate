use std::path::PathBuf;

use dubplate_rekordbox::analysis::{AnalysisCue, BeatGrid};
use dubplate_rekordbox::device_library::DeviceColor;

/// Where rekordbox keeps its own analysis of a track, read to copy its waveforms and phrases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RekordboxAnalysis {
    /// rekordbox's `share` directory.
    pub share_directory: PathBuf,
    /// The track's analysis path as `master.db` stores it: `/PIONEER/USBANLZ/…/ANLZ0000.DAT`.
    pub analysis_path: String,
}

/// What players show about a track.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrackTags {
    /// The title; the file name when the tags have none.
    pub title: Option<String>,
    /// The artist.
    pub artist: Option<String>,
    /// The album.
    pub album: Option<String>,
    /// The genre.
    pub genre: Option<String>,
    /// The record label.
    pub label: Option<String>,
    /// The key in any notation Dubplate reads, such as `8A` or `Am`.
    pub key: Option<String>,
    /// The comment.
    pub comment: Option<String>,
    /// The rating, from 0 to 5 stars.
    pub rating: u8,
    /// The DJ's colour for the track.
    pub color: Option<DeviceColor>,
    /// The year of release.
    pub year: Option<u16>,
    /// When the track joined the DJ's library, as `yyyy-mm-dd`.
    pub date_added: Option<String>,
}

/// One track to put on the stick, with everything players need of it.
#[derive(Debug, Clone, PartialEq)]
pub struct UsbTrack {
    /// The audio file, where it is now.
    pub source: PathBuf,
    /// The tags players show.
    pub tags: TrackTags,
    /// The tempo players show, in beats per minute; the grid's first tempo when `None`.
    pub bpm: Option<f64>,
    /// The length in seconds, when known without decoding the file.
    pub duration_seconds: Option<f64>,
    /// The beat grid as the DJ left it: rekordbox's or Dubplate's analysis, with the DJ's fix.
    pub beat_grid: Option<BeatGrid>,
    /// Whether `beat_grid` is rekordbox's own, unchanged, so rekordbox's extended grid still
    /// describes it.
    pub grid_is_rekordbox_s: bool,
    /// Hot cues, memory cues and saved loops, with their colours and names.
    pub cues: Vec<AnalysisCue>,
    /// rekordbox's analysis of the track, whose waveforms and phrases are copied; Dubplate
    /// draws the waveforms from the audio when there is none.
    pub rekordbox_analysis: Option<RekordboxAnalysis>,
}

/// A playlist to put on the stick, at the top of its playlist tree.
#[derive(Debug, Clone, PartialEq)]
pub struct UsbPlaylist {
    /// The name players show. A playlist of the same name Dubplate put on the stick before is
    /// replaced.
    pub name: String,
    /// The tracks in playing order.
    pub tracks: Vec<UsbTrack>,
}
