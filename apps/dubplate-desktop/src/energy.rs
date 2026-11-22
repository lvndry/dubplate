//! Each track's energy from 1 to 10, as the energy analyser rates it in the background, and the
//! shape a set's energy should follow: a warm-up, a peak three quarters in, and a landing.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use dubplate_prep::{AnalysisQueue, EnergyDirection, Priority};

/// How soon a track is rated: the set's tracks before the rest of the library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EnergyPriority {
    /// A track of the set being prepared.
    Set,
    /// Any other track of the library.
    Library,
}

/// A track's energy as the analyser rated it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnergyReading {
    /// From 1, calm, to 10, intense.
    pub rating: u8,
}

/// The analyser's answer for one track.
#[derive(Debug)]
pub struct AnalysedTrack {
    /// The track's audio file.
    pub path: PathBuf,
    /// Its energy, `None` when it has none to rate, or why it could not be rated.
    pub outcome: Result<Option<EnergyReading>, String>,
}

/// What rates tracks off the interface thread. Kept this small so the analyser's own types stay
/// behind it.
pub trait EnergyAnalyser {
    /// Asks for each track, by its file, to be rated, sooner for an earlier `priority`.
    fn request(&self, tracks: Vec<PathBuf>, priority: EnergyPriority);
    /// The next answer that has arrived, without waiting.
    fn next_result(&self) -> Option<AnalysedTrack>;
}

impl EnergyAnalyser for AnalysisQueue {
    fn request(&self, tracks: Vec<PathBuf>, priority: EnergyPriority) {
        let priority = match priority {
            EnergyPriority::Set => Priority::Set,
            EnergyPriority::Library => Priority::Library,
        };
        self.request_all(tracks.into_iter().map(|path| (path, priority)));
    }

    fn next_result(&self) -> Option<AnalysedTrack> {
        let result = self.results().try_recv().ok()?;
        Some(AnalysedTrack {
            path: result.path,
            outcome: result.outcome.map(|analysis| {
                analysis.energy.map(|energy| EnergyReading {
                    rating: energy.rating,
                })
            }),
        })
    }
}

/// The energies known so far and the tracks still waiting for one.
#[derive(Default)]
pub struct EnergyRatings {
    analyser: Option<Box<dyn EnergyAnalyser>>,
    readings: HashMap<PathBuf, EnergyReading>,
    waiting: HashMap<PathBuf, EnergyPriority>,
    unrated: HashSet<PathBuf>,
}

impl EnergyRatings {
    /// Ratings from `analyser`, or none at all without one.
    pub fn new(analyser: Option<Box<dyn EnergyAnalyser>>) -> Self {
        Self {
            analyser,
            ..Self::default()
        }
    }

    /// Every energy rated so far.
    pub fn readings(&self) -> impl Iterator<Item = (&Path, EnergyReading)> {
        self.readings
            .iter()
            .map(|(path, reading)| (path.as_path(), *reading))
    }

    /// How many tracks are waiting for their energy.
    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }

    /// Asks for each track, by its file, to be rated at `priority`, leaving out the ones rated
    /// already or waiting at the same priority or an earlier one.
    pub fn request<'tracks>(
        &mut self,
        tracks: impl IntoIterator<Item = &'tracks Path>,
        priority: EnergyPriority,
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
            if asked || self.readings.contains_key(path) || self.unrated.contains(path) {
                continue;
            }
            self.waiting.insert(path.to_path_buf(), priority);
            wanted.push(path.to_path_buf());
        }
        if wanted.is_empty() {
            return;
        }
        analyser.request(wanted, priority);
    }

    /// Takes the answers that have arrived: each track with its energy, or `None` when it has
    /// none or could not be read.
    pub fn collect(&mut self) -> Vec<(PathBuf, Option<EnergyReading>)> {
        let mut answered = Vec::new();
        let Some(analyser) = &self.analyser else {
            return answered;
        };
        while let Some(answer) = analyser.next_result() {
            self.waiting.remove(&answer.path);
            let reading = answer.outcome.ok().flatten();
            match reading {
                Some(reading) => self.readings.insert(answer.path.clone(), reading),
                None => {
                    self.unrated.insert(answer.path.clone());
                    None
                }
            };
            answered.push((answer.path, reading));
        }
        answered
    }
}

/// The lowest energy.
pub const LOWEST_ENERGY: f32 = 1.0;
/// The highest energy.
pub const HIGHEST_ENERGY: f32 = 10.0;
/// Where the target starts and peaks until the library has enough ratings to read its own.
const TARGET_START: f32 = 4.0;
const TARGET_PEAK: f32 = 9.0;
/// How far from the warm-up to the peak the set lands.
const LANDING_SHARE: f32 = 0.6;
/// The share of the library rated at or below the warm-up energy, and at or below the peak.
const WARM_UP_SHARE: f32 = 0.2;
const PEAK_SHARE: f32 = 0.9;
/// The least room between warm-up and peak, so a library rated in a narrow band still climbs.
const MINIMUM_SPREAD: f32 = 2.0;
/// How many rated tracks the library needs before its own range replaces the default.
const MINIMUM_RATED: usize = 20;
/// How far into the set the target peaks.
const PEAK_AT: f32 = 0.75;
/// How far a track's energy may be from the target before the next one should move it.
const ON_TARGET: f32 = 1.0;

/// The energies a set climbs between, read from the DJ's own library: the energy model rates
/// some libraries low overall, so a fixed 4-to-9 target would sit above every track.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnergyRange {
    warm_up: f32,
    peak: f32,
}

impl Default for EnergyRange {
    fn default() -> Self {
        Self {
            warm_up: TARGET_START,
            peak: TARGET_PEAK,
        }
    }
}

impl EnergyRange {
    /// The warm-up is the energy a fifth of the library sits at or below, the peak the energy
    /// nine tenths sit at or below, kept at least two apart. Below twenty ratings, 4 to 9.
    pub fn of_library(ratings: impl IntoIterator<Item = f32>) -> Self {
        let mut sorted: Vec<f32> = ratings
            .into_iter()
            .filter(|rating| rating.is_finite())
            .collect();
        if sorted.len() < MINIMUM_RATED {
            return Self::default();
        }
        sorted.sort_by(f32::total_cmp);
        let last = sorted.len() - 1;
        let at_share = |share: f32| sorted[(last as f32 * share).round() as usize];
        let peak = at_share(PEAK_SHARE)
            .max(at_share(WARM_UP_SHARE) + MINIMUM_SPREAD)
            .min(HIGHEST_ENERGY);
        let warm_up = at_share(WARM_UP_SHARE)
            .min(peak - MINIMUM_SPREAD)
            .max(LOWEST_ENERGY);
        Self { warm_up, peak }
    }

    fn landing(self) -> f32 {
        self.warm_up + (self.peak - self.warm_up) * LANDING_SHARE
    }
}

/// The energy the set should have `fraction` of the way through it: rising from the library's
/// warm-up energy to its peak three quarters in, then coming down to land.
pub fn target_energy(fraction: f32, range: EnergyRange) -> f32 {
    let fraction = fraction.clamp(0.0, 1.0);
    if fraction <= PEAK_AT {
        range.warm_up + (range.peak - range.warm_up) * ease(fraction / PEAK_AT)
    } else {
        range.peak + (range.landing() - range.peak) * ease((fraction - PEAK_AT) / (1.0 - PEAK_AT))
    }
}

/// Smooth both ends of a ramp from 0 to 1.
fn ease(progress: f32) -> f32 {
    progress * progress * (3.0 - 2.0 * progress)
}

/// One track's stretch of the set's energy curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveStep {
    /// Where the track starts, as a fraction of the curve's width.
    pub start: f32,
    /// Where it ends.
    pub end: f32,
    /// Its energy, once rated.
    pub rating: Option<f32>,
}

/// The set's energy track by track, `tracks` being each track's length and energy in order. The
/// curve spans the target length, or the set's length when the set runs over.
pub fn set_curve(tracks: &[(Option<Duration>, Option<f32>)], target: Duration) -> Vec<CurveStep> {
    let total: Duration = tracks.iter().filter_map(|(length, _)| *length).sum();
    let span = total.max(target).as_secs_f32();
    if span <= 0.0 {
        return Vec::new();
    }
    let mut elapsed = 0.0;
    tracks
        .iter()
        .map(|(length, rating)| {
            let start = elapsed / span;
            elapsed += length.map_or(0.0, |length| length.as_secs_f32());
            CurveStep {
                start,
                end: elapsed / span,
                rating: *rating,
            }
        })
        .collect()
}

/// Which way the next track should take the energy after a track rated `rating` that ends
/// `fraction` of the way through the set, to follow the target.
pub fn direction_toward_target(
    rating: Option<f32>,
    fraction: f32,
    range: EnergyRange,
) -> EnergyDirection {
    let Some(rating) = rating else {
        return EnergyDirection::Same;
    };
    let wanted = target_energy(fraction, range);
    if wanted - rating >= ON_TARGET {
        EnergyDirection::Up
    } else if rating - wanted >= ON_TARGET {
        EnergyDirection::Down
    } else {
        EnergyDirection::Same
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    #[derive(Default)]
    struct Analyser {
        requests: RefCell<Vec<(PathBuf, EnergyPriority)>>,
        answers: RefCell<Vec<AnalysedTrack>>,
    }

    impl EnergyAnalyser for Rc<Analyser> {
        fn request(&self, tracks: Vec<PathBuf>, priority: EnergyPriority) {
            self.requests
                .borrow_mut()
                .extend(tracks.into_iter().map(|path| (path, priority)));
        }

        fn next_result(&self) -> Option<AnalysedTrack> {
            self.answers.borrow_mut().pop()
        }
    }

    fn answer(path: &str, outcome: Result<Option<u8>, &str>) -> AnalysedTrack {
        AnalysedTrack {
            path: PathBuf::from(path),
            outcome: outcome
                .map(|rating| rating.map(|rating| EnergyReading { rating }))
                .map_err(str::to_owned),
        }
    }

    #[test]
    fn a_track_is_asked_once_and_again_only_to_move_it_up() {
        let analyser = Rc::new(Analyser::default());
        let mut ratings = EnergyRatings::new(Some(Box::new(Rc::clone(&analyser))));
        let path = Path::new("/Music/a.mp3");
        ratings.request([path], EnergyPriority::Library);
        ratings.request([path, path], EnergyPriority::Library);
        ratings.request([path], EnergyPriority::Set);
        ratings.request([path], EnergyPriority::Library);
        assert_eq!(
            *analyser.requests.borrow(),
            [
                (path.to_path_buf(), EnergyPriority::Library),
                (path.to_path_buf(), EnergyPriority::Set),
            ]
        );
        assert_eq!(ratings.waiting(), 1);
    }

    #[test]
    fn answers_are_kept_and_tracks_answered_are_not_asked_again() {
        let analyser = Rc::new(Analyser::default());
        let mut ratings = EnergyRatings::new(Some(Box::new(Rc::clone(&analyser))));
        ratings.request(
            ["/a.mp3", "/b.mp3", "/c.mp3"].map(Path::new),
            EnergyPriority::Library,
        );
        analyser.answers.borrow_mut().extend([
            answer("/a.mp3", Ok(Some(7))),
            answer("/b.mp3", Ok(None)),
            answer("/c.mp3", Err("not audio")),
        ]);
        let mut answered = ratings.collect();
        answered.sort_by(|first, second| first.0.cmp(&second.0));
        assert_eq!(
            answered,
            [
                (PathBuf::from("/a.mp3"), Some(EnergyReading { rating: 7 })),
                (PathBuf::from("/b.mp3"), None),
                (PathBuf::from("/c.mp3"), None),
            ]
        );
        assert_eq!(ratings.waiting(), 0);
        assert_eq!(
            ratings.readings().collect::<Vec<_>>(),
            [(Path::new("/a.mp3"), EnergyReading { rating: 7 })]
        );
        analyser.requests.borrow_mut().clear();
        ratings.request(
            ["/a.mp3", "/b.mp3", "/c.mp3"].map(Path::new),
            EnergyPriority::Set,
        );
        assert!(analyser.requests.borrow().is_empty());
    }

    #[test]
    fn without_an_analyser_nothing_waits() {
        let mut ratings = EnergyRatings::new(None);
        ratings.request([Path::new("/a.mp3")], EnergyPriority::Set);
        assert_eq!(ratings.waiting(), 0);
        assert!(ratings.collect().is_empty());
    }

    #[test]
    fn the_target_warms_up_peaks_three_quarters_in_and_lands() {
        let range = EnergyRange::default();
        assert!((target_energy(0.0, range) - 4.0).abs() < 1e-6);
        assert!((target_energy(PEAK_AT, range) - 9.0).abs() < 1e-6);
        assert!((target_energy(1.0, range) - 7.0).abs() < 1e-6);
        assert!(target_energy(0.3, range) < target_energy(0.5, range));
        assert!(target_energy(0.9, range) < target_energy(0.8, range));
        assert!((target_energy(-1.0, range) - 4.0).abs() < 1e-6);
    }

    #[test]
    fn the_target_follows_a_library_the_model_rates_low() {
        let ratings = [1.0, 1.0, 2.0, 2.0, 2.0, 3.0, 3.0, 3.0, 3.0, 3.0]
            .into_iter()
            .chain([2.0, 3.0, 4.0, 4.0, 4.0, 5.0, 5.0, 6.0, 6.0, 2.0]);
        let range = EnergyRange::of_library(ratings);
        assert!((target_energy(0.0, range) - 2.0).abs() < 1e-6);
        assert!((target_energy(PEAK_AT, range) - 5.0).abs() < 1e-6);
        assert_eq!(
            direction_toward_target(Some(3.0), 0.05, range),
            EnergyDirection::Same,
            "a 3 is a fine warm-up in this library"
        );
    }

    #[test]
    fn a_narrow_or_small_library_keeps_room_to_climb() {
        let narrow = EnergyRange::of_library(std::iter::repeat_n(3.0, 40));
        assert!(target_energy(PEAK_AT, narrow) - target_energy(0.0, narrow) >= 2.0 - 1e-6);
        assert_eq!(
            EnergyRange::of_library([9.0, 9.0, 9.0]),
            EnergyRange::default()
        );
    }

    #[test]
    fn the_curve_spans_the_target_or_the_set_when_it_runs_over() {
        let minutes = |count: u64| Some(Duration::from_secs(count * 60));
        let steps = set_curve(
            &[(minutes(30), Some(5.0)), (minutes(30), None)],
            minutes(120).unwrap(),
        );
        assert_eq!(steps.len(), 2);
        assert!((steps[0].end - 0.25).abs() < 1e-6);
        assert!((steps[1].end - 0.5).abs() < 1e-6);
        assert_eq!(steps[1].rating, None);
        let over = set_curve(
            &[(minutes(90), Some(5.0)), (minutes(60), Some(8.0))],
            minutes(120).unwrap(),
        );
        assert!((over[1].end - 1.0).abs() < 1e-6);
        assert!(set_curve(&[(None, Some(5.0))], Duration::ZERO).is_empty());
    }

    #[test]
    fn the_next_track_moves_the_energy_toward_the_target() {
        let range = EnergyRange::default();
        assert_eq!(
            direction_toward_target(Some(4.0), 0.5, range),
            EnergyDirection::Up
        );
        assert_eq!(
            direction_toward_target(Some(9.0), 0.75, range),
            EnergyDirection::Same
        );
        assert_eq!(
            direction_toward_target(Some(9.0), 0.05, range),
            EnergyDirection::Down
        );
        assert_eq!(
            direction_toward_target(None, 0.5, range),
            EnergyDirection::Same
        );
    }
}
