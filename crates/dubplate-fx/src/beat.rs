use std::fmt;
use std::str::FromStr;

/// A musical length in beats, from 1/16 of a beat to 32 beats.
///
/// Beat-synced effects use it for their echo time, sweep period, gate step or stop time. The
/// length in seconds always follows the current tempo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BeatLength {
    numerator: u16,
    denominator: u16,
}

impl BeatLength {
    /// A sixteenth of a beat.
    pub const SIXTEENTH: Self = Self::fraction(1, 16);
    /// An eighth of a beat.
    pub const EIGHTH: Self = Self::fraction(1, 8);
    /// A quarter of a beat, one sixteenth note in 4/4.
    pub const QUARTER: Self = Self::fraction(1, 4);
    /// A third of a beat, an eighth-note triplet.
    pub const THIRD: Self = Self::fraction(1, 3);
    /// Three eighths of a beat, a dotted sixteenth note.
    pub const THREE_EIGHTHS: Self = Self::fraction(3, 8);
    /// Half a beat.
    pub const HALF: Self = Self::fraction(1, 2);
    /// Two thirds of a beat, a quarter-note triplet.
    pub const TWO_THIRDS: Self = Self::fraction(2, 3);
    /// Three quarters of a beat, a dotted eighth note.
    pub const THREE_QUARTERS: Self = Self::fraction(3, 4);
    /// One beat.
    pub const ONE: Self = Self::fraction(1, 1);
    /// One and a half beats.
    pub const ONE_AND_A_HALF: Self = Self::fraction(3, 2);
    /// Two beats.
    pub const TWO: Self = Self::fraction(2, 1);
    /// Four beats, one bar in 4/4.
    pub const FOUR: Self = Self::fraction(4, 1);
    /// Eight beats.
    pub const EIGHT: Self = Self::fraction(8, 1);
    /// Sixteen beats.
    pub const SIXTEEN: Self = Self::fraction(16, 1);
    /// Thirty-two beats, an eight-bar phrase.
    pub const THIRTY_TWO: Self = Self::fraction(32, 1);

    /// The lengths on the beat buttons of a DJ mixer, shortest first.
    pub const STANDARD: [Self; 11] = [
        Self::SIXTEENTH,
        Self::EIGHTH,
        Self::QUARTER,
        Self::HALF,
        Self::THREE_QUARTERS,
        Self::ONE,
        Self::TWO,
        Self::FOUR,
        Self::EIGHT,
        Self::SIXTEEN,
        Self::THIRTY_TWO,
    ];

    const fn fraction(numerator: u16, denominator: u16) -> Self {
        Self {
            numerator,
            denominator,
        }
    }

    /// Builds `numerator / denominator` beats, or `None` when it falls outside 1/16 to 32 beats.
    pub fn new(numerator: u16, denominator: u16) -> Option<Self> {
        if numerator == 0 || denominator == 0 {
            return None;
        }
        let beats = f64::from(numerator) / f64::from(denominator);
        let shortest = Self::SIXTEENTH.beats();
        let longest = Self::THIRTY_TWO.beats();
        (shortest..=longest)
            .contains(&beats)
            .then_some(Self::fraction(numerator, denominator))
    }

    /// The length in beats.
    pub fn beats(self) -> f64 {
        f64::from(self.numerator) / f64::from(self.denominator)
    }

    /// The length in seconds at `tempo_bpm`.
    pub fn seconds(self, tempo_bpm: f64) -> f64 {
        self.beats() * 60.0 / tempo_bpm
    }

    /// The next longer standard length, or this length when it is already the longest.
    pub fn longer(self) -> Self {
        Self::STANDARD
            .into_iter()
            .find(|standard| standard.beats() > self.beats())
            .unwrap_or(self)
    }

    /// The next shorter standard length, or this length when it is already the shortest.
    pub fn shorter(self) -> Self {
        Self::STANDARD
            .into_iter()
            .rev()
            .find(|standard| standard.beats() < self.beats())
            .unwrap_or(self)
    }

    pub(crate) fn clamped(self, shortest: Self, longest: Self) -> Self {
        if self.beats() < shortest.beats() {
            shortest
        } else if self.beats() > longest.beats() {
            longest
        } else {
            self
        }
    }
}

impl fmt::Display for BeatLength {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.denominator == 1 {
            write!(formatter, "{}", self.numerator)
        } else {
            write!(formatter, "{}/{}", self.numerator, self.denominator)
        }
    }
}

/// The error returned when text is not a beat length such as `1/2`, `3/4` or `4`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseBeatLengthError {
    text: String,
}

impl fmt::Display for ParseBeatLengthError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "`{}` is not a beat length from 1/16 to 32, such as 1/2, 3/4 or 4",
            self.text
        )
    }
}

impl std::error::Error for ParseBeatLengthError {}

impl FromStr for BeatLength {
    type Err = ParseBeatLengthError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let error = || ParseBeatLengthError {
            text: text.to_owned(),
        };
        let trimmed = text.trim();
        let (numerator, denominator) = match trimmed.split_once('/') {
            Some((numerator, denominator)) => (numerator.trim(), denominator.trim()),
            None => (trimmed, "1"),
        };
        let numerator = numerator.parse().map_err(|_| error())?;
        let denominator = denominator.parse().map_err(|_| error())?;
        Self::new(numerator, denominator).ok_or_else(error)
    }
}

#[cfg(test)]
mod tests {
    use super::BeatLength;

    #[test]
    fn parses_and_prints_fractions_and_whole_beats() {
        assert_eq!("1/2".parse::<BeatLength>(), Ok(BeatLength::HALF));
        assert_eq!(
            " 3 / 4 ".parse::<BeatLength>(),
            Ok(BeatLength::THREE_QUARTERS)
        );
        assert_eq!("32".parse::<BeatLength>(), Ok(BeatLength::THIRTY_TWO));
        assert_eq!(BeatLength::THREE_QUARTERS.to_string(), "3/4");
        assert_eq!(BeatLength::FOUR.to_string(), "4");
    }

    #[test]
    fn rejects_lengths_outside_the_range() {
        assert!("1/32".parse::<BeatLength>().is_err());
        assert!("64".parse::<BeatLength>().is_err());
        assert!("0".parse::<BeatLength>().is_err());
        assert!("half".parse::<BeatLength>().is_err());
    }

    #[test]
    fn steps_through_the_standard_lengths() {
        assert_eq!(BeatLength::HALF.longer(), BeatLength::THREE_QUARTERS);
        assert_eq!(BeatLength::THIRD.longer(), BeatLength::HALF);
        assert_eq!(BeatLength::THIRD.shorter(), BeatLength::QUARTER);
        assert_eq!(BeatLength::THIRTY_TWO.longer(), BeatLength::THIRTY_TWO);
        assert_eq!(BeatLength::SIXTEENTH.shorter(), BeatLength::SIXTEENTH);
    }

    #[test]
    fn converts_to_seconds_at_a_tempo() {
        assert!((BeatLength::HALF.seconds(120.0) - 0.25).abs() < 1e-12);
        assert!((BeatLength::FOUR.seconds(128.0) - 1.875).abs() < 1e-12);
    }
}
