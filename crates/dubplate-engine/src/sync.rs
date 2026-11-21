use std::fmt;

use crate::deck::DeckId;

/// Once locked, a phase error past this many beats is corrected with a jump rather than a
/// tempo bend.
const SNAP_BEATS: f64 = 0.05;
/// Before the deck has locked, a phase error past this many beats makes it jump onto the
/// master's beat: well under a millisecond at club tempos.
const LOCK_BEATS: f64 = 0.001;
/// How long a small phase error takes to close by bending the tempo.
const CORRECTION_SECONDS: f64 = 0.5;
/// The largest tempo bend that closes a small phase error, as a share of the tempo: small
/// enough to stay unheard.
const MAX_CORRECTION: f64 = 0.01;
/// Tempo multiples a deck may sync at, when half and double tempo are allowed.
const MULTIPLES: [f64; 3] = [0.5, 1.0, 2.0];

/// How a deck follows the tempo master, like the SYNC button of a CDJ.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum SyncMode {
    /// The deck plays at its own tempo fader.
    #[default]
    Off,
    /// The deck plays at the master's BPM, with its beats wherever they fall.
    Tempo,
    /// The deck plays at the master's BPM with its beats on the master's: beat sync. Nudging,
    /// scratching or bending the pitch of a following deck drops it to [`SyncMode::Tempo`], as
    /// on a CDJ-3000, so a DJ can still ride it by hand.
    Beat,
}

impl SyncMode {
    pub(crate) fn index(self) -> u8 {
        match self {
            SyncMode::Off => 0,
            SyncMode::Tempo => 1,
            SyncMode::Beat => 2,
        }
    }

    pub(crate) fn from_index(index: u8) -> SyncMode {
        match index {
            1 => SyncMode::Tempo,
            2 => SyncMode::Beat,
            _ => SyncMode::Off,
        }
    }
}

impl fmt::Display for SyncMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            SyncMode::Off => "off",
            SyncMode::Tempo => "tempo sync",
            SyncMode::Beat => "beat sync",
        })
    }
}

/// What a following deck needs to know about the tempo master at the start of a block.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MasterBeat {
    pub(crate) deck: DeckId,
    /// The master's tempo as it plays, in beats per minute, without nudges or pitch bends.
    pub(crate) bpm: f64,
    /// The master's beat position.
    pub(crate) beat: f64,
    /// The master's record moves on its own: it plays and no hand holds it.
    pub(crate) running: bool,
}

/// The multiple of the master's tempo a deck syncs at: the same tempo, or half or double when
/// that is closer to the deck's own, so 70 BPM locks to 140 BPM.
pub(crate) fn sync_multiple(master_bpm: f64, deck_bpm: f64, allow_half_and_double: bool) -> f64 {
    if !allow_half_and_double || master_bpm <= 0.0 || deck_bpm <= 0.0 {
        return 1.0;
    }
    let distance = |multiple: f64| (master_bpm * multiple / deck_bpm).ln().abs();
    MULTIPLES
        .into_iter()
        .min_by(|first, second| distance(*first).total_cmp(&distance(*second)))
        .unwrap_or(1.0)
}

/// What a following deck should do about its phase this block.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PhaseAction {
    /// Jump by this many beats.
    Jump(f64),
    /// Play this much faster, as a share of the tempo, to close a small error.
    Bend(f64),
}

/// Compares the deck's beat with the master's, in the deck's beats, and picks a correction. A
/// deck that is not locked yet jumps straight onto the beat; a locked one bends its tempo
/// slightly to stay there, and only jumps when something moved it far off.
pub(crate) fn phase_action(
    master_beat: f64,
    deck_beat: f64,
    deck_beats_per_second: f64,
    locked: bool,
) -> PhaseAction {
    let difference = master_beat - deck_beat;
    let error = difference - difference.round();
    let jump_beyond = if locked { SNAP_BEATS } else { LOCK_BEATS };
    if error.abs() > jump_beyond || deck_beats_per_second <= 0.0 {
        PhaseAction::Jump(error)
    } else {
        let bend = error / (CORRECTION_SECONDS * deck_beats_per_second);
        PhaseAction::Bend(bend.clamp(-MAX_CORRECTION, MAX_CORRECTION))
    }
}

/// A deck's sync settings and what the audio thread found when it last followed the master.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SyncFollow {
    pub(crate) mode: SyncMode,
    /// The multiple of the master's tempo chosen when the deck locked to this master.
    pub(crate) multiple: Option<(DeckId, f64)>,
    /// Whether the deck followed another deck in the last block.
    pub(crate) following: bool,
    /// Whether the deck's beats sat on the master's in the last block.
    pub(crate) locked: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_and_double_tempo_lock_to_the_nearest_multiple() {
        assert_eq!(sync_multiple(140.0, 72.0, true), 0.5);
        assert_eq!(sync_multiple(70.0, 138.0, true), 2.0);
        assert_eq!(sync_multiple(124.0, 128.0, true), 1.0);
        assert_eq!(sync_multiple(140.0, 72.0, false), 1.0);
    }

    #[test]
    fn small_phase_errors_bend_and_large_ones_jump() {
        assert_eq!(phase_action(10.02, 4.0, 2.0, true), PhaseAction::Bend(0.01));
        match phase_action(10.004, 4.0, 2.0, true) {
            PhaseAction::Bend(bend) => assert!((bend - 0.004).abs() < 1e-9),
            PhaseAction::Jump(_) => panic!("a small error should bend"),
        }
        assert_eq!(phase_action(10.0, 3.0, 2.0, true), PhaseAction::Bend(0.0));
        assert!(matches!(
            phase_action(10.004, 4.0, 2.0, false),
            PhaseAction::Jump(_)
        ));
        match phase_action(10.3, 4.0, 2.0, true) {
            PhaseAction::Jump(beats) => assert!((beats - 0.3).abs() < 1e-9),
            PhaseAction::Bend(_) => panic!("a large error should jump"),
        }
        match phase_action(10.0, 4.7, 2.0, true) {
            PhaseAction::Jump(beats) => assert!((beats - 0.3).abs() < 1e-9),
            PhaseAction::Bend(_) => panic!("a large error should jump to the nearest beat"),
        }
    }
}
