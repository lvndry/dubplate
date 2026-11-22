//! A track's energy: Euphoria's rating from 1 to 10 and its curve over time.

use euphoria_core::{AudioBuffer, EuphoriaError};

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

/// Rates the energy of `audio` at `tempo`, with its curve. None for silence and for audio too
/// short to rate. When Euphoria rejects the tempo, the track is rated once more without one
/// rather than failing.
pub(crate) fn rate_track_energy(
    audio: &AudioBuffer,
    tempo: Option<f32>,
) -> Result<Option<TrackEnergy>, EuphoriaError> {
    let (rating, tempo) = match euphoria_core::energy::rate_energy(audio, tempo) {
        Ok(rating) => (rating, tempo),
        Err(_rejected) if tempo.is_some() => {
            (euphoria_core::energy::rate_energy(audio, None)?, None)
        }
        Err(error) => return Err(error),
    };
    let Some(rating) = rating else {
        return Ok(None);
    };
    let curve = euphoria_core::energy::energy_curve(audio, tempo)?;
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
