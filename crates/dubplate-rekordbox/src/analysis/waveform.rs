use serde::{Serialize, Serializer};

use super::error::AnalysisError;
use super::reader::SectionReader;

/// Columns per second of audio in the scrolling detail waveforms.
pub const DETAIL_COLUMNS_PER_SECOND: u32 = 150;

/// A column of a blue waveform (`PWAV`, `PWV3`): a height and how white it is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct MonochromeColumn(pub u8);

impl MonochromeColumn {
    /// The height, from 0 to 31.
    pub fn height(self) -> u8 {
        self.0 & 0x1f
    }

    /// How white the column is drawn, from 0 (blue) to 7 (whitest).
    pub fn whiteness(self) -> u8 {
        self.0 >> 5
    }
}

/// A column of the tiny waveform preview (`PWV2`), a height from 0 to 15.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct TinyColumn(pub u8);

impl TinyColumn {
    /// The height, from 0 to 15.
    pub fn height(self) -> u8 {
        self.0 & 0x0f
    }
}

/// A column of the colour waveform preview (`PWV4`): six channels of one byte.
///
/// Bytes 0 and 1 shape the whiteness of the blue rendering, byte 2 is the energy in the lower
/// half of the spectrum, and bytes 3, 4 and 5 the energy in the lows, mids and highs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ColorPreviewColumn(pub [u8; 6]);

/// A column of the colour detail waveform (`PWV5`): 3-bit red, green and blue and a 5-bit height.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ColorDetailColumn(pub u16);

impl ColorDetailColumn {
    /// Red, from 0 to 7.
    pub fn red(self) -> u8 {
        ((self.0 >> 13) & 0x07) as u8
    }

    /// Green, from 0 to 7.
    pub fn green(self) -> u8 {
        ((self.0 >> 10) & 0x07) as u8
    }

    /// Blue, from 0 to 7.
    pub fn blue(self) -> u8 {
        ((self.0 >> 7) & 0x07) as u8
    }

    /// The height, from 0 to 31.
    pub fn height(self) -> u8 {
        ((self.0 >> 2) & 0x1f) as u8
    }
}

/// A column of a 3-band waveform (`PWV6`, `PWV7`): the level of the mids, highs and lows.
///
/// It serializes as `[mid, high, low]`, the order rekordbox stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThreeBandColumn {
    /// The mid-range level.
    pub mid: u8,
    /// The high-frequency level.
    pub high: u8,
    /// The low-frequency level.
    pub low: u8,
}

impl Serialize for ThreeBandColumn {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        [self.mid, self.high, self.low].serialize(serializer)
    }
}

/// The waveforms rekordbox drew for a track. Each is `None` when its section is absent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Waveforms {
    /// The 400-column blue overview (`PWAV`).
    pub preview: Option<Vec<MonochromeColumn>>,
    /// The 100-column overview for older players (`PWV2`).
    pub tiny_preview: Option<Vec<TinyColumn>>,
    /// The scrolling blue waveform, 150 columns a second (`PWV3`).
    pub detail: Option<Vec<MonochromeColumn>>,
    /// The 1200-column colour overview (`PWV4`).
    pub color_preview: Option<Vec<ColorPreviewColumn>>,
    /// The scrolling colour waveform, 150 columns a second (`PWV5`).
    pub color_detail: Option<Vec<ColorDetailColumn>>,
    /// The 1200-column 3-band overview (`PWV6`).
    pub three_band_preview: Option<Vec<ThreeBandColumn>>,
    /// The scrolling 3-band waveform, 150 columns a second (`PWV7`).
    pub three_band_detail: Option<Vec<ThreeBandColumn>>,
}

const PREVIEW_LENGTH_POSITION: usize = 0x0c;
const PREVIEW_DATA_POSITION: usize = 0x14;
const ENTRY_SIZE_POSITION: usize = 0x0c;
const ENTRY_COUNT_POSITION: usize = 0x10;
const DETAIL_DATA_POSITION: usize = 0x18;
const THREE_BAND_PREVIEW_DATA_POSITION: usize = 0x14;

/// Reads `PWAV` and `PWV2`, whose data length comes first.
pub(crate) fn parse_preview_bytes<'data>(
    reader: &SectionReader<'data>,
) -> Result<&'data [u8], AnalysisError> {
    let length = reader.u32_at(PREVIEW_LENGTH_POSITION, "the preview length")?;
    reader.expect_entries(PREVIEW_DATA_POSITION, length, 1, "preview columns")
}

/// Reads the sections that give an entry size and an entry count, then the entries.
pub(crate) fn parse_entries<'data, Column>(
    reader: &SectionReader<'data>,
    entry_size: usize,
    data_position: usize,
    column: impl Fn(&[u8]) -> Column,
) -> Result<Vec<Column>, AnalysisError> {
    let declared_size = reader.u32_at(ENTRY_SIZE_POSITION, "the entry size")?;
    if usize::try_from(declared_size).ok() != Some(entry_size) {
        return Err(reader.malformed(format!(
            "waveform entries are {declared_size} bytes, but this kind has {entry_size}"
        )));
    }
    let count = reader.u32_at(ENTRY_COUNT_POSITION, "the entry count")?;
    let data = reader.expect_entries(data_position, count, entry_size, "waveform columns")?;
    Ok(data.chunks_exact(entry_size).map(column).collect())
}

pub(crate) fn parse_detail(
    reader: &SectionReader<'_>,
) -> Result<Vec<MonochromeColumn>, AnalysisError> {
    parse_entries(reader, 1, DETAIL_DATA_POSITION, |bytes| {
        MonochromeColumn(bytes[0])
    })
}

pub(crate) fn parse_color_preview(
    reader: &SectionReader<'_>,
) -> Result<Vec<ColorPreviewColumn>, AnalysisError> {
    parse_entries(reader, 6, DETAIL_DATA_POSITION, |bytes| {
        ColorPreviewColumn([bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5]])
    })
}

pub(crate) fn parse_color_detail(
    reader: &SectionReader<'_>,
) -> Result<Vec<ColorDetailColumn>, AnalysisError> {
    parse_entries(reader, 2, DETAIL_DATA_POSITION, |bytes| {
        ColorDetailColumn(u16::from_be_bytes([bytes[0], bytes[1]]))
    })
}

pub(crate) fn parse_three_band_preview(
    reader: &SectionReader<'_>,
) -> Result<Vec<ThreeBandColumn>, AnalysisError> {
    parse_entries(reader, 3, THREE_BAND_PREVIEW_DATA_POSITION, three_band)
}

pub(crate) fn parse_three_band_detail(
    reader: &SectionReader<'_>,
) -> Result<Vec<ThreeBandColumn>, AnalysisError> {
    parse_entries(reader, 3, DETAIL_DATA_POSITION, three_band)
}

fn three_band(bytes: &[u8]) -> ThreeBandColumn {
    ThreeBandColumn {
        mid: bytes[0],
        high: bytes[1],
        low: bytes[2],
    }
}
