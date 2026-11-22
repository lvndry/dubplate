//! MIDI data values and what they mean on a DJ controller: fader positions and encoder steps.

use std::fmt;

use serde::Deserialize;

/// A 7-bit MIDI data value, 0 to 127.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct U7(u8);

impl U7 {
    /// Zero.
    pub const MIN: U7 = U7(0);
    /// The largest 7-bit value, 127.
    pub const MAX: U7 = U7(0x7F);

    /// The value, or `None` above 127.
    pub const fn new(value: u8) -> Option<U7> {
        if value <= Self::MAX.0 {
            Some(U7(value))
        } else {
            None
        }
    }

    /// The value as a byte.
    pub const fn get(self) -> u8 {
        self.0
    }
}

/// A 14-bit MIDI value, 0 to 16383, which a controller sends as two 7-bit halves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct U14(u16);

impl U14 {
    /// The largest 14-bit value, 16383.
    pub const MAX: U14 = U14(0x3FFF);
    /// The middle of the range, 8192, where a centre-detent control rests.
    pub const CENTER: U14 = U14(0x2000);

    /// The value, or `None` above 16383.
    pub const fn new(value: u16) -> Option<U14> {
        if value <= Self::MAX.0 {
            Some(U14(value))
        } else {
            None
        }
    }

    /// Joins the most and least significant halves.
    pub const fn from_halves(most_significant: U7, least_significant: U7) -> U14 {
        U14(((most_significant.0 as u16) << 7) | least_significant.0 as u16)
    }

    /// The value as an integer.
    pub const fn get(self) -> u16 {
        self.0
    }
}

impl From<U7> for U14 {
    /// Spreads a 7-bit value over the 14-bit range so 0, 64 and 127 land exactly on the bottom,
    /// the centre and the top. A 7-bit tempo fader at its detent then reads as zero.
    fn from(value: U7) -> U14 {
        let coarse = u32::from(value.0);
        let center = u32::from(U14::CENTER.0);
        let fine = if coarse <= 64 {
            coarse * 128
        } else {
            center + (coarse - 64) * (u32::from(U14::MAX.0) - center) / 63
        };
        U14(fine as u16)
    }
}

/// Where a fader or knob sits along its travel, at 14-bit resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Position(U14);

impl Position {
    /// A position from the raw value the controller sent.
    pub const fn new(raw: U14) -> Position {
        Position(raw)
    }

    /// The raw value the controller sent.
    pub const fn raw(self) -> U14 {
        self.0
    }

    /// 0.0 at the bottom, left or fully counter-clockwise end, 1.0 at the other end.
    pub fn fraction(self) -> f32 {
        f32::from(self.0.get()) / f32::from(U14::MAX.get())
    }

    /// -1.0 to 1.0 with the centre at exactly 0.0, for tempo faders and centre-detent knobs.
    pub fn bipolar(self) -> f32 {
        let center = f32::from(U14::CENTER.get());
        let half_range = f32::from(U14::MAX.get()) - center;
        ((f32::from(self.0.get()) - center) / half_range).max(-1.0)
    }
}

/// Signed steps a jog wheel or encoder moved since its last message. Positive is clockwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ticks(i8);

impl Ticks {
    /// Ticks from a signed count.
    pub const fn new(count: i8) -> Ticks {
        Ticks(count)
    }

    /// The signed count.
    pub const fn get(self) -> i8 {
        self.0
    }
}

impl fmt::Display for Ticks {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:+}", self.0)
    }
}

/// How an encoder packs a signed step count into a 7-bit value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum RelativeEncoding {
    /// 64 means no movement, 65 is one step clockwise, 63 one step counter-clockwise.
    #[serde(rename = "offset-64")]
    Offset64,
    /// 1 is one step clockwise, 127 one step counter-clockwise (7-bit two's complement).
    TwosComplement,
}

impl RelativeEncoding {
    pub(crate) const fn decode(self, value: U7) -> Ticks {
        match self {
            RelativeEncoding::Offset64 => Ticks(value.0.cast_signed() - 64),
            RelativeEncoding::TwosComplement => Ticks((value.0 << 1).cast_signed() >> 1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u7(value: u8) -> U7 {
        U7::new(value).unwrap()
    }

    #[test]
    fn values_reject_out_of_range() {
        assert_eq!(U7::new(128), None);
        assert_eq!(U14::new(16384), None);
        assert_eq!(U14::new(16383), Some(U14::MAX));
    }

    #[test]
    fn halves_join_most_significant_first() {
        assert_eq!(U14::from_halves(u7(0x7F), u7(0x7F)), U14::MAX);
        assert_eq!(U14::from_halves(u7(0x40), u7(0x00)), U14::CENTER);
        assert_eq!(U14::from_halves(u7(0x01), u7(0x02)).get(), 130);
    }

    #[test]
    fn seven_bit_values_widen_onto_bottom_centre_and_top() {
        assert_eq!(U14::from(u7(0)), U14::new(0).unwrap());
        assert_eq!(U14::from(u7(64)), U14::CENTER);
        assert_eq!(U14::from(u7(127)), U14::MAX);
        let widened: Vec<u16> = (0..=127).map(|value| U14::from(u7(value)).get()).collect();
        assert!(widened.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn positions_read_as_fraction_and_bipolar() {
        let bottom = Position::new(U14::new(0).unwrap());
        let center = Position::new(U14::CENTER);
        let top = Position::new(U14::MAX);
        assert_eq!(bottom.fraction(), 0.0);
        assert_eq!(top.fraction(), 1.0);
        assert_eq!(bottom.bipolar(), -1.0);
        assert_eq!(center.bipolar(), 0.0);
        assert_eq!(top.bipolar(), 1.0);
    }

    #[test]
    fn offset_encoding_is_centred_on_64() {
        let encoding = RelativeEncoding::Offset64;
        assert_eq!(encoding.decode(u7(0x41)), Ticks(1));
        assert_eq!(encoding.decode(u7(0x3F)), Ticks(-1));
        assert_eq!(encoding.decode(u7(0x40)), Ticks(0));
        assert_eq!(encoding.decode(u7(0x7F)), Ticks(63));
        assert_eq!(encoding.decode(u7(0x00)), Ticks(-64));
    }

    #[test]
    fn twos_complement_encoding_wraps_at_seven_bits() {
        let encoding = RelativeEncoding::TwosComplement;
        assert_eq!(encoding.decode(u7(0x01)), Ticks(1));
        assert_eq!(encoding.decode(u7(0x7F)), Ticks(-1));
        assert_eq!(encoding.decode(u7(0x3F)), Ticks(63));
        assert_eq!(encoding.decode(u7(0x40)), Ticks(-64));
    }
}
