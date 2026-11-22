//! What the eight performance pads of a deck do in each pad mode, as rekordbox lays them out on
//! the DDJ-FLX4: hot cues, beat jumps, beat loops, pad effects and rolls, the keyboard and key
//! shift, and the sampler that waits for the engine.

use dubplate_engine::{BeatLength, EffectKind, PadEffect, ParameterValue};
use dubplate_midi::PadMode;

use crate::effects::effect_pad_label;

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

/// What a Pad FX pad plays while it is held.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PadFx {
    /// A loop roll of this length: the beat just passed repeats while the track plays on
    /// underneath.
    Roll(BeatLength),
    /// A beat effect on the deck.
    Effect(PadEffect),
}

impl PadFx {
    /// Its name and parameter, as its pad shows them.
    pub fn label(self) -> (&'static str, String) {
        match self {
            PadFx::Roll(length) => ("Roll", length.to_string()),
            PadFx::Effect(effect) => (
                effect_pad_label(effect.kind),
                effect
                    .parameter
                    .map_or_else(String::new, |value| value.to_string()),
            ),
        }
    }
}

/// Depth for the echoes and reverbs: the trail as loud as the track, on top of it.
const WASH_DEPTH: f32 = 0.5;
/// Depth for the other effects, which take the track over.
const FULL_DEPTH: f32 = 0.85;

const fn effect(kind: EffectKind, parameter: Option<ParameterValue>) -> PadFx {
    let depth = match kind {
        EffectKind::Echo | EffectKind::PingPong | EffectKind::Reverb => WASH_DEPTH,
        _ => FULL_DEPTH,
    };
    PadFx::Effect(PadEffect {
        kind,
        parameter,
        depth,
    })
}

const fn beats(length: BeatLength) -> Option<ParameterValue> {
    Some(ParameterValue::Beats(length))
}

const fn percent(value: i16) -> Option<ParameterValue> {
    Some(ParameterValue::Percent(value))
}

/// The 32 pad effects of the FLX4's PAD FX1 and PAD FX2 modes, each with and without SHIFT,
/// pad 1 first: rolls and echoes to build, filters and modulations to colour, brakes and
/// backspins to end on.
const PAD_FX_BANKS: [[PadFx; PAD_COUNT]; 4] = [
    [
        PadFx::Roll(BeatLength::QUARTER),
        PadFx::Roll(BeatLength::HALF),
        PadFx::Roll(BeatLength::ONE),
        PadFx::Roll(BeatLength::TWO),
        effect(EffectKind::Echo, beats(BeatLength::HALF)),
        effect(EffectKind::Echo, beats(BeatLength::THREE_QUARTERS)),
        effect(EffectKind::Reverb, percent(50)),
        effect(EffectKind::Brake, beats(BeatLength::ONE)),
    ],
    [
        PadFx::Roll(BeatLength::SIXTEENTH),
        PadFx::Roll(BeatLength::EIGHTH),
        effect(EffectKind::Gate, beats(BeatLength::QUARTER)),
        effect(EffectKind::Gate, beats(BeatLength::EIGHTH)),
        effect(EffectKind::PingPong, beats(BeatLength::HALF)),
        effect(EffectKind::Echo, beats(BeatLength::ONE)),
        effect(EffectKind::Crusher, None),
        effect(EffectKind::Backspin, beats(BeatLength::ONE)),
    ],
    [
        effect(EffectKind::Filter, beats(BeatLength::FOUR)),
        effect(EffectKind::HighPass, percent(50)),
        effect(EffectKind::LowPass, percent(50)),
        effect(EffectKind::BandPass, percent(50)),
        effect(EffectKind::Flanger, beats(BeatLength::FOUR)),
        effect(EffectKind::Phaser, beats(BeatLength::FOUR)),
        effect(EffectKind::Gate, beats(BeatLength::HALF)),
        effect(EffectKind::Crusher, None),
    ],
    [
        PadFx::Roll(BeatLength::FOUR),
        PadFx::Roll(BeatLength::EIGHT),
        effect(EffectKind::Echo, beats(BeatLength::QUARTER)),
        effect(EffectKind::PingPong, beats(BeatLength::THREE_QUARTERS)),
        effect(EffectKind::Reverb, percent(80)),
        effect(EffectKind::Filter, beats(BeatLength::EIGHT)),
        effect(EffectKind::Brake, beats(BeatLength::TWO)),
        effect(EffectKind::Backspin, beats(BeatLength::TWO)),
    ],
];

/// What pad `pad` plays in a Pad FX mode, with or without SHIFT.
pub fn pad_fx(mode: PadMode, pad: usize, shift: bool) -> Option<PadFx> {
    let bank = match (mode, shift) {
        (PadMode::PadFx1, false) => 0,
        (PadMode::PadFx1, true) => 1,
        (PadMode::PadFx2, false) => 2,
        (PadMode::PadFx2, true) => 3,
        _ => return None,
    };
    PAD_FX_BANKS[bank].get(pad).copied()
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
    }

    #[test]
    fn every_pad_fx_pad_plays_something_and_rolls_build_from_short_to_long() {
        for mode in [PadMode::PadFx1, PadMode::PadFx2] {
            for shift in [false, true] {
                for pad in 0..PAD_COUNT {
                    assert!(pad_fx(mode, pad, shift).is_some(), "{mode:?} {shift} {pad}");
                }
            }
        }
        assert_eq!(
            pad_fx(PadMode::PadFx1, 0, false),
            Some(PadFx::Roll(BeatLength::QUARTER))
        );
        assert_eq!(pad_fx(PadMode::HotCue, 0, false), None);
        assert_eq!(pad_fx(PadMode::PadFx1, 8, false), None);
        let (name, beats) = pad_fx(PadMode::PadFx1, 4, false).unwrap().label();
        assert_eq!((name, beats.as_str()), ("Echo", "1/2"));
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
