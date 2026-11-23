//! The rows rekordbox writes into the same tables of every export, whatever the library holds.
//! They were read from fresh exports of rekordbox 6 and 7, empty and with tracks; players read
//! the browse columns and track colours from them.

use super::database::{EncodedRow, align4};
use super::error::DeviceLibraryError;
use super::library::DeviceColor;
use super::string::DeviceString;

/// The browse columns: id, the code players use for the column, and its name.
const COLUMNS: [(u16, u16, &str); 27] = [
    (1, 0x0080, "GENRE"),
    (2, 0x0081, "ARTIST"),
    (3, 0x0082, "ALBUM"),
    (4, 0x0083, "TRACK"),
    (5, 0x0085, "BPM"),
    (6, 0x0086, "RATING"),
    (7, 0x0087, "YEAR"),
    (8, 0x0088, "REMIXER"),
    (9, 0x0089, "LABEL"),
    (10, 0x008a, "ORIGINAL ARTIST"),
    (11, 0x008b, "KEY"),
    (12, 0x008d, "CUE"),
    (13, 0x008e, "COLOR"),
    (14, 0x0092, "TIME"),
    (15, 0x0093, "BITRATE"),
    (16, 0x0094, "FILE NAME"),
    (17, 0x0084, "PLAYLIST"),
    (18, 0x0098, "HOT CUE BANK"),
    (19, 0x0095, "HISTORY"),
    (20, 0x0091, "SEARCH"),
    (21, 0x0096, "COMMENTS"),
    (22, 0x008c, "DATE ADDED"),
    (23, 0x0097, "DJ PLAY COUNT"),
    (24, 0x0090, "FOLDER"),
    (25, 0x00a1, "DEFAULT"),
    (26, 0x00a2, "ALPHABET"),
    (27, 0x00aa, "MATCHING"),
];

/// Column names are wrapped in the Unicode interlinear annotation marks.
const COLUMN_NAME_START: char = '\u{fffa}';
const COLUMN_NAME_END: char = '\u{fffb}';

/// The rows of table 17, four little-endian `u16` each.
const TABLE_17: [[u16; 4]; 22] = [
    [0x0001, 0x0001, 0x0163, 0x0000],
    [0x0005, 0x0006, 0x0105, 0x0000],
    [0x0006, 0x0007, 0x0163, 0x0000],
    [0x0007, 0x0008, 0x0163, 0x0000],
    [0x0008, 0x0009, 0x0163, 0x0000],
    [0x0009, 0x000a, 0x0163, 0x0000],
    [0x000a, 0x000b, 0x0163, 0x0000],
    [0x000d, 0x000f, 0x0163, 0x0000],
    [0x000e, 0x0013, 0x0104, 0x0000],
    [0x000f, 0x0014, 0x0106, 0x0000],
    [0x0010, 0x0015, 0x0163, 0x0000],
    [0x0012, 0x0017, 0x0163, 0x0000],
    [0x0002, 0x0002, 0x0002, 0x0001],
    [0x0003, 0x0003, 0x0003, 0x0002],
    [0x0004, 0x0004, 0x0001, 0x0003],
    [0x000b, 0x000c, 0x0063, 0x0004],
    [0x0011, 0x0005, 0x0063, 0x0005],
    [0x0013, 0x0016, 0x0063, 0x0006],
    [0x0014, 0x0012, 0x0063, 0x0007],
    [0x001b, 0x001a, 0x0263, 0x0008],
    [0x0018, 0x0011, 0x0063, 0x0009],
    [0x0016, 0x001b, 0x0063, 0x000a],
];

/// The rows of table 18, four little-endian `u16` each.
const TABLE_18: [[u16; 4]; 17] = [
    [0x0001, 0x0006, 0x0001, 0x0000],
    [0x0015, 0x0007, 0x0001, 0x0000],
    [0x000e, 0x0008, 0x0001, 0x0000],
    [0x0008, 0x0009, 0x0001, 0x0000],
    [0x0009, 0x000a, 0x0001, 0x0000],
    [0x000a, 0x000b, 0x0001, 0x0000],
    [0x000f, 0x000d, 0x0001, 0x0000],
    [0x000d, 0x000f, 0x0001, 0x0000],
    [0x0017, 0x0010, 0x0001, 0x0000],
    [0x0016, 0x0011, 0x0001, 0x0000],
    [0x0019, 0x0000, 0x0100, 0x0000],
    [0x001a, 0x0001, 0x0200, 0x0000],
    [0x0002, 0x0002, 0x0300, 0x0000],
    [0x0003, 0x0003, 0x0400, 0x0000],
    [0x0005, 0x0004, 0x0500, 0x0000],
    [0x0006, 0x0005, 0x0600, 0x0000],
    [0x000b, 0x000c, 0x0700, 0x0000],
];

const HISTORY_SYNC_SUBTYPE: u16 = 0x0280;
const HISTORY_SYNC_VERSION: &str = "1000";
const HISTORY_SYNC_ROW_LENGTH: usize = 40;
pub(crate) const HISTORY_SYNC_DATE_POSITION: usize = 0x0c;

pub(crate) fn color_rows() -> Result<Vec<EncodedRow>, DeviceLibraryError> {
    DeviceColor::ALL
        .into_iter()
        .map(|color| {
            let mut bytes = vec![0_u8; 8];
            bytes[4] = color.id();
            bytes[5..7].copy_from_slice(&u16::from(color.id()).to_le_bytes());
            let name = DeviceString::for_text(color.name());
            name.write_to(&mut bytes)?;
            Ok(EncodedRow::new(bytes, align4(8 + name.encoded_len())))
        })
        .collect()
}

pub(crate) fn column_rows() -> Result<Vec<EncodedRow>, DeviceLibraryError> {
    COLUMNS
        .into_iter()
        .map(|(id, code, name)| {
            let wrapped = format!("{COLUMN_NAME_START}{name}{COLUMN_NAME_END}");
            let text = DeviceString::Utf16(&wrapped);
            let mut bytes = Vec::with_capacity(4 + text.encoded_len());
            bytes.extend_from_slice(&id.to_le_bytes());
            bytes.extend_from_slice(&code.to_le_bytes());
            text.write_to(&mut bytes)?;
            Ok(EncodedRow::new(bytes, align4(4 + text.encoded_len())))
        })
        .collect()
}

fn u16_rows(rows: &[[u16; 4]]) -> Vec<EncodedRow> {
    rows.iter()
        .map(|values| {
            let bytes = values
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect();
            EncodedRow::new(bytes, 8)
        })
        .collect()
}

pub(crate) fn table_17_rows() -> Vec<EncodedRow> {
    u16_rows(&TABLE_17)
}

pub(crate) fn table_18_rows() -> Vec<EncodedRow> {
    u16_rows(&TABLE_18)
}

/// The history sync row of a fresh export made on `date`, written as `yyyy-mm-dd`: the date,
/// then rekordbox's version string and an empty string, each named by a one-byte offset.
pub(crate) fn history_sync_row(date: &str) -> Result<EncodedRow, DeviceLibraryError> {
    let mut bytes = vec![0_u8; HISTORY_SYNC_DATE_POSITION];
    bytes[..2].copy_from_slice(&HISTORY_SYNC_SUBTYPE.to_le_bytes());
    DeviceString::for_text(date).write_to(&mut bytes)?;
    let offsets_position = bytes.len();
    bytes.extend_from_slice(&[0, 0]);
    let version_position = bytes.len();
    DeviceString::for_text(HISTORY_SYNC_VERSION).write_to(&mut bytes)?;
    let empty_position = bytes.len();
    DeviceString::for_text("").write_to(&mut bytes)?;
    bytes[offsets_position] = u8::try_from(version_position)
        .map_err(|_| DeviceLibraryError::StringTooLong(date.len()))?;
    bytes[offsets_position + 1] =
        u8::try_from(empty_position).map_err(|_| DeviceLibraryError::StringTooLong(date.len()))?;
    Ok(EncodedRow::with_index_shift(bytes, HISTORY_SYNC_ROW_LENGTH))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_history_sync_row_is_rekordbox_s_byte_for_byte() {
        let row = history_sync_row("2022-02-02").unwrap();
        let expected = "80020000000000000000000017323032322d30322d3032191e0b313030300300";
        let hex: String = row.bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        assert_eq!(hex, &expected[..hex.len()]);
        assert_eq!(row.allocation, 40);
    }

    #[test]
    fn column_and_colour_rows_match_rekordbox_s() {
        let columns = column_rows().unwrap();
        assert_eq!(
            columns[0].bytes,
            [
                0x01, 0x00, 0x80, 0x00, 0x90, 0x12, 0x00, 0x00, 0xfa, 0xff, 0x47, 0x00, 0x45, 0x00,
                0x4e, 0x00, 0x52, 0x00, 0x45, 0x00, 0xfb, 0xff
            ]
        );
        assert_eq!(columns[0].allocation, 24);
        let colors = color_rows().unwrap();
        assert_eq!(
            colors[1].bytes,
            [0, 0, 0, 0, 2, 2, 0, 0, 0x09, b'R', b'e', b'd']
        );
        assert_eq!(colors[0].allocation, 16);
        assert_eq!(colors[1].allocation, 12);
    }
}
