use std::fmt;
use std::str::FromStr;

use crate::beat::BeatLength;
use crate::effect::EffectKind;

/// Keeps every signed length's position away from zero, so falling lengths order before rising
/// ones: -32 beats first, then towards -1/16, then 1/16 up to 32.
const SIGNED_POSITION_OFFSET: f64 = 16.0;

/// The value of an effect's parameter: what the BEAT buttons of a DJ mixer step through and
/// what its BEAT window shows.
///
/// Time-based effects take a length in beats. Others take a percentage, such as a reverb's
/// size, or a pitch in semitones.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ParameterValue {
    /// A length in beats: an echo time, a sweep period, a gate step, a roll length.
    Beats(BeatLength),
    /// A length in beats with a direction, for effects that rise or fall forever.
    SignedBeats(SignedBeats),
    /// A percentage.
    Percent(i16),
    /// A pitch in semitones.
    Semitones(i8),
}

impl ParameterValue {
    /// The length in beats, for a value that is one.
    pub fn beats(self) -> Option<BeatLength> {
        match self {
            Self::Beats(length) | Self::SignedBeats(SignedBeats { length, .. }) => Some(length),
            Self::Percent(_) | Self::Semitones(_) => None,
        }
    }

    fn unit(self) -> ParameterUnit {
        match self {
            Self::Beats(_) => ParameterUnit::Beats,
            Self::SignedBeats(_) => ParameterUnit::SignedBeats,
            Self::Percent(_) => ParameterUnit::Percent,
            Self::Semitones(_) => ParameterUnit::Semitones,
        }
    }

    /// A number that orders values of the same unit, for finding the nearest step.
    fn position(self) -> f64 {
        match self {
            Self::Beats(length) => length.beats().log2(),
            Self::SignedBeats(SignedBeats { length, falling }) => {
                let magnitude = SIGNED_POSITION_OFFSET + length.beats().log2();
                if falling { -magnitude } else { magnitude }
            }
            Self::Percent(percent) => f64::from(percent),
            Self::Semitones(semitones) => f64::from(semitones),
        }
    }
}

/// A cycle length in beats with a direction, for effects that rise or fall forever.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SignedBeats {
    /// The length of one cycle.
    pub length: BeatLength,
    /// Whether the effect falls instead of rising.
    pub falling: bool,
}

impl SignedBeats {
    /// A rising cycle of `length`.
    pub const fn rising(length: BeatLength) -> Self {
        Self {
            length,
            falling: false,
        }
    }

    /// A falling cycle of `length`.
    pub const fn falling(length: BeatLength) -> Self {
        Self {
            length,
            falling: true,
        }
    }
}

impl From<BeatLength> for ParameterValue {
    fn from(length: BeatLength) -> Self {
        Self::Beats(length)
    }
}

impl fmt::Display for ParameterValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Beats(length) => write!(formatter, "{length}"),
            Self::SignedBeats(SignedBeats { length, falling }) => {
                write!(formatter, "{}{length}", if *falling { "-" } else { "+" })
            }
            Self::Percent(percent) => write!(formatter, "{percent}%"),
            Self::Semitones(0) => formatter.write_str("0 st"),
            Self::Semitones(semitones) => write!(formatter, "{semitones:+} st"),
        }
    }
}

/// The error returned when text is not a parameter value such as `1/2`, `-4`, `75%` or `+3 st`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseParameterError {
    text: String,
}

impl fmt::Display for ParseParameterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "`{}` is not an effect parameter such as 1/2, -4, 75% or +3 st",
            self.text
        )
    }
}

impl std::error::Error for ParseParameterError {}

impl FromStr for ParameterValue {
    type Err = ParseParameterError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let error = || ParseParameterError {
            text: text.to_owned(),
        };
        let trimmed = text.trim();
        if let Some(percent) = trimmed.strip_suffix('%') {
            return percent
                .trim()
                .parse()
                .map(Self::Percent)
                .map_err(|_| error());
        }
        if let Some(semitones) = trimmed.strip_suffix("st") {
            let semitones = semitones.trim();
            let semitones = semitones.strip_prefix('+').unwrap_or(semitones);
            return semitones.parse().map(Self::Semitones).map_err(|_| error());
        }
        if let Some(length) = trimmed.strip_prefix('+') {
            let length = length.parse().map_err(|_| error())?;
            return Ok(Self::SignedBeats(SignedBeats::rising(length)));
        }
        if let Some(length) = trimmed.strip_prefix('-') {
            let length = length.parse().map_err(|_| error())?;
            return Ok(Self::SignedBeats(SignedBeats::falling(length)));
        }
        trimmed.parse().map(Self::Beats).map_err(|_| error())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ParameterUnit {
    Beats,
    SignedBeats,
    Percent,
    Semitones,
}

/// What an effect's parameter is: its name, the values the BEAT buttons step through and the
/// value it starts at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParameterSpec {
    /// What the parameter sets, such as `Beat`, `Size` or `Pitch`.
    pub name: &'static str,
    /// Every value, lowest first.
    pub steps: &'static [ParameterValue],
    /// The value a new effect starts at.
    pub default: ParameterValue,
}

impl ParameterSpec {
    /// The step `steps` places from `value`, stopping at either end. A value that is not one of
    /// the steps moves from the nearest one; a value of another unit gives the default.
    pub fn step(&self, value: ParameterValue, steps: i32) -> ParameterValue {
        let Some(position) = self.nearest(value) else {
            return self.default;
        };
        let last = self.steps.len().saturating_sub(1);
        let moved = i64::try_from(position).unwrap_or(0) + i64::from(steps);
        let index = usize::try_from(moved.max(0)).unwrap_or(0).min(last);
        self.steps.get(index).copied().unwrap_or(self.default)
    }

    /// Whether `value` is in this parameter's unit, so the effect can take it.
    pub fn accepts(&self, value: ParameterValue) -> bool {
        value.unit() == self.default.unit()
    }

    fn nearest(&self, value: ParameterValue) -> Option<usize> {
        if !self.accepts(value) {
            return None;
        }
        let wanted = value.position();
        self.steps
            .iter()
            .enumerate()
            .min_by(|(_, first), (_, second)| {
                (first.position() - wanted)
                    .abs()
                    .total_cmp(&(second.position() - wanted).abs())
            })
            .map(|(index, _)| index)
    }
}

const fn beats(length: BeatLength) -> ParameterValue {
    ParameterValue::Beats(length)
}

/// 1/16 to 16 beats: the echoes, gates and rolls of a DJ mixer's beat effects.
const SIXTEENTH_TO_SIXTEEN: [ParameterValue; 10] = [
    beats(BeatLength::SIXTEENTH),
    beats(BeatLength::EIGHTH),
    beats(BeatLength::QUARTER),
    beats(BeatLength::HALF),
    beats(BeatLength::THREE_QUARTERS),
    beats(BeatLength::ONE),
    beats(BeatLength::TWO),
    beats(BeatLength::FOUR),
    beats(BeatLength::EIGHT),
    beats(BeatLength::SIXTEEN),
];

/// 1/16 to 32 beats: the sweeps of the filter, flanger and phaser.
const SIXTEENTH_TO_THIRTY_TWO: [ParameterValue; 11] = [
    beats(BeatLength::SIXTEENTH),
    beats(BeatLength::EIGHTH),
    beats(BeatLength::QUARTER),
    beats(BeatLength::HALF),
    beats(BeatLength::THREE_QUARTERS),
    beats(BeatLength::ONE),
    beats(BeatLength::TWO),
    beats(BeatLength::FOUR),
    beats(BeatLength::EIGHT),
    beats(BeatLength::SIXTEEN),
    beats(BeatLength::THIRTY_TWO),
];

/// 1/4 to 32 beats: how long a brake or a backspin takes.
const QUARTER_TO_THIRTY_TWO: [ParameterValue; 9] = [
    beats(BeatLength::QUARTER),
    beats(BeatLength::HALF),
    beats(BeatLength::THREE_QUARTERS),
    beats(BeatLength::ONE),
    beats(BeatLength::TWO),
    beats(BeatLength::FOUR),
    beats(BeatLength::EIGHT),
    beats(BeatLength::SIXTEEN),
    beats(BeatLength::THIRTY_TWO),
];

/// 10% to 100% in tens.
const TENS_OF_PERCENT: [ParameterValue; 10] = [
    ParameterValue::Percent(10),
    ParameterValue::Percent(20),
    ParameterValue::Percent(30),
    ParameterValue::Percent(40),
    ParameterValue::Percent(50),
    ParameterValue::Percent(60),
    ParameterValue::Percent(70),
    ParameterValue::Percent(80),
    ParameterValue::Percent(90),
    ParameterValue::Percent(100),
];

/// 0% to 100% in tens.
const ZERO_TO_HUNDRED_PERCENT: [ParameterValue; 11] = [
    ParameterValue::Percent(0),
    ParameterValue::Percent(10),
    ParameterValue::Percent(20),
    ParameterValue::Percent(30),
    ParameterValue::Percent(40),
    ParameterValue::Percent(50),
    ParameterValue::Percent(60),
    ParameterValue::Percent(70),
    ParameterValue::Percent(80),
    ParameterValue::Percent(90),
    ParameterValue::Percent(100),
];

const fn beat_spec(steps: &'static [ParameterValue], default: BeatLength) -> ParameterSpec {
    ParameterSpec {
        name: "Beat",
        steps,
        default: ParameterValue::Beats(default),
    }
}

const ECHO: ParameterSpec = beat_spec(&SIXTEENTH_TO_SIXTEEN, BeatLength::HALF);
const GATE: ParameterSpec = beat_spec(&SIXTEENTH_TO_SIXTEEN, BeatLength::EIGHTH);
const FILTER_SWEEP: ParameterSpec = beat_spec(&SIXTEENTH_TO_THIRTY_TWO, BeatLength::FOUR);
const PHASER_SWEEP: ParameterSpec = beat_spec(&SIXTEENTH_TO_THIRTY_TWO, BeatLength::TWO);
const BRAKE: ParameterSpec = beat_spec(&QUARTER_TO_THIRTY_TWO, BeatLength::ONE);
const BACKSPIN: ParameterSpec = beat_spec(&QUARTER_TO_THIRTY_TWO, BeatLength::TWO);
const REVERB: ParameterSpec = ParameterSpec {
    name: "Size",
    steps: &TENS_OF_PERCENT,
    default: ParameterValue::Percent(50),
};
const RESONANCE: ParameterSpec = ParameterSpec {
    name: "Resonance",
    steps: &ZERO_TO_HUNDRED_PERCENT,
    default: ParameterValue::Percent(30),
};

/// The parameter of every effect, as its BEAT buttons step through it.
pub(crate) fn parameter_spec(kind: EffectKind) -> Option<&'static ParameterSpec> {
    match kind {
        EffectKind::Echo | EffectKind::PingPong => Some(&ECHO),
        EffectKind::Reverb => Some(&REVERB),
        EffectKind::LowPass | EffectKind::HighPass | EffectKind::BandPass => Some(&RESONANCE),
        EffectKind::Filter | EffectKind::Flanger => Some(&FILTER_SWEEP),
        EffectKind::Phaser => Some(&PHASER_SWEEP),
        EffectKind::Gate => Some(&GATE),
        EffectKind::Crusher => None,
        EffectKind::Brake => Some(&BRAKE),
        EffectKind::Backspin => Some(&BACKSPIN),
    }
}

#[cfg(test)]
mod tests {
    use super::{ParameterValue, SignedBeats};
    use crate::beat::BeatLength;
    use crate::config::EffectConfig;
    use crate::effect::{EffectKind, create};

    #[test]
    fn values_print_in_their_unit_and_parse_back() {
        let values = [
            ParameterValue::Beats(BeatLength::HALF),
            ParameterValue::SignedBeats(SignedBeats::falling(BeatLength::FOUR)),
            ParameterValue::SignedBeats(SignedBeats::rising(BeatLength::QUARTER)),
            ParameterValue::Percent(75),
            ParameterValue::Percent(-40),
            ParameterValue::Semitones(3),
            ParameterValue::Semitones(-12),
            ParameterValue::Semitones(0),
        ];
        let printed: Vec<String> = values.iter().map(ToString::to_string).collect();
        assert_eq!(
            printed,
            [
                "1/2", "-4", "+1/4", "75%", "-40%", "+3 st", "-12 st", "0 st"
            ]
        );
        for (value, text) in values.iter().zip(&printed) {
            assert_eq!(text.parse::<ParameterValue>().as_ref(), Ok(value));
        }
        assert!("loud".parse::<ParameterValue>().is_err());
    }

    #[test]
    fn steps_stop_at_either_end_and_snap_values_between_steps() {
        let spec = EffectKind::Echo.parameter().expect("echo has a beat");
        let half = ParameterValue::Beats(BeatLength::HALF);
        assert_eq!(
            spec.step(half, 1),
            ParameterValue::Beats(BeatLength::THREE_QUARTERS)
        );
        assert_eq!(
            spec.step(half, -100),
            ParameterValue::Beats(BeatLength::SIXTEENTH)
        );
        assert_eq!(
            spec.step(half, 100),
            ParameterValue::Beats(BeatLength::SIXTEEN)
        );
        assert_eq!(
            spec.step(ParameterValue::Beats(BeatLength::THIRD), 0),
            ParameterValue::Beats(BeatLength::QUARTER)
        );
        assert_eq!(spec.step(ParameterValue::Percent(40), 1), spec.default);
        let reverb = EffectKind::Reverb.parameter().expect("reverb has a size");
        assert_eq!(
            reverb.step(ParameterValue::Percent(50), -2),
            ParameterValue::Percent(30)
        );
    }

    #[test]
    fn every_effect_starts_at_its_default_and_keeps_every_step() {
        let config = EffectConfig {
            max_sample_rate: 48_000.0,
            max_delay_seconds: 2.0,
        };
        for kind in EffectKind::ALL {
            let mut effect = create(kind, &config);
            let Some(spec) = kind.parameter() else {
                assert_eq!(effect.parameter(), None, "{kind}");
                continue;
            };
            assert_eq!(effect.parameter(), Some(spec.default), "{kind}");
            for step in spec.steps {
                effect.set_parameter(*step);
                assert_eq!(effect.parameter(), Some(*step), "{kind}");
            }
            assert!(spec.steps.contains(&spec.default), "{kind}");
        }
    }
}
