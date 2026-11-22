//! The shape a set's energy should follow, from the energies the analysis rated: a warm-up, a
//! peak three quarters in, and a landing.

use std::time::Duration;

use dubplate_prep::EnergyDirection;

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
    use super::*;

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
