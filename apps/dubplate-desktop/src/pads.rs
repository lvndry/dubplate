//! What the eight performance pads of a deck do in each pad mode, as rekordbox lays them out on
//! the DDJ-FLX4: hot cues, beat jumps and beat loops, and the modes that wait for the engine.

use dubplate_engine::BeatLength;
use dubplate_midi::PadMode;

/// Pads on a deck.
pub const PAD_COUNT: usize = 8;

/// The pad modes in the order of the FLX4's four mode buttons, each followed by the mode it
/// gives with SHIFT.
pub const PAD_MODES: [PadMode; 8] = [
    PadMode::HotCue,
    PadMode::Keyboard,
    PadMode::PadFx1,
    PadMode::PadFx2,
    PadMode::BeatJump,
    PadMode::BeatLoop,
    PadMode::Sampler,
    PadMode::KeyShift,
];

/// The beats each beat jump pad moves by at the base size: back and forward by 1, 2, 4 and 8,
/// pad 1 first, as rekordbox lays them out.
const BEAT_JUMP_BEATS: [f64; PAD_COUNT] = [-1.0, 1.0, -2.0, 2.0, -4.0, 4.0, -8.0, 8.0];
/// How many halvings and doublings SHIFT + pads 7 and 8 allow from the base size.
pub const BEAT_JUMP_SCALE_RANGE: i32 = 3;

/// The loop each beat loop pad starts, pad 1 first.
pub const BEAT_LOOP_LENGTHS: [BeatLength; PAD_COUNT] = [
    BeatLength::QUARTER,
    BeatLength::HALF,
    BeatLength::ONE,
    BeatLength::TWO,
    BeatLength::FOUR,
    BeatLength::EIGHT,
    BeatLength::SIXTEEN,
    BeatLength::THIRTY_TWO,
];

/// The loop 4 BEAT/EXIT starts.
pub const AUTO_BEAT_LOOP: BeatLength = BeatLength::FOUR;

/// The mode's name on screen.
pub fn mode_label(mode: PadMode) -> &'static str {
    match mode {
        PadMode::HotCue => "Hot cue",
        PadMode::PadFx1 => "Pad FX 1",
        PadMode::BeatJump => "Beat jump",
        PadMode::Sampler => "Sampler",
        PadMode::Keyboard => "Keyboard",
        PadMode::PadFx2 => "Pad FX 2",
        PadMode::BeatLoop => "Beat loop",
        PadMode::KeyShift => "Key shift",
    }
}

/// Whether Dubplate plays the pads of this mode yet. The others light their button and wait
/// for the engine's pad effects, sampler and key shifting.
pub fn mode_is_playable(mode: PadMode) -> bool {
    matches!(
        mode,
        PadMode::HotCue | PadMode::BeatJump | PadMode::BeatLoop
    )
}

/// The mode after `mode` in the order of the mode buttons, going round.
pub fn next_mode(mode: PadMode) -> PadMode {
    let position = PAD_MODES
        .iter()
        .position(|candidate| *candidate == mode)
        .unwrap_or(0);
    PAD_MODES[(position + 1) % PAD_MODES.len()]
}

/// The four mode buttons, each as its own mode and its SHIFT mode.
pub fn mode_buttons() -> impl Iterator<Item = (PadMode, PadMode)> {
    PAD_MODES
        .as_chunks::<2>()
        .0
        .iter()
        .map(|[mode, shift_mode]| (*mode, *shift_mode))
}

/// The beats beat jump pad `pad` moves by, at a size halved or doubled `scale` times.
pub fn beat_jump_beats(pad: usize, scale: i32) -> Option<f64> {
    let base = BEAT_JUMP_BEATS.get(pad)?;
    Some(base * 2_f64.powi(scale.clamp(-BEAT_JUMP_SCALE_RANGE, BEAT_JUMP_SCALE_RANGE)))
}

/// A beat count as a pad shows it: `1/2`, `4`.
pub fn beats_label(beats: f64) -> String {
    let magnitude = beats.abs();
    if magnitude >= 1.0 {
        format!("{magnitude:.0}")
    } else {
        format!("1/{:.0}", 1.0 / magnitude)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beat_jump_pads_go_back_and_forward_and_scale_by_halves_and_doubles() {
        assert_eq!(beat_jump_beats(0, 0), Some(-1.0));
        assert_eq!(beat_jump_beats(7, 0), Some(8.0));
        assert_eq!(beat_jump_beats(7, 1), Some(16.0));
        assert_eq!(beat_jump_beats(1, -2), Some(0.25));
        assert_eq!(
            beat_jump_beats(7, 99),
            Some(64.0),
            "the size stops at its range"
        );
        assert_eq!(beat_jump_beats(8, 0), None);
        assert_eq!(beats_label(-0.5), "1/2");
        assert_eq!(beats_label(16.0), "16");
    }

    #[test]
    fn the_modes_follow_the_buttons_and_their_shift_modes() {
        let buttons: Vec<(PadMode, PadMode)> = mode_buttons().collect();
        assert_eq!(
            buttons,
            [
                (PadMode::HotCue, PadMode::Keyboard),
                (PadMode::PadFx1, PadMode::PadFx2),
                (PadMode::BeatJump, PadMode::BeatLoop),
                (PadMode::Sampler, PadMode::KeyShift),
            ]
        );
        assert_eq!(next_mode(PadMode::KeyShift), PadMode::HotCue);
        assert_eq!(next_mode(PadMode::HotCue), PadMode::Keyboard);
        assert!(mode_is_playable(PadMode::BeatLoop));
        assert!(!mode_is_playable(PadMode::Sampler));
    }
}
