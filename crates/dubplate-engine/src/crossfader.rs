//! The faders' curves and the crossfader assign switches, as on a DJM and in rekordbox's mixer
//! panel and Preferences > Controller > Mixer.

use std::f32::consts::PI;
use std::fmt;

use crate::DECK_COUNT;
use crate::curves::{channel_fader_gain, crossfader_gains};
use crate::deck::DeckId;

/// How far from its end the Scratch curve's crossfader opens a side fully: a sharp cut, with
/// just enough travel that the cut does not click.
const SCRATCH_CUT_IN: f32 = 0.03;

/// How a channel fader's position becomes its level, as a DJM's channel fader curve switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChannelFaderCurve {
    /// The channel stays low until the fader is well up, then rises quickly: half way up is
    /// about -18 dB. For long, gradual blends.
    Slow,
    /// The audio taper of a club mixer: half way up is about -12 dB.
    #[default]
    Standard,
    /// The channel comes up quickly and the top of the fader changes little: half way up is
    /// about -2.5 dB. For cuts and quick mixes.
    Fast,
}

impl ChannelFaderCurve {
    /// Every curve, from slowest to fastest rise.
    pub const ALL: [ChannelFaderCurve; 3] = [Self::Slow, Self::Standard, Self::Fast];

    /// The gain at `position`, from 0.0 (closed, silent) to 1.0 (fully open, unity).
    pub fn gain(self, position: f32) -> f32 {
        let position = position.clamp(0.0, 1.0);
        match self {
            Self::Slow => position * position * position,
            Self::Standard => channel_fader_gain(position),
            Self::Fast => {
                let remaining = 1.0 - position;
                1.0 - remaining * remaining
            }
        }
    }
}

impl fmt::Display for ChannelFaderCurve {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Slow => "slow",
            Self::Standard => "standard",
            Self::Fast => "fast",
        })
    }
}

/// How the crossfader shares the level between its two sides, as a DJM's crossfader curve
/// switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CrossfaderCurve {
    /// A long, smooth blend at constant power: both sides at -3 dB in the middle.
    #[default]
    Blend,
    /// Both sides at full level through the middle, each fading out over the far half: no dip
    /// in the middle of a mix.
    Constant,
    /// A scratch cut: each side opens fully within the first few percent of travel from its
    /// far end.
    Scratch,
}

impl CrossfaderCurve {
    /// Every curve, from the smoothest to the sharpest.
    pub const ALL: [CrossfaderCurve; 3] = [Self::Blend, Self::Constant, Self::Scratch];

    /// The gains of the left (A) and right (B) sides at `position`, 0.0 fully left and 1.0
    /// fully right. At either end the far side is silent and the near side at unity.
    pub fn gains(self, position: f32) -> (f32, f32) {
        let position = position.clamp(0.0, 1.0);
        match self {
            Self::Blend => crossfader_gains(position),
            Self::Constant => {
                let fade = |past_middle: f32| (past_middle.max(0.0) * PI).cos();
                (fade(position - 0.5), fade(0.5 - position))
            }
            Self::Scratch => {
                let open = |from_far_end: f32| (from_far_end / SCRATCH_CUT_IN).clamp(0.0, 1.0);
                (open(1.0 - position), open(position))
            }
        }
    }
}

impl fmt::Display for CrossfaderCurve {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Blend => "blend",
            Self::Constant => "constant",
            Self::Scratch => "scratch",
        })
    }
}

/// Which side of the crossfader a channel plays on, as the assign switch under each channel of
/// a DJM. THRU takes the channel off the crossfader: only its own fader sets its level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrossfaderAssign {
    /// The left side.
    A,
    /// Off the crossfader.
    Thru,
    /// The right side.
    B,
}

impl CrossfaderAssign {
    /// Every assignment, left to right as the switch reads.
    pub const ALL: [CrossfaderAssign; 3] = [Self::A, Self::Thru, Self::B];

    /// The side a deck plays on until the DJ changes it: deck A on A, deck B on B.
    pub fn default_for(deck: DeckId) -> Self {
        match deck {
            DeckId::A => Self::A,
            DeckId::B => Self::B,
        }
    }

    /// The next position of the switch, A to THRU to B and back to A.
    pub fn next(self) -> Self {
        match self {
            Self::A => Self::Thru,
            Self::Thru => Self::B,
            Self::B => Self::A,
        }
    }
}

impl fmt::Display for CrossfaderAssign {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::A => "A",
            Self::Thru => "THRU",
            Self::B => "B",
        })
    }
}

/// The crossfader: whether it is in use, where it sits, its curve and which side each channel
/// is on. The engine and the window work out a channel's crossfader gain from it the same way.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crossfader {
    /// Whether the crossfader changes the sound at all. Off, every channel plays as if on
    /// THRU and the fader moves without effect, for DJs who mix on the channel faders alone.
    pub enabled: bool,
    /// From 0.0, fully left (A), to 1.0, fully right (B).
    pub position: f32,
    /// How the level is shared between the sides.
    pub curve: CrossfaderCurve,
    /// Each deck's side, deck A first.
    pub assign: [CrossfaderAssign; DECK_COUNT],
}

impl Default for Crossfader {
    fn default() -> Self {
        Self {
            enabled: true,
            position: 0.5,
            curve: CrossfaderCurve::default(),
            assign: DeckId::ALL.map(CrossfaderAssign::default_for),
        }
    }
}

impl Crossfader {
    /// The gain the crossfader gives `deck`: its side's gain, or unity on THRU or with the
    /// crossfader off.
    pub fn gain(&self, deck: DeckId) -> f32 {
        if !self.enabled {
            return 1.0;
        }
        let (left, right) = self.curve.gains(self.position);
        match self.assign[deck.index()] {
            CrossfaderAssign::A => left,
            CrossfaderAssign::Thru => 1.0,
            CrossfaderAssign::B => right,
        }
    }

    /// The deck on the left side and the deck on the right, when the crossfader is on and
    /// there is one deck on each: what Smart Fader needs to move from one deck to the other.
    pub fn sides(&self) -> Option<[DeckId; 2]> {
        if !self.enabled {
            return None;
        }
        match self.assign {
            [CrossfaderAssign::A, CrossfaderAssign::B] => Some([DeckId::A, DeckId::B]),
            [CrossfaderAssign::B, CrossfaderAssign::A] => Some([DeckId::B, DeckId::A]),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gain_to_decibels;

    fn close(actual: f32, expected: f32) -> bool {
        (actual - expected).abs() < 1e-4
    }

    #[test]
    fn every_channel_curve_is_silent_closed_and_at_unity_open() {
        for curve in ChannelFaderCurve::ALL {
            assert!(close(curve.gain(0.0), 0.0), "{curve}");
            assert!(close(curve.gain(1.0), 1.0), "{curve}");
            let mut previous = 0.0;
            for step in 1..=100 {
                let gain = curve.gain(step as f32 / 100.0);
                assert!(gain >= previous, "{curve} falls at {step}");
                previous = gain;
            }
        }
    }

    #[test]
    fn channel_curves_differ_half_way_up() {
        let half = |curve: ChannelFaderCurve| gain_to_decibels(curve.gain(0.5));
        assert!((half(ChannelFaderCurve::Slow) + 18.06).abs() < 0.1);
        assert!((half(ChannelFaderCurve::Standard) + 12.04).abs() < 0.1);
        assert!((half(ChannelFaderCurve::Fast) + 2.5).abs() < 0.1);
    }

    #[test]
    fn every_crossfader_curve_cuts_the_far_side_at_each_end() {
        for curve in CrossfaderCurve::ALL {
            let (left, right) = curve.gains(0.0);
            assert!(close(left, 1.0) && close(right, 0.0), "{curve} at A");
            let (left, right) = curve.gains(1.0);
            assert!(close(left, 0.0) && close(right, 1.0), "{curve} at B");
            let (left, right) = curve.gains(0.5);
            assert!(close(left, right), "{curve} is symmetric");
        }
    }

    #[test]
    fn the_curves_shape_the_middle() {
        let (left, _) = CrossfaderCurve::Blend.gains(0.5);
        assert!((gain_to_decibels(left) + 3.01).abs() < 0.05);
        let (left, right) = CrossfaderCurve::Constant.gains(0.5);
        assert!(close(left, 1.0) && close(right, 1.0));
        let (left, right) = CrossfaderCurve::Scratch.gains(0.02);
        assert!(close(left, 1.0) && right > 0.6, "open after two percent");
        let (left, right) = CrossfaderCurve::Scratch.gains(0.5);
        assert!(close(left, 1.0) && close(right, 1.0));
    }

    #[test]
    fn thru_takes_a_channel_off_the_crossfader() {
        let crossfader = Crossfader {
            position: 0.0,
            assign: [CrossfaderAssign::A, CrossfaderAssign::Thru],
            ..Crossfader::default()
        };
        assert!(close(crossfader.gain(DeckId::A), 1.0));
        assert!(close(crossfader.gain(DeckId::B), 1.0));
        let at_b = Crossfader {
            position: 1.0,
            ..crossfader
        };
        assert!(close(at_b.gain(DeckId::A), 0.0));
        assert!(close(at_b.gain(DeckId::B), 1.0));
        assert_eq!(crossfader.sides(), None);
    }

    #[test]
    fn a_crossfader_turned_off_leaves_both_channels_open() {
        let crossfader = Crossfader {
            enabled: false,
            position: 1.0,
            curve: CrossfaderCurve::Scratch,
            ..Crossfader::default()
        };
        assert!(close(crossfader.gain(DeckId::A), 1.0));
        assert!(close(crossfader.gain(DeckId::B), 1.0));
        assert_eq!(crossfader.sides(), None);
    }

    #[test]
    fn swapped_sides_follow_the_switches() {
        let crossfader = Crossfader {
            position: 0.0,
            assign: [CrossfaderAssign::B, CrossfaderAssign::A],
            ..Crossfader::default()
        };
        assert!(close(crossfader.gain(DeckId::A), 0.0));
        assert!(close(crossfader.gain(DeckId::B), 1.0));
        assert_eq!(crossfader.sides(), Some([DeckId::B, DeckId::A]));
        assert_eq!(Crossfader::default().sides(), Some([DeckId::A, DeckId::B]));
    }

    #[test]
    fn the_switch_steps_a_thru_b() {
        assert_eq!(CrossfaderAssign::A.next(), CrossfaderAssign::Thru);
        assert_eq!(CrossfaderAssign::Thru.next(), CrossfaderAssign::B);
        assert_eq!(CrossfaderAssign::B.next(), CrossfaderAssign::A);
    }
}
