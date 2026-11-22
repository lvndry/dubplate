//! What each deck holds on the interface side: the track, its waveforms and beat grid, and the
//! playhead shown. Preparing a track (opening the audio file and reading rekordbox's analysis)
//! happens on a background thread; the audio itself lives in the engine.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use dubplate_engine::DeckId;
use dubplate_rekordbox::{HotCuePad, TrackAnalysis};

use crate::collection::{CollectionTrack, TrackOrigin};
use crate::playhead::PlayheadClock;
use crate::prep::GridCorrection;
use crate::waveform::{BeatMarks, OverviewWaveform, Zoom, ZoomedWaveform};

/// A track made ready for a deck on a background thread.
pub struct PreparedTrack {
    /// The audio, decoding as it plays.
    pub audio: dubplate_engine::Track,
    zoomed: Option<ZoomedWaveform>,
    overview: Option<OverviewWaveform>,
    beats: BeatMarks,
    /// The tempo and first downbeat, for the engine's beat-synced effects.
    pub beat_grid: Option<dubplate_engine::BeatGrid>,
    analysis_problem: Option<String>,
}

/// The waveforms and beat grid of a loaded track. Missing analysis leaves them empty and says
/// why in `analysis_problem`; the track still plays.
#[derive(Clone)]
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

/// Opens the track's audio and reads its analysis from rekordbox's `share_directory`, with
/// the DJ's `correction` of its beat grid. Disk work: run it off the interface thread.
pub fn prepare_track(
    entry: &CollectionTrack,
    share_directory: Option<&Path>,
    zoom: Zoom,
    correction: GridCorrection,
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
        (TrackOrigin::Folder, _, _) => Err("Not analysed yet: no beat grid or waveform".to_owned()),
        (TrackOrigin::Rekordbox, Some(analysis_path), Some(share_directory)) => {
            TrackAnalysis::read(share_directory, analysis_path).map_err(|error| error.to_string())
        }
        (TrackOrigin::Rekordbox, _, _) => Err("rekordbox has not analysed this track".to_owned()),
    };
    Ok(match analysis {
        Ok(analysis) => {
            let grid = analysis
                .beat_grid
                .as_ref()
                .map(|grid| correction.apply(grid));
            PreparedTrack {
                audio,
                zoomed: ZoomedWaveform::from_waveforms(&analysis.waveforms, zoom),
                overview: OverviewWaveform::from_waveforms(&analysis.waveforms),
                beats: grid.as_ref().map(BeatMarks::from_grid).unwrap_or_default(),
                beat_grid: grid.as_ref().and_then(engine_beat_grid),
                analysis_problem: None,
            }
        }
        Err(problem) => PreparedTrack {
            audio,
            zoomed: None,
            overview: None,
            beats: BeatMarks::default(),
            beat_grid: None,
            analysis_problem: Some(problem),
        },
    })
}

/// rekordbox's grid as the engine keeps it: the tempo at the first downbeat, and where that
/// downbeat falls, so effects land on the beat and the bar.
fn engine_beat_grid(
    grid: &dubplate_rekordbox::analysis::BeatGrid,
) -> Option<dubplate_engine::BeatGrid> {
    let first = grid
        .beats
        .iter()
        .find(|beat| beat.beat_in_bar == 1)
        .or_else(|| grid.beats.first())?;
    dubplate_engine::BeatGrid::new(
        f64::from(first.tempo_hundredths) / 100.0,
        f64::from(first.time_ms) / 1000.0,
    )
    .ok()
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

/// The hot cues of the track on a deck, pad A first, in seconds from the start of the track.
/// They start as rekordbox saved them; pads set or deleted on the deck change only this copy.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct HotCues([Option<f64>; HotCuePad::ALL.len()]);

impl HotCues {
    /// The hot cues rekordbox saved for `track`.
    pub fn of(track: &CollectionTrack) -> Self {
        let mut hot_cues = Self::default();
        for cue in &track.hot_cues {
            hot_cues.set(cue.pad, cue.seconds);
        }
        hot_cues
    }

    /// Where the hot cue on `pad` is, if one is set.
    pub fn get(&self, pad: HotCuePad) -> Option<f64> {
        self.0[pad_position(pad)]
    }

    /// Puts the hot cue of `pad` at `seconds`.
    pub fn set(&mut self, pad: HotCuePad, seconds: f64) {
        self.0[pad_position(pad)] = Some(seconds);
    }

    /// Deletes the hot cue of `pad`.
    pub fn delete(&mut self, pad: HotCuePad) {
        self.0[pad_position(pad)] = None;
    }

    /// Every hot cue set, with its pad, A first.
    pub fn iter(&self) -> impl Iterator<Item = (HotCuePad, f64)> + '_ {
        HotCuePad::ALL
            .into_iter()
            .zip(self.0)
            .filter_map(|(pad, seconds)| Some((pad, seconds?)))
    }
}

/// Where `pad` sits among the pads, A being 0.
pub fn pad_position(pad: HotCuePad) -> usize {
    HotCuePad::ALL
        .iter()
        .position(|candidate| *candidate == pad)
        .unwrap_or_default()
}

/// The interface side of one deck.
#[derive(Default)]
pub struct Deck {
    /// The track on the deck, loaded or not.
    pub track: Option<Arc<CollectionTrack>>,
    /// Where it stands.
    pub content: DeckContent,
    /// The track's hot cues as the pads show them.
    pub hot_cues: HotCues,
    /// The playhead drawn on its waveforms.
    pub clock: PlayheadClock,
    /// Whether CUE is held down at the cue point, playing until it is let go, as on a CDJ.
    pub cue_preview: bool,
    load_generation: u64,
}

impl Deck {
    /// Marks `track` as loading with `hot_cues` on its pads, and returns the number that its
    /// result must bring back, so a slower earlier load never replaces a later one.
    pub fn begin_load(&mut self, track: Arc<CollectionTrack>, hot_cues: HotCues) -> u64 {
        self.load_generation += 1;
        self.track = Some(track);
        self.content = DeckContent::Loading;
        self.hot_cues = hot_cues;
        self.cue_preview = false;
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
        let first = deck.begin_load(
            Arc::new(CollectionTrack::from_file(Path::new("one.mp3"))),
            HotCues::default(),
        );
        let second = deck.begin_load(
            Arc::new(CollectionTrack::from_file(Path::new("two.mp3"))),
            HotCues::default(),
        );
        assert!(!deck.is_current_load(first));
        assert!(deck.is_current_load(second));
        let title = deck.track.as_ref().map(|track| track.row.title.to_string());
        assert_eq!(title.as_deref(), Some("two"));
    }

    #[test]
    fn hot_cues_are_set_and_deleted_by_pad() {
        let mut hot_cues = HotCues::default();
        hot_cues.set(HotCuePad::C, 12.5);
        hot_cues.set(HotCuePad::A, 1.0);
        assert_eq!(hot_cues.get(HotCuePad::C), Some(12.5));
        let set: Vec<(char, f64)> = hot_cues
            .iter()
            .map(|(pad, seconds)| (pad.letter(), seconds))
            .collect();
        assert_eq!(set, [('A', 1.0), ('C', 12.5)]);
        hot_cues.delete(HotCuePad::C);
        assert_eq!(hot_cues.get(HotCuePad::C), None);
        assert_eq!(pad_position(HotCuePad::H), 7);
    }
}
