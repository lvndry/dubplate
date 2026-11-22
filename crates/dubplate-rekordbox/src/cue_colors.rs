//! The colours rekordbox gives cues: hot cues pick from its 62-colour palette, memory cues and
//! saved loops from its eight memory colours.
//!
//! `master.db` keeps a hot cue's colour in `djmdCue.ColorTableIndex` and a memory cue's in
//! `djmdCue.Color`. A hot cue stored with index 0 has no colour of its own: rekordbox draws it
//! in its pad's default from the HOT CUE colour setting, COLORFUL unless the DJ changed it.

use std::fmt;

use serde::{Serialize, Serializer};

use crate::model::HotCuePad;

/// A colour as red, green and blue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb {
    /// Red, 0 to 255.
    pub red: u8,
    /// Green, 0 to 255.
    pub green: u8,
    /// Blue, 0 to 255.
    pub blue: u8,
}

impl Rgb {
    const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }
}

impl fmt::Display for Rgb {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "#{:02x}{:02x}{:02x}",
            self.red, self.green, self.blue
        )
    }
}

impl Serialize for Rgb {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// A colour of rekordbox's hot cue palette, by its place in the colour table, from 1 to 62.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct PaletteColor(u8);

impl PaletteColor {
    /// The colour at `index` in rekordbox's colour table, or `None` for 0 (no colour of its
    /// own) and for any index outside the palette.
    pub fn from_index(index: i64) -> Option<Self> {
        let index = u8::try_from(index).ok()?;
        let place = usize::from(index.checked_sub(1)?);
        (place < HOT_CUE_PALETTE.len()).then_some(Self(index))
    }

    /// Its place in the colour table, as `ColorTableIndex` and the analysis files store it.
    pub fn index(self) -> u8 {
        self.0
    }

    /// The colour rekordbox draws.
    pub fn rgb(self) -> Rgb {
        HOT_CUE_PALETTE[usize::from(self.0 - 1)]
    }

    /// The colour rekordbox draws a hot cue on `pad` in when the cue has none of its own, with
    /// the HOT CUE colour setting on COLORFUL, rekordbox's default.
    pub fn pad_default(pad: HotCuePad) -> Self {
        Self(COLORFUL_PAD_DEFAULTS[pad.index()])
    }

    /// The colour rekordbox draws a hot loop in when it has none of its own: orange.
    pub const HOT_LOOP_DEFAULT: Self = Self(36);
}

/// The defaults of pads A to H in the COLORFUL table: pink-red, light blue, green, purple,
/// green, orange, blue and yellow. Read from rekordbox 7.2.19's `hotCueColorIndexColorfull`.
const COLORFUL_PAD_DEFAULTS: [u8; HotCuePad::ALL.len()] = [43, 8, 23, 60, 19, 38, 1, 32];

/// The colour of a memory cue or saved loop, as rekordbox's MEMORY list offers them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryCueColor {
    /// Pink, stored as 0.
    Pink,
    /// Red, stored as 1.
    Red,
    /// Orange, stored as 2.
    Orange,
    /// Yellow, stored as 3.
    Yellow,
    /// Green, stored as 4.
    Green,
    /// Aqua, stored as 5.
    Aqua,
    /// Blue, stored as 6.
    Blue,
    /// Purple, stored as 7.
    Purple,
}

impl MemoryCueColor {
    /// Every memory colour, in the order rekordbox stores them.
    pub const ALL: [Self; 8] = [
        Self::Pink,
        Self::Red,
        Self::Orange,
        Self::Yellow,
        Self::Green,
        Self::Aqua,
        Self::Blue,
        Self::Purple,
    ];

    /// The colour `djmdCue.Color` names, or `None` for -1, 255 and anything else rekordbox
    /// draws as an uncoloured (red) memory cue.
    pub fn from_rekordbox(value: i64) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|color| i64::from(color.rekordbox_value()) == value)
    }

    /// The value `djmdCue.Color` stores for it.
    pub fn rekordbox_value(self) -> u8 {
        match self {
            Self::Pink => 0,
            Self::Red => 1,
            Self::Orange => 2,
            Self::Yellow => 3,
            Self::Green => 4,
            Self::Aqua => 5,
            Self::Blue => 6,
            Self::Purple => 7,
        }
    }

    /// Its name in rekordbox's MEMORY list, in lowercase.
    pub fn name(self) -> &'static str {
        match self {
            Self::Pink => "pink",
            Self::Red => "red",
            Self::Orange => "orange",
            Self::Yellow => "yellow",
            Self::Green => "green",
            Self::Aqua => "aqua",
            Self::Blue => "blue",
            Self::Purple => "purple",
        }
    }

    /// The colour rekordbox draws its marker in.
    pub fn rgb(self) -> Rgb {
        match self {
            Self::Pink => Rgb::new(0xf8, 0x70, 0xf8),
            Self::Red => Rgb::new(0xff, 0x00, 0x00),
            Self::Orange => Rgb::new(0xf8, 0xa0, 0x30),
            Self::Yellow => Rgb::new(0xf8, 0xe3, 0x30),
            Self::Green => Rgb::new(0x00, 0xe0, 0x00),
            Self::Aqua => Rgb::new(0x00, 0xc0, 0xf8),
            Self::Blue => Rgb::new(0x00, 0x50, 0xf8),
            Self::Purple => Rgb::new(0x98, 0x08, 0xf8),
        }
    }
}

/// The colours of rekordbox's hot cue palette, by index from 1, as rekordbox draws them.
const HOT_CUE_PALETTE: [Rgb; 62] = [
    Rgb::new(0x30, 0x5a, 0xff),
    Rgb::new(0x50, 0x73, 0xff),
    Rgb::new(0x50, 0x8c, 0xff),
    Rgb::new(0x50, 0xa0, 0xff),
    Rgb::new(0x50, 0xb4, 0xff),
    Rgb::new(0x50, 0xb0, 0xf2),
    Rgb::new(0x50, 0xae, 0xe8),
    Rgb::new(0x45, 0xac, 0xdb),
    Rgb::new(0x00, 0xe0, 0xff),
    Rgb::new(0x19, 0xda, 0xf0),
    Rgb::new(0x32, 0xd2, 0xe6),
    Rgb::new(0x21, 0xb4, 0xb9),
    Rgb::new(0x20, 0xaa, 0xa0),
    Rgb::new(0x1f, 0xa3, 0x92),
    Rgb::new(0x19, 0xa0, 0x8c),
    Rgb::new(0x14, 0xa5, 0x84),
    Rgb::new(0x14, 0xaa, 0x7d),
    Rgb::new(0x10, 0xb1, 0x76),
    Rgb::new(0x30, 0xd2, 0x6e),
    Rgb::new(0x37, 0xde, 0x5a),
    Rgb::new(0x3c, 0xeb, 0x50),
    Rgb::new(0x28, 0xe2, 0x14),
    Rgb::new(0x7d, 0xc1, 0x3d),
    Rgb::new(0x8c, 0xc8, 0x32),
    Rgb::new(0x9b, 0xd7, 0x23),
    Rgb::new(0xa5, 0xe1, 0x16),
    Rgb::new(0xa5, 0xdc, 0x0a),
    Rgb::new(0xaa, 0xd2, 0x08),
    Rgb::new(0xb4, 0xc8, 0x05),
    Rgb::new(0xb4, 0xbe, 0x04),
    Rgb::new(0xba, 0xb4, 0x04),
    Rgb::new(0xc3, 0xaf, 0x04),
    Rgb::new(0xe1, 0xaa, 0x00),
    Rgb::new(0xff, 0xa0, 0x00),
    Rgb::new(0xff, 0x96, 0x00),
    Rgb::new(0xff, 0x8c, 0x00),
    Rgb::new(0xff, 0x75, 0x00),
    Rgb::new(0xe0, 0x64, 0x1b),
    Rgb::new(0xe0, 0x46, 0x1e),
    Rgb::new(0xe0, 0x30, 0x1e),
    Rgb::new(0xe0, 0x28, 0x23),
    Rgb::new(0xe6, 0x28, 0x28),
    Rgb::new(0xff, 0x37, 0x6f),
    Rgb::new(0xff, 0x2d, 0x6f),
    Rgb::new(0xff, 0x12, 0x7b),
    Rgb::new(0xf5, 0x1e, 0x8c),
    Rgb::new(0xeb, 0x2d, 0xa0),
    Rgb::new(0xe6, 0x37, 0xb4),
    Rgb::new(0xde, 0x44, 0xcf),
    Rgb::new(0xde, 0x44, 0x8d),
    Rgb::new(0xe6, 0x30, 0xb4),
    Rgb::new(0xe6, 0x19, 0xdc),
    Rgb::new(0xe6, 0x00, 0xff),
    Rgb::new(0xdc, 0x00, 0xff),
    Rgb::new(0xcc, 0x00, 0xff),
    Rgb::new(0xb4, 0x32, 0xff),
    Rgb::new(0xb9, 0x3c, 0xff),
    Rgb::new(0xc5, 0x42, 0xff),
    Rgb::new(0xaa, 0x5a, 0xff),
    Rgb::new(0xaa, 0x72, 0xff),
    Rgb::new(0x82, 0x72, 0xff),
    Rgb::new(0x64, 0x73, 0xff),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_indexes_in_the_palette_are_colours() {
        assert_eq!(PaletteColor::from_index(0), None);
        assert_eq!(PaletteColor::from_index(-1), None);
        assert_eq!(PaletteColor::from_index(63), None);
        assert_eq!(PaletteColor::from_index(256), None);
        let first = PaletteColor::from_index(1).unwrap();
        assert_eq!(first.rgb().to_string(), "#305aff");
        assert_eq!(PaletteColor::from_index(62).unwrap().index(), 62);
    }

    #[test]
    fn uncoloured_cues_take_rekordbox_s_colorful_defaults() {
        let defaults: Vec<String> = HotCuePad::ALL
            .into_iter()
            .map(|pad| PaletteColor::pad_default(pad).rgb().to_string())
            .collect();
        assert_eq!(
            defaults,
            [
                "#ff376f", "#45acdb", "#7dc13d", "#aa72ff", "#30d26e", "#e0641b", "#305aff",
                "#c3af04"
            ]
        );
        assert_eq!(PaletteColor::HOT_LOOP_DEFAULT.rgb().to_string(), "#ff8c00");
    }

    #[test]
    fn memory_colours_round_trip_and_unknown_values_have_none() {
        for color in MemoryCueColor::ALL {
            assert_eq!(
                MemoryCueColor::from_rekordbox(i64::from(color.rekordbox_value())),
                Some(color)
            );
        }
        assert_eq!(MemoryCueColor::from_rekordbox(-1), None);
        assert_eq!(MemoryCueColor::from_rekordbox(255), None);
        assert_eq!(
            MemoryCueColor::from_rekordbox(6),
            Some(MemoryCueColor::Blue)
        );
    }
}
