//! Dubplate's analysis of each track (tempo, beat grid, key and energy) as it arrives from the
//! background analyser, and which tempo and key a track shows: rekordbox's for its own tracks,
//! the analysis for the others.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dubplate_prep::{AnalysisQueue, AudioAnalysis, Key, Priority};
use gpui::SharedString;

use crate::collection::{CollectionTrack, TrackOrigin};

/// How soon a track is analysed: the set's tracks and the decks' before the rest of the
/// library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AnalysisPriority {
    /// A track of the set being prepared, or one just loaded on a deck.
    Set,
    /// Any other track of the library.
    Library,
}

/// The analyser's answer for one track.
#[derive(Debug)]
pub struct AnalysedTrack {
    /// The track's audio file.
    pub path: PathBuf,
    /// Library revision that requested this answer.
    pub revision: u64,
    /// What the analysis found, or why the track could not be analysed.
    pub outcome: Result<AudioAnalysis, String>,
}

/// What analyses tracks off the interface thread. Kept this small so the analyser's own types
/// stay behind it.
pub trait TrackAnalyser {
    /// Asks for each track, by its file, to be analysed, sooner for an earlier `priority`.
    fn request(&self, tracks: Vec<PathBuf>, priority: AnalysisPriority, revision: u64);
    /// The next answer that has arrived, without waiting.
    fn next_result(&self) -> Option<AnalysedTrack>;
}

impl TrackAnalyser for AnalysisQueue {
    fn request(&self, tracks: Vec<PathBuf>, priority: AnalysisPriority, revision: u64) {
        let priority = match priority {
            AnalysisPriority::Set => Priority::Set,
            AnalysisPriority::Library => Priority::Library,
        };
        self.request_all_versioned(tracks.into_iter().map(|path| (path, priority, revision)));
    }

    fn next_result(&self) -> Option<AnalysedTrack> {
        let result = self.results().try_recv().ok()?;
        Some(AnalysedTrack {
            path: result.path,
            revision: result.revision,
            outcome: result.outcome,
        })
    }
}

/// What the analyser has said about a track so far.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AnalysisState<'analysis> {
    /// No answer yet, or no analyser to give one.
    Waiting,
    /// The track could not be analysed.
    Failed,
    /// What the analysis found.
    Found(&'analysis AudioAnalysis),
}

impl<'analysis> AnalysisState<'analysis> {
    /// What the analysis found, once it found it.
    pub fn found(self) -> Option<&'analysis AudioAnalysis> {
        match self {
            Self::Found(analysis) => Some(analysis),
            Self::Waiting | Self::Failed => None,
        }
    }
}

/// The analyses that have arrived and the tracks still waiting for one.
#[derive(Default)]
pub struct TrackAnalyses {
    analyser: Option<Box<dyn TrackAnalyser>>,
    found: HashMap<PathBuf, Arc<AudioAnalysis>>,
    waiting: HashMap<PathBuf, AnalysisPriority>,
    failed: HashMap<PathBuf, String>,
    revision: u64,
}

/// An answer retains the failure reason instead of treating it as missing metadata.
pub type AnalysisAnswer = (PathBuf, Result<Arc<AudioAnalysis>, String>);

impl TrackAnalyses {
    /// A source rescan asks the backend cache to validate file fingerprints again. Pending
    /// answers from the old library are ignored, including answers already in flight.
    pub fn refresh(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.found.clear();
        self.failed.clear();
        self.waiting.clear();
    }

    /// An explicit load may retry a file repaired since its previous failure.
    pub fn retry_failed(&mut self, path: &Path) {
        self.failed.remove(path);
    }

    /// Analyses from `analyser`, or none at all without one.
    pub fn new(analyser: Option<Box<dyn TrackAnalyser>>) -> Self {
        Self {
            analyser,
            ..Self::default()
        }
    }

    /// What the analyser has said about the track whose audio file is `path`.
    pub fn state(&self, path: &Path) -> AnalysisState<'_> {
        match self.found.get(path) {
            Some(analysis) => AnalysisState::Found(analysis),
            None if self.failed.contains_key(path) => AnalysisState::Failed,
            None => AnalysisState::Waiting,
        }
    }

    /// The analysis of the track whose audio file is `path`, once it has arrived.
    pub fn get(&self, path: &Path) -> Option<&Arc<AudioAnalysis>> {
        self.found.get(path)
    }

    /// How many tracks are waiting for their analysis.
    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }

    /// Asks for each track, by its file, to be analysed at `priority`, leaving out the ones
    /// answered already or waiting at the same priority or an earlier one.
    pub fn request<'tracks>(
        &mut self,
        tracks: impl IntoIterator<Item = &'tracks Path>,
        priority: AnalysisPriority,
    ) {
        let Some(analyser) = &self.analyser else {
            return;
        };
        let mut wanted = Vec::new();
        for path in tracks {
            let asked = self
                .waiting
                .get(path)
                .is_some_and(|waiting| *waiting <= priority);
            if asked || self.found.contains_key(path) || self.failed.contains_key(path) {
                continue;
            }
            self.waiting.insert(path.to_path_buf(), priority);
            wanted.push(path.to_path_buf());
        }
        if wanted.is_empty() {
            return;
        }
        analyser.request(wanted, priority, self.revision);
    }

    /// Takes arrived answers, preserving the error when a track could not be analysed.
    pub fn collect(&mut self) -> Vec<AnalysisAnswer> {
        let mut answered = Vec::new();
        let Some(analyser) = &self.analyser else {
            return answered;
        };
        while let Some(answer) = analyser.next_result() {
            if answer.revision != self.revision {
                continue;
            }
            self.waiting.remove(&answer.path);
            let analysis = answer.outcome.map(Arc::new);
            match &analysis {
                Ok(analysis) => {
                    self.failed.remove(&answer.path);
                    self.found.insert(answer.path.clone(), Arc::clone(analysis));
                }
                Err(problem) => {
                    self.found.remove(&answer.path);
                    self.failed.insert(answer.path.clone(), problem.clone());
                }
            }
            answered.push((answer.path, analysis));
        }
        answered
    }
}

/// The value a track shows when both its own data and the analysis may have one: rekordbox's
/// own analysis comes first for rekordbox tracks, Dubplate's for tracks from the music folders
/// and loose files, so the tempo shown is the one the deck's grid follows.
pub fn preferred<Value>(
    origin: TrackOrigin,
    own: Option<Value>,
    analysed: Option<Value>,
) -> Option<Value> {
    match origin {
        TrackOrigin::Rekordbox => own.or(analysed),
        TrackOrigin::Folder | TrackOrigin::File => analysed.or(own),
    }
}

/// The text of a tempo or key cell: the preferred value, blank while a track that would show
/// the analysis is still waiting for it, as the energy is.
fn cell(
    entry: &CollectionTrack,
    own: &SharedString,
    state: AnalysisState<'_>,
    analysed: Option<String>,
) -> SharedString {
    let own_value = (!own.is_empty()).then(|| own.clone());
    let waiting_for_it = state == AnalysisState::Waiting && entry.origin != TrackOrigin::Rekordbox;
    if waiting_for_it {
        return SharedString::default();
    }
    preferred(entry.origin, own_value, analysed.map(SharedString::from)).unwrap_or_default()
}

/// The BPM a track shows, with two decimals.
pub fn bpm_cell(entry: &CollectionTrack, state: AnalysisState<'_>) -> SharedString {
    let analysed = state
        .found()
        .and_then(|analysis| analysis.bpm)
        .map(|bpm| format!("{bpm:.2}"));
    cell(entry, &entry.row.bpm, state, analysed)
}

/// The key a track shows, in Camelot for the analysed ones.
pub fn key_cell(entry: &CollectionTrack, state: AnalysisState<'_>) -> SharedString {
    let analysed = state
        .found()
        .and_then(|analysis| analysis.key)
        .map(|key| key.camelot());
    cell(entry, &entry.row.key, state, analysed)
}

/// The energy a track shows: blank while it waits, a dash when it has none.
pub fn energy_cell(state: AnalysisState<'_>) -> SharedString {
    match state {
        AnalysisState::Waiting => SharedString::default(),
        AnalysisState::Failed => "\u{2013}".into(),
        AnalysisState::Found(analysis) => match &analysis.energy {
            Some(energy) => energy.rating.to_string().into(),
            None => "\u{2013}".into(),
        },
    }
}

/// The key a track is mixed in, for transposing it on the deck.
pub fn track_key(entry: &CollectionTrack, state: AnalysisState<'_>) -> Option<Key> {
    let own = entry.track.key.as_deref().and_then(Key::parse_file_key);
    preferred(
        entry.origin,
        own,
        state.found().and_then(|analysis| analysis.key),
    )
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use dubplate_prep::{AnalysedGrid, TrackEnergy};

    use super::*;

    #[derive(Default)]
    struct Analyser {
        requests: RefCell<Vec<(PathBuf, AnalysisPriority)>>,
        answers: RefCell<Vec<AnalysedTrack>>,
    }

    impl TrackAnalyser for Rc<Analyser> {
        fn request(&self, tracks: Vec<PathBuf>, priority: AnalysisPriority, _revision: u64) {
            self.requests
                .borrow_mut()
                .extend(tracks.into_iter().map(|path| (path, priority)));
        }

        fn next_result(&self) -> Option<AnalysedTrack> {
            self.answers.borrow_mut().pop()
        }
    }

    fn analysis(bpm: Option<f32>, key: Option<&str>, energy: Option<u8>) -> AudioAnalysis {
        AudioAnalysis {
            duration_seconds: 300.0,
            bpm,
            beat_grid: bpm.map(|bpm| AnalysedGrid {
                bpm: f64::from(bpm),
                first_downbeat_seconds: 0.1,
            }),
            key: key.and_then(Key::parse_file_key),
            energy: energy.map(|rating| TrackEnergy {
                rating,
                score: 0.5,
                curve: Vec::new(),
            }),
        }
    }

    fn answer(path: &str, outcome: Result<AudioAnalysis, &str>) -> AnalysedTrack {
        AnalysedTrack {
            path: PathBuf::from(path),
            revision: 0,
            outcome: outcome.map_err(str::to_owned),
        }
    }

    #[test]
    fn a_track_is_asked_once_and_again_only_to_move_it_up() {
        let analyser = Rc::new(Analyser::default());
        let mut analyses = TrackAnalyses::new(Some(Box::new(Rc::clone(&analyser))));
        let path = Path::new("/Music/a.mp3");
        analyses.request([path], AnalysisPriority::Library);
        analyses.request([path, path], AnalysisPriority::Library);
        analyses.request([path], AnalysisPriority::Set);
        analyses.request([path], AnalysisPriority::Library);
        assert_eq!(
            *analyser.requests.borrow(),
            [
                (path.to_path_buf(), AnalysisPriority::Library),
                (path.to_path_buf(), AnalysisPriority::Set),
            ]
        );
        assert_eq!(analyses.waiting(), 1);
        assert_eq!(analyses.state(path), AnalysisState::Waiting);
    }

    #[test]
    fn answers_are_kept_and_tracks_answered_are_not_asked_again() {
        let analyser = Rc::new(Analyser::default());
        let mut analyses = TrackAnalyses::new(Some(Box::new(Rc::clone(&analyser))));
        let paths = ["/a.mp3", "/b.mp3"].map(Path::new);
        analyses.request(paths, AnalysisPriority::Library);
        analyser.answers.borrow_mut().extend([
            answer("/a.mp3", Ok(analysis(Some(122.0), Some("8A"), Some(7)))),
            answer("/b.mp3", Err("not audio")),
        ]);
        let mut answered = analyses.collect();
        answered.sort_by(|first, second| first.0.cmp(&second.0));
        assert_eq!(answered.len(), 2);
        assert_eq!(
            answered[0]
                .1
                .as_ref()
                .ok()
                .and_then(|analysis| analysis.bpm),
            Some(122.0)
        );
        assert_eq!(answered[1].1.as_ref().unwrap_err(), "not audio");
        assert_eq!(analyses.waiting(), 0);
        assert_eq!(analyses.state(paths[1]), AnalysisState::Failed);
        assert_eq!(analyses.found.len(), 1);
        analyser.requests.borrow_mut().clear();
        analyses.request(paths, AnalysisPriority::Set);
        assert!(analyser.requests.borrow().is_empty());
    }

    #[test]
    fn rescan_revalidates_successes_retries_failures_and_ignores_old_answers() {
        let analyser = Rc::new(Analyser::default());
        let mut analyses = TrackAnalyses::new(Some(Box::new(Rc::clone(&analyser))));
        let paths = ["/a.mp3", "/b.mp3"].map(Path::new);
        analyses.request(paths, AnalysisPriority::Library);
        analyser.answers.borrow_mut().extend([
            answer("/a.mp3", Ok(analysis(Some(122.0), None, None))),
            answer("/b.mp3", Err("disconnected")),
        ]);
        analyses.collect();
        analyser.requests.borrow_mut().clear();
        analyses.refresh();
        analyses.request(paths, AnalysisPriority::Library);
        assert_eq!(analyser.requests.borrow().len(), 2);
        analyser
            .answers
            .borrow_mut()
            .push(answer("/a.mp3", Ok(analysis(Some(99.0), None, None))));
        assert!(analyses.collect().is_empty());
        assert_eq!(analyses.waiting(), 2);
        let mut fresh = answer("/a.mp3", Ok(analysis(Some(124.0), None, None)));
        fresh.revision = 1;
        analyser.answers.borrow_mut().push(fresh);
        assert_eq!(analyses.collect().len(), 1);
        assert_eq!(analyses.get(paths[0]).unwrap().bpm, Some(124.0));
    }

    #[test]
    fn without_an_analyser_nothing_waits() {
        let mut analyses = TrackAnalyses::new(None);
        analyses.request([Path::new("/a.mp3")], AnalysisPriority::Set);
        assert_eq!(analyses.waiting(), 0);
        assert!(analyses.collect().is_empty());
    }

    fn rekordbox_track() -> CollectionTrack {
        let mut track = dubplate_prep::file_track(Path::new("/Music/rekordbox.mp3"));
        track.bpm = Some(124.0);
        track.key = Some("5A".to_owned());
        CollectionTrack::for_tests(track, TrackOrigin::Rekordbox)
    }

    fn folder_track(tag_bpm: Option<f64>, tag_key: Option<&str>) -> CollectionTrack {
        let mut track = dubplate_prep::file_track(Path::new("/Music/folder.mp3"));
        track.bpm = tag_bpm;
        track.key = tag_key.map(str::to_owned);
        CollectionTrack::for_tests(track, TrackOrigin::Folder)
    }

    #[test]
    fn a_folder_track_shows_the_analysis_and_is_blank_while_it_waits() {
        let track = folder_track(Some(120.0), Some("Am"));
        let found = analysis(Some(113.0), Some("4A"), Some(6));
        assert_eq!(
            bpm_cell(&track, AnalysisState::Found(&found)).as_ref(),
            "113.00"
        );
        assert_eq!(
            key_cell(&track, AnalysisState::Found(&found)).as_ref(),
            "4A"
        );
        assert_eq!(energy_cell(AnalysisState::Found(&found)).as_ref(), "6");
        assert_eq!(bpm_cell(&track, AnalysisState::Waiting).as_ref(), "");
        assert_eq!(key_cell(&track, AnalysisState::Waiting).as_ref(), "");
        assert_eq!(energy_cell(AnalysisState::Waiting).as_ref(), "");
    }

    #[test]
    fn a_folder_track_the_analysis_found_nothing_in_keeps_its_tags() {
        let track = folder_track(Some(120.0), Some("Am"));
        let silent = analysis(None, None, None);
        assert_eq!(
            bpm_cell(&track, AnalysisState::Found(&silent)).as_ref(),
            "120.00"
        );
        assert_eq!(key_cell(&track, AnalysisState::Failed).as_ref(), "Am");
        assert_eq!(
            energy_cell(AnalysisState::Found(&silent)).as_ref(),
            "\u{2013}"
        );
        assert_eq!(energy_cell(AnalysisState::Failed).as_ref(), "\u{2013}");
    }

    #[test]
    fn a_rekordbox_track_keeps_rekordbox_values_and_is_filled_in_where_it_has_none() {
        let track = rekordbox_track();
        let found = analysis(Some(123.0), Some("6A"), Some(5));
        assert_eq!(
            bpm_cell(&track, AnalysisState::Found(&found)).as_ref(),
            "124.00"
        );
        assert_eq!(bpm_cell(&track, AnalysisState::Waiting).as_ref(), "124.00");
        assert_eq!(key_cell(&track, AnalysisState::Waiting).as_ref(), "5A");
        let mut unanalysed = rekordbox_track();
        unanalysed.row.bpm = SharedString::default();
        unanalysed.row.key = SharedString::default();
        unanalysed.track.key = None;
        assert_eq!(
            bpm_cell(&unanalysed, AnalysisState::Found(&found)).as_ref(),
            "123.00"
        );
        assert_eq!(
            track_key(&unanalysed, AnalysisState::Found(&found)),
            Key::parse_file_key("6A")
        );
        assert_eq!(
            track_key(&track, AnalysisState::Found(&found)),
            Key::parse_file_key("5A")
        );
    }
}
