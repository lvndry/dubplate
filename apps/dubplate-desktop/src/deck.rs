//! What each deck holds on the interface side: the track, its waveforms and beat grid, and the
//! playhead shown. Preparing a track (opening the audio file and reading rekordbox's analysis,
//! or taking Dubplate's for a track rekordbox never analysed) happens on a background thread;
//! the audio itself lives in the engine.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use dubplate_engine::DeckId;
use dubplate_prep::{AudioAnalysis, GridCorrection};
use dubplate_rekordbox::analysis::BeatGrid;
use dubplate_rekordbox::{TrackAnalysis, TrackCues};

use crate::collection::{CollectionTrack, TrackOrigin};
use crate::cue_marks::{MarkerPulse, SeenMarks};
use crate::phrases::{PhraseSpan, phrase_spans};
use crate::playhead::PlayheadClock;
use crate::prep::chosen_grid;
use crate::tap_tempo::TapTempo;
use crate::waveform::{BandLevels, BeatMarks, OverviewWaveform, Zoom, ZoomedWaveform};

/// A track made ready for a deck on a background thread.
pub struct PreparedTrack {
    /// The audio, decoding as it plays.
    pub audio: dubplate_engine::Track,
    zoomed: Option<ZoomedWaveform>,
    overview: Option<OverviewWaveform>,
    beats: BeatMarks,
    read_grid: Option<Arc<BeatGrid>>,
    phrases: Vec<PhraseSpan>,
    /// The tempo and first downbeat, for the engine's beat-synced effects.
    pub beat_grid: Option<dubplate_engine::BeatGrid>,
    /// Where the beat grid comes from, when the track has one.
    pub grid_source: Option<GridSource>,
    analysis_problem: Option<String>,
}

/// Where the beat grid of the track on a deck comes from, which decides what may replace it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridSource {
    /// rekordbox's analysis, which nothing on the deck replaces.
    Rekordbox,
    /// Dubplate's analysis, for a track rekordbox never analysed; tapping replaces it.
    Analysis,
    /// The DJ's taps.
    Tapped,
}

/// A beat grid made ready for a deck: its marks for the waveforms and the engine's grid.
pub struct DeckGrid {
    /// The beats drawn on the waveforms.
    pub beats: BeatMarks,
    /// The tempo and first downbeat, for sync, the beat-synced effects, loops and quantize.
    pub engine: Option<dubplate_engine::BeatGrid>,
}

impl DeckGrid {
    /// `grid` with the DJ's `correction`, as the deck uses it.
    pub fn new(grid: &BeatGrid, correction: GridCorrection) -> Self {
        let corrected = correction.apply(grid);
        Self {
            beats: BeatMarks::from_grid(&corrected),
            engine: engine_beat_grid(&corrected),
        }
    }
}

/// What the lanes say while the waveform of a track rekordbox never analysed is drawn from its
/// audio, once Dubplate's analysis has given it a beat grid.
pub const ANALYSED_GRID_NOTE: &str = "Dubplate's beat grid \u{b7} drawing the waveform\u{2026}";

/// The waveforms and beat grid of a loaded track. Missing analysis leaves them empty and says
/// why in `analysis_problem`; the track still plays.
#[derive(Clone)]
pub struct TrackVisuals {
    /// The scrolling waveform at the lanes' zoom.
    pub zoomed: Option<Arc<ZoomedWaveform>>,
    /// The whole-track waveform, with its bars kept for the last width drawn.
    pub overview: Option<Rc<RefCell<OverviewBars>>>,
    /// The beat grid, with the DJ's fix.
    pub beats: Arc<BeatMarks>,
    /// The beat grid as rekordbox or Dubplate's analysis found it, before the DJ's fix, so a
    /// new fix applies to it again. A tapped grid has none.
    pub read_grid: Option<Arc<BeatGrid>>,
    /// rekordbox's phrases in seconds, for the overview's phrase strip.
    pub phrases: Arc<[PhraseSpan]>,
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
            read_grid: prepared.read_grid.take(),
            phrases: std::mem::take(&mut prepared.phrases).into(),
            analysis_problem: prepared.analysis_problem.take(),
        }
    }

    /// Draws `zoomed` and `overview`, worked out after the track was loaded.
    pub fn show_waveforms(
        &mut self,
        zoomed: Arc<ZoomedWaveform>,
        overview: Option<OverviewWaveform>,
    ) {
        self.zoomed = Some(zoomed);
        self.overview = overview.map(|waveform| Rc::new(RefCell::new(OverviewBars::new(waveform))));
    }
}

/// An overview waveform with its bars for one bar count, worked out again only when the count
/// changes.
pub struct OverviewBars {
    waveform: OverviewWaveform,
    bars: Vec<BandLevels>,
}

impl OverviewBars {
    fn new(waveform: OverviewWaveform) -> Self {
        Self {
            waveform,
            bars: Vec::new(),
        }
    }

    /// The bands of each of `bar_count` bars.
    pub fn bars(&mut self, bar_count: usize) -> &[BandLevels] {
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
/// the DJ's `correction` of its beat grid. A track rekordbox has no grid for takes the grid of
/// Dubplate's `analysis`, when it has arrived. Disk work: run it off the interface thread.
pub fn prepare_track(
    entry: &CollectionTrack,
    share_directory: Option<&Path>,
    zoom: Zoom,
    correction: GridCorrection,
    analysis: Option<&AudioAnalysis>,
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
    let rekordbox = match (entry.origin, analysis_path, share_directory) {
        (TrackOrigin::File, _, _) => {
            Err("Not in your rekordbox collection: no beat grid or cues \u{b7} drawing the waveform\u{2026}".to_owned())
        }
        (TrackOrigin::Folder, _, _) => {
            Err("Not analysed yet: no beat grid \u{b7} drawing the waveform\u{2026}".to_owned())
        }
        (TrackOrigin::Rekordbox, Some(analysis_path), Some(share_directory)) => {
            TrackAnalysis::read(share_directory, analysis_path).map_err(|error| error.to_string())
        }
        (TrackOrigin::Rekordbox, _, _) => Err(
            "rekordbox has not analysed this track \u{b7} drawing the waveform\u{2026}".to_owned(),
        ),
    };
    let rekordbox_problem = rekordbox.as_ref().err().cloned();
    let rekordbox = rekordbox.as_ref().ok();
    let chosen = chosen_grid(rekordbox, analysis);
    let grid_source = chosen.as_ref().map(|(_, source)| *source);
    let read_grid = chosen.map(|(grid, _)| Arc::new(grid));
    let (beats, beat_grid) = read_grid
        .as_deref()
        .map(|grid| DeckGrid::new(grid, correction))
        .map_or_else(Default::default, |grid| (grid.beats, grid.engine));
    let analysis_problem = match grid_source {
        Some(GridSource::Analysis) => Some(ANALYSED_GRID_NOTE.to_owned()),
        Some(GridSource::Rekordbox | GridSource::Tapped) | None => rekordbox_problem,
    };
    Ok(PreparedTrack {
        audio,
        zoomed: rekordbox
            .and_then(|analysis| ZoomedWaveform::from_waveforms(&analysis.waveforms, zoom)),
        overview: rekordbox
            .and_then(|analysis| OverviewWaveform::from_waveforms(&analysis.waveforms)),
        phrases: rekordbox
            .and_then(|analysis| {
                Some(phrase_spans(
                    analysis.song_structure.as_ref()?,
                    analysis.beat_grid.as_ref()?,
                ))
            })
            .unwrap_or_default(),
        beats,
        read_grid,
        beat_grid,
        grid_source,
        analysis_problem,
    })
}

/// Preserves every tempo segment and anchors beat zero at the first downbeat. A grid the engine
/// cannot follow beat by beat, such as one with a beat out of order or at an impossible tempo,
/// still gives the deck a steady grid at the downbeat's tempo rather than none at all.
fn engine_beat_grid(grid: &BeatGrid) -> Option<dubplate_engine::BeatGrid> {
    let downbeat = grid.first_downbeat_index().unwrap_or(0);
    let beats: Vec<_> = grid
        .beats
        .iter()
        .map(|beat| dubplate_engine::GridBeat {
            seconds: f64::from(beat.time_ms) / 1000.0,
            bpm: f64::from(beat.tempo_hundredths) / 100.0,
        })
        .collect();
    dubplate_engine::BeatGrid::from_beats_with_downbeat(&beats, downbeat)
        .or_else(|_| {
            let anchor = beats.get(downbeat).ok_or(())?;
            dubplate_engine::BeatGrid::new(anchor.bpm, anchor.seconds).map_err(|_| ())
        })
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

/// The interface side of one deck.
#[derive(Default)]
pub struct Deck {
    /// The track on the deck, loaded or not.
    pub track: Option<Arc<CollectionTrack>>,
    /// Where it stands.
    pub content: DeckContent,
    /// The track's cues as the deck uses them: the ones the DJ made in Dubplate, else
    /// rekordbox's.
    pub cues: Arc<TrackCues>,
    /// The playhead drawn on its waveforms.
    pub clock: PlayheadClock,
    /// Whether CUE is held down at the cue point, playing until it is let go, as on a CDJ.
    pub cue_preview: bool,
    /// The taps on this deck's tempo.
    pub tap_tempo: TapTempo,
    /// Where the deck's beat grid comes from, when it has one.
    pub grid_source: Option<GridSource>,
    /// Where the pointer is over the deck's overview, from 0.0 to 1.0.
    pub overview_hover: Option<f32>,
    /// The deck's cues as last drawn, to notice one being set.
    pub seen_marks: Option<SeenMarks>,
    /// The cue just set, pulsing where it landed.
    pub marker_pulse: Option<MarkerPulse>,
    load_generation: u64,
    committed_generation: u64,
    pending: Option<(Arc<CollectionTrack>, Arc<TrackCues>)>,
}

impl Deck {
    /// Marks `track` as loading with `cues`, and returns the number that its result must bring
    /// back, so a slower earlier load never replaces a later one.
    pub fn begin_load(&mut self, track: Arc<CollectionTrack>, cues: Arc<TrackCues>) -> u64 {
        self.load_generation += 1;
        self.pending = Some((track, cues));
        if self.track.is_none() {
            self.content = DeckContent::Loading;
        }
        self.load_generation
    }

    /// Makes a successfully committed engine load visible; stale completions do nothing.
    pub fn commit_load(
        &mut self,
        generation: u64,
        visuals: TrackVisuals,
        grid_source: Option<GridSource>,
    ) -> bool {
        if !self.is_current_load(generation) {
            return false;
        }
        let Some((track, cues)) = self.pending.take() else {
            return false;
        };
        self.committed_generation = generation;
        self.track = Some(track);
        self.cues = cues;
        self.content = DeckContent::Loaded(visuals);
        self.cue_preview = false;
        self.tap_tempo = TapTempo::default();
        self.grid_source = grid_source;
        true
    }

    /// Abandons a failed replacement while keeping the previous loaded track usable.
    pub fn fail_load(&mut self, generation: u64, problem: String) {
        if !self.is_current_load(generation) {
            return;
        }
        self.pending = None;
        if self.track.is_none() {
            self.content = DeckContent::Failed(problem);
        }
    }

    /// The number of the latest load asked for.
    pub fn load_generation(&self) -> u64 {
        self.load_generation
    }

    /// Whether a load result with `generation` is the latest one asked for.
    pub fn is_current_load(&self, generation: u64) -> bool {
        self.load_generation == generation
    }

    /// Whether a background visual still belongs to the committed track. A pending or
    /// failed replacement must not cancel work for the track that remains on the deck.
    pub fn is_loaded_generation(&self, generation: u64) -> bool {
        self.track.is_some() && self.committed_generation == generation
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
    use dubplate_rekordbox::{HotCue, HotCuePad};

    use super::*;

    #[test]
    fn only_the_latest_load_counts() {
        let mut deck = Deck::default();
        let first = deck.begin_load(
            Arc::new(CollectionTrack::from_file(Path::new("one.mp3"))),
            Arc::default(),
        );
        let second = deck.begin_load(
            Arc::new(CollectionTrack::from_file(Path::new("two.mp3"))),
            Arc::default(),
        );
        assert!(!deck.is_current_load(first));
        assert!(deck.is_current_load(second));
        assert!(
            deck.track.is_none(),
            "pending requests must not replace the loaded identity"
        );
        assert_eq!(deck.pending.as_ref().unwrap().0.row.title.as_ref(), "two");
    }

    fn visuals() -> TrackVisuals {
        TrackVisuals {
            zoomed: None,
            overview: None,
            beats: Arc::default(),
            read_grid: None,
            phrases: Arc::default(),
            analysis_problem: None,
        }
    }

    #[test]
    fn failed_replacement_retains_identity_cues_grid_and_visuals() {
        let mut deck = Deck::default();
        let mut cues = TrackCues::default();
        cues.set_hot_cue(HotCuePad::B, HotCue::at(2_000));
        let first = deck.begin_load(
            Arc::new(CollectionTrack::from_file(Path::new("old.mp3"))),
            Arc::new(cues),
        );
        assert!(deck.commit_load(first, visuals(), Some(GridSource::Tapped)));
        let failed = deck.begin_load(
            Arc::new(CollectionTrack::from_file(Path::new("missing.mp3"))),
            Arc::default(),
        );
        assert_eq!(deck.track.as_ref().unwrap().row.title.as_ref(), "old");
        assert!(deck.is_loaded_generation(first));
        assert!(!deck.is_current_load(first));
        deck.fail_load(failed, "missing".into());
        assert!(deck.is_loaded_generation(first));
        assert_eq!(deck.track.as_ref().unwrap().row.title.as_ref(), "old");
        assert_eq!(
            deck.cues.hot_cue(HotCuePad::B).map(|cue| cue.start_ms),
            Some(2_000)
        );
        assert_eq!(deck.grid_source, Some(GridSource::Tapped));
        assert!(deck.visuals().is_some());
        assert!(!deck.commit_load(first, visuals(), None));
    }

    #[test]
    fn variable_grid_keeps_tempo_changes_and_the_downbeat_anchor() {
        use dubplate_rekordbox::analysis::Beat;
        let grid = BeatGrid {
            beats: vec![
                Beat {
                    time_ms: 0,
                    tempo_hundredths: 12000,
                    beat_in_bar: 4,
                },
                Beat {
                    time_ms: 500,
                    tempo_hundredths: 12000,
                    beat_in_bar: 1,
                },
                Beat {
                    time_ms: 1000,
                    tempo_hundredths: 10000,
                    beat_in_bar: 2,
                },
                Beat {
                    time_ms: 1600,
                    tempo_hundredths: 10000,
                    beat_in_bar: 3,
                },
            ],
        };
        let engine = engine_beat_grid(&grid).unwrap();
        assert!((engine.bpm_at(3.0) - 100.0).abs() < 1e-8);
        assert!((engine.beat_at(0.5)).abs() < 1e-8);
        assert!((engine.beat_at(0.0) + 1.0).abs() < 1e-8);
    }

    #[test]
    fn the_djs_grid_fix_moves_a_variable_grid_and_keeps_its_tempo_changes() {
        use dubplate_rekordbox::analysis::Beat;
        let beat = |time_ms, tempo_hundredths, beat_in_bar| Beat {
            time_ms,
            tempo_hundredths,
            beat_in_bar,
        };
        let grid = BeatGrid {
            beats: vec![
                beat(0, 12000, 4),
                beat(500, 12000, 1),
                beat(1000, 10000, 2),
                beat(1600, 10000, 3),
            ],
        };
        let correction = GridCorrection {
            offset_ms: 20,
            downbeat_shift: 1,
        };
        let engine = DeckGrid::new(&grid, correction).engine.unwrap();
        assert!(engine.has_tempo_changes());
        assert!((engine.bpm_at(3.0) - 100.0).abs() < 1e-8);
        assert!(
            engine.beat_at(1.02).abs() < 1e-8,
            "the shifted downbeat is beat zero"
        );
        assert!((engine.beat_at(0.52) + 1.0).abs() < 1e-8);
    }

    #[test]
    fn a_grid_with_a_beat_out_of_order_still_gives_a_steady_grid_at_the_downbeat() {
        use dubplate_rekordbox::analysis::Beat;
        let grid = BeatGrid {
            beats: vec![
                Beat {
                    time_ms: 500,
                    tempo_hundredths: 12000,
                    beat_in_bar: 1,
                },
                Beat {
                    time_ms: 1000,
                    tempo_hundredths: 12000,
                    beat_in_bar: 2,
                },
                Beat {
                    time_ms: 1000,
                    tempo_hundredths: 12000,
                    beat_in_bar: 3,
                },
            ],
        };
        let engine = engine_beat_grid(&grid).unwrap();
        assert!(!engine.has_tempo_changes());
        assert!((engine.bpm() - 120.0).abs() < 1e-8);
        assert!((engine.beat_at(0.5)).abs() < 1e-8);
    }
}
