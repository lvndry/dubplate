//! Dubplate's own analysis of a track's audio: its tempo, beat grid, key and energy, all from
//! one decode, for the tracks rekordbox never analysed.
//!
//! The detectors are Euphoria's, put together in the order and with the tempo range Euphoria's
//! own analysis uses, so a track gets the tempo, grid and key Euphoria would give it.

use std::path::{Path, PathBuf};
use std::time::Duration;

use dubplate_rekordbox::analysis::{Beat, BeatGrid};
use euphoria_core::detect::{StratumDetector, detect_beat_grid, read_key};
use euphoria_core::tempo_range::TempoRange;
use euphoria_core::{AudioBuffer, EuphoriaError, Key};
use thiserror::Error;

use crate::downbeat::settle_grid;
use crate::energy::{TrackEnergy, rate_track_energy};
use crate::grid::BEATS_PER_BAR;
use crate::loudness::{Lufs, integrated_loudness};

/// Raised whenever a change to how Dubplate puts Euphoria's detectors together can change what
/// an analysis reports, so analyses cached before it are made again.
const PIPELINE_VERSION: u32 = 4;

/// The version of everything that decides an analysis: Euphoria's detectors, its energy model
/// and the way Dubplate combines them. A cached analysis made under another version is stale.
pub fn analysis_version() -> String {
    format!(
        "{}+energy{}+dubplate{PIPELINE_VERSION}",
        euphoria_core::ANALYZER_VERSION,
        euphoria_core::ENERGY_MODEL_VERSION,
    )
}

/// Where the beats and bars of an analysed track fall, at one constant tempo.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnalysedGrid {
    /// The tempo of the grid in beats per minute, counted in the octave of the track's tempo.
    pub bpm: f64,
    /// Seconds from the start of the track to the first downbeat, within the first bar.
    pub first_downbeat_seconds: f64,
}

/// The length of `duration` in whole milliseconds, clamped to what an `i64` holds.
pub(crate) fn millis_i64(duration: Duration) -> i64 {
    duration.as_millis().min(i64::MAX as u128) as i64
}

impl AnalysedGrid {
    /// Euphoria's fitted grid counted in the octave nearest to `bpm`, the tempo reported for the
    /// track, with its first downbeat kept within the first bar. Euphoria's analysis does the
    /// same, so a refined tempo that crosses an octave boundary still counts the same beats.
    fn counted_near(grid: &euphoria_core::BeatGrid, bpm: f64) -> Self {
        let octaves = (bpm / grid.bpm).log2().round() as i32;
        let counted_bpm = grid.bpm * 2.0_f64.powi(octaves);
        let bar_seconds = 60.0 / counted_bpm * f64::from(BEATS_PER_BAR);
        Self {
            bpm: counted_bpm,
            first_downbeat_seconds: grid.first_downbeat_seconds.rem_euclid(bar_seconds),
        }
    }

    /// Seconds from one beat to the next.
    pub fn beat_seconds(&self) -> f64 {
        60.0 / self.bpm
    }

    /// The grid as rekordbox keeps one: every beat from the start of the track to
    /// `duration_seconds`, numbered in its bar from the downbeat. The decks, the waveform, the
    /// track check and the DJ's grid corrections then use it as they use rekordbox's.
    pub fn rekordbox_grid(&self, duration_seconds: f64) -> BeatGrid {
        let usable = self.bpm.is_finite()
            && self.bpm > 0.0
            && self.first_downbeat_seconds.is_finite()
            && duration_seconds.is_finite();
        if !usable {
            return BeatGrid { beats: Vec::new() };
        }
        let beat_seconds = self.beat_seconds();
        let tempo_hundredths = (self.bpm * 100.0).round().clamp(0.0, f64::from(u16::MAX)) as u16;
        let beats_before_downbeat = (self.first_downbeat_seconds / beat_seconds).floor() as i64;
        let last_beat = ((duration_seconds - self.first_downbeat_seconds) / beat_seconds).floor();
        let beats = (-beats_before_downbeat..=last_beat as i64)
            .filter_map(|index| {
                let seconds = self.first_downbeat_seconds + index as f64 * beat_seconds;
                let time_ms = u32::try_from((seconds * 1000.0).round() as i64).ok()?;
                let in_bar = index.rem_euclid(i64::from(BEATS_PER_BAR)) + 1;
                Some(Beat {
                    beat_in_bar: u16::try_from(in_bar).ok()?,
                    tempo_hundredths,
                    time_ms,
                })
            })
            .collect();
        BeatGrid { beats }
    }
}

/// What Dubplate's analysis found in one track.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioAnalysis {
    /// The length of the decoded audio, in seconds.
    pub duration_seconds: f32,
    /// The tempo in beats per minute, the beat grid's when there is one, from about 90 to 180,
    /// or None when the track has no detectable tempo, such as silence.
    pub bpm: Option<f32>,
    /// Where the beats and bars fall, or None when the track has no tempo or no clear beat.
    pub beat_grid: Option<AnalysedGrid>,
    /// The key, or None when the track has no detectable key.
    pub key: Option<Key>,
    /// The energy, or None when there is nothing to rate, such as silence.
    pub energy: Option<TrackEnergy>,
    /// How loud the track sounds over its length, or None for silence. Auto Gain levels the
    /// track from it.
    pub loudness: Option<Lufs>,
}

impl AudioAnalysis {
    /// The beat grid as rekordbox keeps one, through the whole track. See
    /// [`AnalysedGrid::rekordbox_grid`].
    pub fn rekordbox_grid(&self) -> Option<BeatGrid> {
        self.beat_grid
            .map(|grid| grid.rekordbox_grid(f64::from(self.duration_seconds)))
    }
}

/// What went wrong while analysing a track.
#[derive(Debug, Error)]
pub enum AnalysisError {
    /// The file could not be decoded to mono samples.
    #[error("could not decode {path}: {message}")]
    Decode {
        /// The audio file that was read.
        path: PathBuf,
        /// What the decoder reported.
        message: String,
    },
    /// The audio was decoded but a detector failed.
    #[error("could not analyse {path}: {message}")]
    Analysis {
        /// The audio file that was analysed.
        path: PathBuf,
        /// What the detector reported.
        message: String,
    },
}

/// Decodes the file at `path` once and analyses it. See [`analyse_audio`].
pub fn analyse_track(path: &Path) -> Result<AudioAnalysis, AnalysisError> {
    let audio = euphoria_core::audio::decode_mono(path).map_err(|error| AnalysisError::Decode {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    analyse_audio(&audio).map_err(|error| AnalysisError::Analysis {
        path: path.to_path_buf(),
        message: error.to_string(),
    })
}

/// Finds the tempo, key, energy and beat grid of `audio`, in Euphoria's order.
///
/// The tempo detector estimates the tempo and chooses its octave; the energy is rated and the
/// grid fitted at that estimate placed between 90 and 180 BPM, Euphoria's default range. When
/// the grid fits the beats well its tempo, whole when a whole tempo fits as well, replaces the
/// estimate, in the estimate's octave, and the grid is counted at that tempo.
///
/// The track's tempo is then the grid's, so the library shows the tempo a deck plays and syncs
/// the track at. Euphoria's own analysis keeps the estimate when the grid fits loosely; the
/// grid's tempo is the closer of the two there, since it was fitted to the beats. A track
/// without a grid keeps the estimate.
///
/// Dubplate then settles the grid's beats and bars on the kicks (see [`settle_grid`]): Euphoria
/// often puts a dance track's bars a beat or two off, or its beats on the off-beat.
pub fn analyse_audio(audio: &AudioBuffer) -> Result<AudioAnalysis, EuphoriaError> {
    let tempo_reading = StratumDetector.read_tempo(audio)?;
    let key = read_key(audio)?.key;
    let tempo = tempo_reading.map(|reading| reading.tempo_in(TempoRange::DEFAULT));
    let energy = rate_track_energy(audio, tempo)?;
    let fitted = tempo.and_then(|tempo| detect_beat_grid(audio, f64::from(tempo)));
    let estimate =
        tempo_reading.map(|reading| TempoRange::DEFAULT.fold(reading.refined_by(fitted.as_ref())));
    let beat_grid = fitted
        .zip(estimate)
        .map(|(grid, estimate)| AnalysedGrid::counted_near(&grid, f64::from(estimate)))
        .map(|grid| settle_grid(audio, grid));
    let bpm = beat_grid.map(|grid| grid.bpm as f32).or(estimate);
    Ok(AudioAnalysis {
        duration_seconds: audio.duration_seconds(),
        bpm,
        beat_grid,
        key,
        energy,
        loudness: integrated_loudness(audio),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_grid_lists_every_beat_numbered_from_the_downbeat() {
        let grid = AnalysedGrid {
            bpm: 120.0,
            first_downbeat_seconds: 1.25,
        };
        let beats = grid.rekordbox_grid(3.0).beats;
        let shown: Vec<(u32, u16)> = beats
            .iter()
            .map(|beat| (beat.time_ms, beat.beat_in_bar))
            .collect();
        assert_eq!(
            shown,
            [
                (250, 3),
                (750, 4),
                (1250, 1),
                (1750, 2),
                (2250, 3),
                (2750, 4)
            ]
        );
        assert!(beats.iter().all(|beat| beat.tempo_hundredths == 12_000));
    }

    #[test]
    fn a_grid_counted_near_a_tempo_moves_by_octaves_and_keeps_the_downbeat_in_the_first_bar() {
        let fitted = euphoria_core::BeatGrid {
            first_downbeat_seconds: 3.0,
            bpm: 61.0,
            salience: 10.0,
            drift_seconds: None,
        };
        let counted = AnalysedGrid::counted_near(&fitted, 122.0);
        assert!((counted.bpm - 122.0).abs() < 1e-9);
        let bar_seconds = 4.0 * 60.0 / 122.0;
        assert!(counted.first_downbeat_seconds < bar_seconds);
        let bars_moved = (3.0 - counted.first_downbeat_seconds) / bar_seconds;
        assert!((bars_moved - bars_moved.round()).abs() < 1e-9);
    }

    #[test]
    fn an_unusable_grid_has_no_beats() {
        let broken = AnalysedGrid {
            bpm: f64::NAN,
            first_downbeat_seconds: 0.0,
        };
        assert!(broken.rekordbox_grid(60.0).beats.is_empty());
    }
}
