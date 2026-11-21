//! What each deck holds on the interface side: the track, its waveforms and beat grid, and the
//! playhead shown. Preparing a track (opening the audio file and reading rekordbox's analysis)
//! happens on a background thread; the audio itself lives in the engine.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use dubplate_engine::DeckId;
use dubplate_rekordbox::TrackAnalysis;

use crate::collection::{CollectionTrack, TrackOrigin};
use crate::playhead::PlayheadClock;
use crate::waveform::{BeatMarks, OverviewWaveform, Zoom, ZoomedWaveform};

/// A track made ready for a deck on a background thread.
pub struct PreparedTrack {
    /// The audio, decoding as it plays.
    pub audio: dubplate_engine::Track,
    zoomed: Option<ZoomedWaveform>,
    overview: Option<OverviewWaveform>,
    beats: BeatMarks,
    analysis_problem: Option<String>,
}

/// The waveforms and beat grid of a loaded track. Missing analysis leaves them empty and says
/// why in `analysis_problem`; the track still plays.
pub struct TrackVisuals {
    /// The scrolling waveform at the lanes' zoom.
    pub zoomed: Option<Arc<ZoomedWaveform>>,
    /// The whole-track waveform, with its bars kept for the last width drawn.
    pub overview: Option<Rc<RefCell<OverviewBars>>>,
    /// The beat grid.
    pub beats: Arc<BeatMarks>,
    /// Why rekordbox's analysis could not be read, if it could not.
    pub analysis_problem: Option<String>,
}

impl TrackVisuals {
    /// The visuals of a prepared track, for the interface thread.
    pub fn new(prepared: &mut PreparedTrack) -> Self {
        Self {
            zoomed: prepared.zoomed.take().map(Arc::new),
            overview: prepared
                .overview
                .take()
                .map(|waveform| Rc::new(RefCell::new(OverviewBars::new(waveform)))),
            beats: Arc::new(std::mem::take(&mut prepared.beats)),
            analysis_problem: prepared.analysis_problem.take(),
        }
    }
}

/// An overview waveform with its bars for one bar count, worked out again only when the count
/// changes.
pub struct OverviewBars {
    waveform: OverviewWaveform,
    bars: Vec<f32>,
}

impl OverviewBars {
    fn new(waveform: OverviewWaveform) -> Self {
        Self {
            waveform,
            bars: Vec::new(),
        }
    }

    /// The level of each of `bar_count` bars.
    pub fn bars(&mut self, bar_count: usize) -> &[f32] {
        if self.bars.len() != bar_count {
            self.bars = self.waveform.bars(bar_count);
        }
        &self.bars
    }
}

/// Why a track could not be loaded.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    /// rekordbox has no file path for the track.
    #[error("rekordbox has no audio file for this track")]
    NoFile,
    /// The audio file is not where rekordbox says it is.
    #[error("The audio file is missing: {0}")]
    Missing(String),
    /// The engine could not open or decode the file.
    #[error(transparent)]
    Audio(#[from] dubplate_engine::EngineError),
}

/// Opens the track's audio and reads its analysis from rekordbox's `share_directory`. Disk
/// work: run it off the interface thread.
pub fn prepare_track(
    entry: &CollectionTrack,
    share_directory: Option<&Path>,
    zoom: Zoom,
) -> Result<PreparedTrack, LoadError> {
    let path = entry.track.file_path.as_deref().ok_or(LoadError::NoFile)?;
    if !path.is_file() {
        let name = path.file_name().unwrap_or(path.as_os_str());
        return Err(LoadError::Missing(name.to_string_lossy().into_owned()));
    }
    let audio = dubplate_engine::decode_file(path)?;
    let analysis_path = entry
        .track
        .analysis_path
        .as_deref()
        .filter(|analysis_path| !analysis_path.is_empty());
    let analysis = match (entry.origin, analysis_path, share_directory) {
        (TrackOrigin::File, _, _) => {
            Err("Not in your rekordbox collection: no beat grid, cues or waveform".to_owned())
        }
        (TrackOrigin::Rekordbox, Some(analysis_path), Some(share_directory)) => {
            TrackAnalysis::read(share_directory, analysis_path).map_err(|error| error.to_string())
        }
        (TrackOrigin::Rekordbox, _, _) => Err("rekordbox has not analysed this track".to_owned()),
    };
    Ok(match analysis {
        Ok(analysis) => PreparedTrack {
            audio,
            zoomed: ZoomedWaveform::from_waveforms(&analysis.waveforms, zoom),
            overview: OverviewWaveform::from_waveforms(&analysis.waveforms),
            beats: analysis
                .beat_grid
                .as_ref()
                .map(BeatMarks::from_grid)
                .unwrap_or_default(),
            analysis_problem: None,
        },
        Err(problem) => PreparedTrack {
            audio,
            zoomed: None,
            overview: None,
            beats: BeatMarks::default(),
            analysis_problem: Some(problem),
        },
    })
}

/// The letter of a deck, as printed on it.
pub fn deck_letter(deck: DeckId) -> &'static str {
    match deck {
        DeckId::A => "A",
        DeckId::B => "B",
    }
}

/// Where a deck's track stands.
#[derive(Default)]
pub enum DeckContent {
    /// No track.
    #[default]
    Empty,
    /// The track is being prepared.
    Loading,
    /// The track is ready to play, with its waveforms and beat grid.
    Loaded(TrackVisuals),
    /// The track could not be loaded, and why.
    Failed(String),
}

/// The interface side of one deck.
#[derive(Default)]
pub struct Deck {
    /// The track on the deck, loaded or not.
    pub track: Option<Arc<CollectionTrack>>,
    /// Where it stands.
    pub content: DeckContent,
    /// The playhead drawn on its waveforms.
    pub clock: PlayheadClock,
    load_generation: u64,
}

impl Deck {
    /// Marks `track` as loading and returns the number that its result must bring back, so a
    /// slower earlier load never replaces a later one.
    pub fn begin_load(&mut self, track: Arc<CollectionTrack>) -> u64 {
        self.load_generation += 1;
        self.track = Some(track);
        self.content = DeckContent::Loading;
        self.load_generation
    }

    /// Whether a load result with `generation` is the latest one asked for.
    pub fn is_current_load(&self, generation: u64) -> bool {
        self.load_generation == generation
    }

    /// The waveforms and beat grid, once loaded.
    pub fn visuals(&self) -> Option<&TrackVisuals> {
        match &self.content {
            DeckContent::Loaded(visuals) => Some(visuals),
            DeckContent::Empty | DeckContent::Loading | DeckContent::Failed(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_latest_load_counts() {
        let mut deck = Deck::default();
        let first = deck.begin_load(Arc::new(CollectionTrack::from_file(Path::new("one.mp3"))));
        let second = deck.begin_load(Arc::new(CollectionTrack::from_file(Path::new("two.mp3"))));
        assert!(!deck.is_current_load(first));
        assert!(deck.is_current_load(second));
        let title = deck.track.as_ref().map(|track| track.row.title.to_string());
        assert_eq!(title.as_deref(), Some("two"));
    }
}
