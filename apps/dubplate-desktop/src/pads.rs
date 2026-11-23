//! What the eight performance pads of a deck do in each pad mode, as rekordbox lays them out on
//! the DDJ-FLX4: hot cues, beat jumps, beat loops, pad effects and rolls, the keyboard and key
//! shift, the slicer and the sampler.

use dubplate_engine::{BeatLength, PadEffect, SlicerDomain, SlicerMode, SlicerRoll, SlicerSlice};
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
/// SHIFT with this beat jump pad, pad 7, makes the jumps shorter, as rekordbox's Beat down.
pub const SHORTER_JUMPS_PAD: usize = 6;
/// SHIFT with this beat jump pad, pad 8, makes the jumps longer, as rekordbox's Beat up.
pub const LONGER_JUMPS_PAD: usize = 7;
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
        PadMode::Slicer => "Slicer",
        PadMode::SlicerLoop => "Slicer loop",
    }
}

/// What a Pad FX pad plays while it is held or latched.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PadFx {
    /// A loop roll of this length: the beat just passed repeats while the track plays on
    /// underneath.
    Roll(BeatLength),
    /// A beat effect on the deck.
    Effect(PadEffect),
}

/// The semitones of the Keyboard and Key Shift pads before SHIFT moves them, pad 1 first: +4 to
/// +7 on the top row and ±0 to +3 below, as rekordbox lays them out.
const KEY_PAD_SEMITONES: [i32; PAD_COUNT] = [4, 5, 6, 7, 0, 1, 2, 3];
/// The furthest a key moves, either way.
pub const MAX_KEY_SHIFT: i32 = 12;
/// SHIFT with this Keyboard or Key Shift pad moves every pad up a semitone.
pub const KEY_PADS_UP_PAD: usize = 6;
/// SHIFT with this Keyboard or Key Shift pad moves every pad down a semitone.
pub const KEY_PADS_DOWN_PAD: usize = 7;

/// The semitones Keyboard or Key Shift pad `pad` gives once SHIFT has moved the pads by
/// `offset`, or `None` past the range.
pub fn key_pad_semitones(pad: usize, offset: i32) -> Option<i32> {
    let semitones = KEY_PAD_SEMITONES.get(pad)? + offset;
    (-MAX_KEY_SHIFT..=MAX_KEY_SHIFT)
        .contains(&semitones)
        .then_some(semitones)
}

/// How far SHIFT may move the key pads, so that some pad always stays in range.
pub fn clamp_key_pad_offset(offset: i32) -> i32 {
    offset.clamp(-MAX_KEY_SHIFT, MAX_KEY_SHIFT - 3)
}

/// A shift in semitones as a pad shows it: `±0`, `+3`, `−2`.
pub fn semitones_label(semitones: i32) -> String {
    match semitones {
        0 => "\u{b1}0".to_owned(),
        positive if positive > 0 => format!("+{positive}"),
        negative => format!("\u{2212}{}", -negative),
    }
}

/// Whether `mode` is one of the two slicer modes.
pub fn is_slicer_mode(mode: PadMode) -> bool {
    slicer_engine_mode(mode).is_some()
}

/// The engine's slicer mode for a pad mode, or `None` for a pad mode that is not the slicer.
pub fn slicer_engine_mode(mode: PadMode) -> Option<SlicerMode> {
    match mode {
        PadMode::Slicer => Some(SlicerMode::Slicer),
        PadMode::SlicerLoop => Some(SlicerMode::SlicerLoop),
        _ => None,
    }
}

/// The mode the slicer key picks next: Slicer, then Slicer loop, then back to hot cues.
pub fn next_slicer_mode(mode: PadMode) -> PadMode {
    match mode {
        PadMode::Slicer => PadMode::SlicerLoop,
        PadMode::SlicerLoop => PadMode::HotCue,
        _ => PadMode::Slicer,
    }
}

/// The mode after `mode` in the order of the mode buttons, going round. The slicer modes
/// are left to their own key, and the mode after them is hot cue.
pub fn next_mode(mode: PadMode) -> PadMode {
    let Some(position) = PAD_MODES.iter().position(|candidate| *candidate == mode) else {
        return PadMode::HotCue;
    };
    PAD_MODES[(position + 1) % PAD_MODES.len()]
}

/// The on-screen modes and the alternate modes available on each button.
pub fn mode_buttons() -> impl Iterator<Item = (PadMode, PadMode)> {
    PAD_MODES
        .as_chunks::<2>()
        .0
        .iter()
        .map(|[mode, shift_mode]| (*mode, *shift_mode))
        .chain([
            (PadMode::Slicer, PadMode::Slicer),
            (PadMode::SlicerLoop, PadMode::SlicerLoop),
        ])
}

/// The FLX4's notes still carry their physical mode while its pads play the on-screen slicer.
pub fn pad_mode_for_press(deck_mode: PadMode, sent_mode: Option<PadMode>) -> PadMode {
    if is_slicer_mode(deck_mode) {
        deck_mode
    } else {
        sent_mode.unwrap_or(deck_mode)
    }
}

/// How many pads roll in the slicer's SHIFT layer, pad 1 first.
pub const SLICER_ROLL_PADS: usize = SlicerRoll::ALL.len();

/// What a slicer pad plays: the slice it is, or with SHIFT a roll of the slice the playhead is
/// in (the first when it is outside the domain), longest roll on pad 1. `None` for the pads
/// that do nothing with SHIFT.
pub fn slicer_pad_target(
    pad: usize,
    shift: bool,
    playing: Option<SlicerSlice>,
) -> Option<(SlicerSlice, Option<SlicerRoll>)> {
    if shift {
        let roll = SlicerRoll::ALL.get(pad).copied()?;
        return Some((playing.unwrap_or(SlicerSlice::FIRST), Some(roll)));
    }
    let slice = SlicerSlice::from_index(u8::try_from(pad).ok()?)?;
    Some((slice, None))
}

/// A roll as a pad shows it: `1/16`.
pub fn roll_label(roll: SlicerRoll) -> &'static str {
    match roll {
        SlicerRoll::Eighth => "1/8",
        SlicerRoll::Sixteenth => "1/16",
        SlicerRoll::ThirtySecond => "1/32",
        SlicerRoll::SixtyFourth => "1/64",
    }
}

/// A slicer domain as its picker button reads: the beats.
pub fn domain_label(domain: SlicerDomain) -> String {
    format!("{}", domain.beats())
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
                (PadMode::Slicer, PadMode::Slicer),
                (PadMode::SlicerLoop, PadMode::SlicerLoop),
            ]
        );
        assert_eq!(next_mode(PadMode::KeyShift), PadMode::HotCue);
        assert_eq!(next_mode(PadMode::HotCue), PadMode::Keyboard);
        assert_eq!(next_mode(PadMode::SlicerLoop), PadMode::HotCue);
    }

    #[test]
    fn the_slicer_key_goes_slicer_then_slicer_loop_then_back_to_hot_cues() {
        assert_eq!(next_slicer_mode(PadMode::Sampler), PadMode::Slicer);
        assert_eq!(next_slicer_mode(PadMode::Slicer), PadMode::SlicerLoop);
        assert_eq!(next_slicer_mode(PadMode::SlicerLoop), PadMode::HotCue);
        assert_eq!(
            slicer_engine_mode(PadMode::SlicerLoop),
            Some(SlicerMode::SlicerLoop)
        );
        assert_eq!(slicer_engine_mode(PadMode::BeatLoop), None);
    }

    #[test]
    fn pads_one_to_eight_play_slices_and_shift_pads_play_rolls_of_the_playing_slice() {
        for pad in 0..PAD_COUNT {
            let (slice, roll) = slicer_pad_target(pad, false, None).unwrap();
            assert_eq!(usize::from(slice.index()), pad);
            assert_eq!(roll, None);
        }
        assert_eq!(slicer_pad_target(8, false, None), None);
        let third = SlicerSlice::from_index(2);
        let rolls: Vec<_> = (0..PAD_COUNT)
            .map(|pad| slicer_pad_target(pad, true, third))
            .collect();
        assert_eq!(rolls[0], Some((third.unwrap(), Some(SlicerRoll::Eighth))));
        assert_eq!(
            rolls[1],
            Some((third.unwrap(), Some(SlicerRoll::Sixteenth)))
        );
        assert_eq!(
            rolls[2],
            Some((third.unwrap(), Some(SlicerRoll::ThirtySecond)))
        );
        assert_eq!(
            rolls[3],
            Some((third.unwrap(), Some(SlicerRoll::SixtyFourth)))
        );
        assert!(rolls[4..].iter().all(Option::is_none));
        assert_eq!(
            slicer_pad_target(0, true, None),
            Some((SlicerSlice::FIRST, Some(SlicerRoll::Eighth))),
            "a roll outside the domain uses the first slice"
        );
        assert_eq!(roll_label(SlicerRoll::SixtyFourth), "1/64");
        assert_eq!(domain_label(SlicerDomain::SixtyFour), "64");
    }

    #[test]
    fn tagged_controller_pads_play_the_current_slicer_mode() {
        assert_eq!(
            pad_mode_for_press(PadMode::Slicer, Some(PadMode::HotCue)),
            PadMode::Slicer,
        );
        assert_eq!(
            pad_mode_for_press(PadMode::SlicerLoop, Some(PadMode::Sampler)),
            PadMode::SlicerLoop,
        );
        assert_eq!(
            pad_mode_for_press(PadMode::HotCue, Some(PadMode::BeatJump)),
            PadMode::BeatJump,
        );
        assert_eq!(pad_mode_for_press(PadMode::Slicer, None), PadMode::Slicer);
    }

    #[test]
    fn key_pads_follow_rekordbox_and_shift_moves_them_within_an_octave() {
        assert_eq!(key_pad_semitones(0, 0), Some(4));
        assert_eq!(key_pad_semitones(4, 0), Some(0));
        assert_eq!(key_pad_semitones(7, 0), Some(3));
        assert_eq!(key_pad_semitones(4, -12), Some(-12));
        assert_eq!(key_pad_semitones(3, 9), None, "+16 is past the range");
        assert_eq!(clamp_key_pad_offset(40), 9);
        assert_eq!(clamp_key_pad_offset(-40), -12);
        assert_eq!(semitones_label(0), "\u{b1}0");
        assert_eq!(semitones_label(-2), "\u{2212}2");
        assert_eq!(semitones_label(5), "+5");
    }
}
