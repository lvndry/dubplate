use euphoria_core::harmony::words::relation_name;
use euphoria_core::harmony::{EnergyDirection, MixOptions, next_tracks};

use crate::track::PrepTrack;

/// A track that would play well after the reference, and why.
#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion<'candidates> {
    /// The suggested track.
    pub track: &'candidates PrepTrack,
    /// How its key moves from the reference, in Euphoria's words, such as
    /// "One step on the wheel".
    pub key_move: &'static str,
    /// How much faster it is than the reference, in percent, when both tempos are known.
    pub tempo_change_percent: Option<f32>,
    /// Its energy minus the reference's, when both are known.
    pub energy_change: Option<f32>,
}

impl Suggestion<'_> {
    /// One line explaining the pick, such as "One step on the wheel · +1.6% · energy +2".
    pub fn reason(&self) -> String {
        let mut parts = vec![self.key_move.to_owned()];
        if let Some(tempo) = self.tempo_change_percent {
            parts.push(format!("{tempo:+.1}%"));
        }
        if let Some(energy) = self.energy_change {
            parts.push(format!("energy {energy:+.0}"));
        }
        parts.join(" · ")
    }
}

/// The tracks that mix best after `reference` with the energy going `direction`, best first,
/// leaving out the ones `is_in_set` picks so a set never suggests a track it already holds.
pub fn suggest_next<'candidates>(
    reference: &PrepTrack,
    candidates: &'candidates [PrepTrack],
    direction: EnergyDirection,
    is_in_set: impl Fn(&PrepTrack) -> bool,
    limit: usize,
) -> Vec<Suggestion<'candidates>> {
    let pool = candidates
        .iter()
        .filter(|candidate| candidate.track_path != reference.track_path)
        .filter(|candidate| !is_in_set(candidate));
    next_tracks(reference, pool, &MixOptions::default(), direction)
        .into_iter()
        .take(limit)
        .map(|mix_match| Suggestion {
            track: mix_match.track,
            key_move: relation_name(mix_match.relation),
            tempo_change_percent: mix_match.tempo.map(|tempo| tempo.difference_percent),
            energy_change: mix_match.energy_difference,
        })
        .collect()
}
