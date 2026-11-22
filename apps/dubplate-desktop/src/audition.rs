//! Where a seam audition starts in each track: a few bars of the outgoing track, then the
//! incoming track from its mix-in at the outgoing track's tempo, overlapping as planned.

use std::time::Duration;

/// How long a seam audition plays.
pub const AUDITION_LENGTH: Duration = Duration::from_secs(20);
/// How much of the outgoing track plays before the incoming track comes in.
const LEAD_IN_SECONDS: f64 = 8.0;
/// The overlap when the seam has no plan yet.
pub const DEFAULT_OVERLAP_BARS: u32 = 16;
/// Beats in a bar.
const BEATS_PER_BAR: f64 = 4.0;

/// What the audition needs to know about one side of the seam.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SeamSide {
    /// The track's tempo, when known.
    pub bpm: Option<f32>,
    /// The cue the seam uses: the outgoing track's mix-out or the incoming track's mix-in, in
    /// milliseconds.
    pub cue_ms: Option<u32>,
}

/// Where and how fast each track plays in an audition.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AuditionPlan {
    /// Where the outgoing track starts, in seconds.
    pub outgoing_start_seconds: f64,
    /// Where the incoming track starts: its mix-in, in seconds.
    pub incoming_start_seconds: f64,
    /// When the incoming track comes in, in seconds after the audition starts.
    pub incoming_offset_seconds: f64,
    /// The incoming track's tempo, where 1.0 is its own, matched to the outgoing track.
    pub incoming_tempo: f64,
}

/// Why a seam cannot be auditioned yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditionProblem {
    /// The outgoing track has no mix-out.
    NoMixOut,
    /// The incoming track has no mix-in.
    NoMixIn,
}

/// Plans the audition of the seam from `outgoing` into `incoming`, overlapping `overlap_bars`.
/// The incoming track comes in `overlap_bars` before the outgoing track's mix-out, after a lead
/// of the outgoing track, and plays at the outgoing track's tempo.
pub fn plan_audition(
    outgoing: SeamSide,
    incoming: SeamSide,
    overlap_bars: u32,
) -> Result<AuditionPlan, AuditionProblem> {
    let mix_out = seconds(outgoing.cue_ms.ok_or(AuditionProblem::NoMixOut)?);
    let mix_in = seconds(incoming.cue_ms.ok_or(AuditionProblem::NoMixIn)?);
    let overlap = match outgoing.bpm.filter(|bpm| *bpm > 0.0) {
        Some(bpm) => f64::from(overlap_bars) * BEATS_PER_BAR * 60.0 / f64::from(bpm),
        None => 0.0,
    };
    let incoming_enters = (mix_out - overlap).max(0.0);
    let outgoing_start = (incoming_enters - LEAD_IN_SECONDS).max(0.0);
    let incoming_tempo = match (outgoing.bpm, incoming.bpm) {
        (Some(outgoing_bpm), Some(incoming_bpm)) if outgoing_bpm > 0.0 && incoming_bpm > 0.0 => {
            f64::from(outgoing_bpm) / f64::from(incoming_bpm)
        }
        _ => 1.0,
    };
    Ok(AuditionPlan {
        outgoing_start_seconds: outgoing_start,
        incoming_start_seconds: mix_in,
        incoming_offset_seconds: incoming_enters - outgoing_start,
        incoming_tempo,
    })
}

fn seconds(milliseconds: u32) -> f64 {
    f64::from(milliseconds) / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn side(bpm: f32, cue_ms: u32) -> SeamSide {
        SeamSide {
            bpm: Some(bpm),
            cue_ms: Some(cue_ms),
        }
    }

    #[test]
    fn the_incoming_track_comes_in_the_overlap_before_the_mix_out_after_a_lead() {
        let plan = plan_audition(side(120.0, 300_000), side(125.0, 16_000), 16).unwrap();
        let overlap = 16.0 * 4.0 * 0.5;
        assert!((plan.outgoing_start_seconds - (300.0 - overlap - LEAD_IN_SECONDS)).abs() < 1e-9);
        assert!((plan.incoming_offset_seconds - LEAD_IN_SECONDS).abs() < 1e-9);
        assert!((plan.incoming_start_seconds - 16.0).abs() < 1e-9);
        assert!((plan.incoming_tempo - 120.0 / 125.0).abs() < 1e-9);
    }

    #[test]
    fn a_seam_near_the_start_begins_at_the_start() {
        let plan = plan_audition(side(128.0, 4_000), side(128.0, 0), 16).unwrap();
        assert_eq!(plan.outgoing_start_seconds, 0.0);
        assert_eq!(plan.incoming_offset_seconds, 0.0);
        assert_eq!(plan.incoming_tempo, 1.0);
    }

    #[test]
    fn without_tempos_the_tracks_play_as_they_are_and_meet_at_the_mix_out() {
        let outgoing = SeamSide {
            bpm: None,
            cue_ms: Some(60_000),
        };
        let incoming = SeamSide {
            bpm: Some(124.0),
            cue_ms: Some(1_000),
        };
        let plan = plan_audition(outgoing, incoming, 16).unwrap();
        assert_eq!(plan.incoming_tempo, 1.0);
        assert!((plan.outgoing_start_seconds - (60.0 - LEAD_IN_SECONDS)).abs() < 1e-9);
    }

    #[test]
    fn a_seam_needs_both_cues() {
        let missing = SeamSide {
            bpm: Some(120.0),
            cue_ms: None,
        };
        assert_eq!(
            plan_audition(missing, side(120.0, 0), 16),
            Err(AuditionProblem::NoMixOut)
        );
        assert_eq!(
            plan_audition(side(120.0, 0), missing, 16),
            Err(AuditionProblem::NoMixIn)
        );
    }
}
