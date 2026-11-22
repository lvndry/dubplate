//! A track's energy: Euphoria's rating from 1 to 10 and its curve over time.

use std::path::{Path, PathBuf};

use thiserror::Error;

/// One window of a track's energy curve.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EnergyWindow {
    /// Where the window starts, in seconds from the start of the track.
    pub start_seconds: f32,
    /// The window's rating from 1 to 10, or None when the window is silent.
    pub rating: Option<u8>,
}

/// The energy of one track, as rated by Euphoria.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackEnergy {
    /// The track's rating, from 1 (calm) to 10 (intense).
    pub rating: u8,
    /// Position of the rating on the 0.0 to 1.0 scale, to order tracks that share a rating.
    pub score: f32,
    /// The rating over the track, window by window.
    pub curve: Vec<EnergyWindow>,
}

/// What went wrong while measuring a track's energy.
#[derive(Debug, Error)]
pub enum EnergyError {
    /// The file could not be decoded to mono samples.
    #[error("could not decode {path}: {message}")]
    Decode {
        /// The audio file that was read.
        path: PathBuf,
        /// What the decoder reported.
        message: String,
    },
    /// The audio was decoded but the energy analysis failed.
    #[error("could not rate the energy of {path}: {message}")]
    Analysis {
        /// The audio file that was rated.
        path: PathBuf,
        /// What the analysis reported.
        message: String,
    },
}

/// Decodes `path` and rates its energy with Euphoria.
///
/// Returns Ok(None) for silence and for audio too short to analyse. When a
/// tempo is supplied and Euphoria rejects it, the track is rated once more
/// without a tempo rather than failing.
pub fn measure_track_energy(
    path: &Path,
    bpm: Option<f32>,
) -> Result<Option<TrackEnergy>, EnergyError> {
    let audio = euphoria_core::audio::decode_mono(path).map_err(|error| EnergyError::Decode {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;

    let (rating, tempo) = rate_energy_with_tempo_fallback(&audio, path, bpm)?;
    let Some(rating) = rating else {
        return Ok(None);
    };
    let curve = euphoria_core::energy::energy_curve(&audio, tempo)
        .map_err(|error| analysis_error(path, error))?;
    Ok(Some(TrackEnergy {
        rating: rating.rating,
        score: rating.score,
        curve: curve
            .into_iter()
            .map(|point| EnergyWindow {
                start_seconds: point.start_seconds,
                rating: point.rating.map(|rating| rating.rating),
            })
            .collect(),
    }))
}

/// Rates the audio at `bpm`; when Euphoria rejects the tempo, rates it once
/// more without one. Returns the rating and the tempo that produced it.
fn rate_energy_with_tempo_fallback(
    audio: &euphoria_core::AudioBuffer,
    path: &Path,
    bpm: Option<f32>,
) -> Result<(Option<euphoria_core::energy::EnergyRating>, Option<f32>), EnergyError> {
    match euphoria_core::energy::rate_energy(audio, bpm) {
        Ok(rating) => Ok((rating, bpm)),
        Err(_rejected) if bpm.is_some() => {
            let rating = euphoria_core::energy::rate_energy(audio, None)
                .map_err(|retry| analysis_error(path, retry))?;
            Ok((rating, None))
        }
        Err(error) => Err(analysis_error(path, error)),
    }
}

/// Carries `path` into an analysis error.
fn analysis_error(path: &Path, error: euphoria_core::EuphoriaError) -> EnergyError {
    EnergyError::Analysis {
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}
