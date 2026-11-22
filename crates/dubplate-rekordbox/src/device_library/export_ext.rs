//! `exportExt.pdb`, the companion database rekordbox writes beside `export.pdb`. It holds the
//! My Tag categories and tags; Dubplate writes rekordbox's defaults, which a fresh rekordbox
//! export carries, and no track tags.

use serde::Serialize;

use super::database::{DatabaseFile, EncodedRow, TableRows, align4, get_u32, write_database};
use super::error::DeviceLibraryError;
use super::ids::LibraryId;
use super::string::{DeviceString, read_string};

const TABLE_COUNT: u32 = 9;
const TAGS_TABLE: u32 = 3;
const SETTINGS_TABLE: u32 = 7;
const TAG_SUBTYPE: u16 = 0x0680;
const TAG_NAME_POSITION: usize = 0x1f;
const SETTINGS_SUBTYPE: u16 = 0x0700;
const SETTINGS_LIBRARY_POSITION: usize = 0x18;
const SETTINGS_STRING_COUNT: u8 = 5;
const SETTINGS_ALLOCATION: usize = 60;
const EMPTY_STRING: u8 = 0x03;

/// A My Tag category or tag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MyTag {
    /// The name.
    pub name: String,
    /// The category a tag belongs to, or 0 for a category.
    pub category: u32,
    /// The place of a tag in its category, or of a category among the categories, from 0.
    pub position: u32,
    /// The id: a small number for a category, a fixed 32-bit number for a tag.
    pub id: u32,
    /// Whether the row is a category.
    pub is_category: bool,
}

/// rekordbox's default categories and tags, with the ids rekordbox gives them on every stick.
const DEFAULT_TAGS: [(&str, u32, u32, u32, bool); 28] = [
    ("Genre", 0, 0, 0x0000_0001, true),
    ("Acid House", 1, 0, 0x81e7_6ec2, false),
    ("Deep House", 1, 1, 0x159e_8f7a, false),
    ("Techno", 1, 2, 0xfdd8_6884, false),
    ("Nu Disco", 1, 3, 0xbf68_a216, false),
    ("Electro House", 1, 4, 0x6d84_a9d8, false),
    ("Bass Music", 1, 5, 0x89a5_05a0, false),
    ("Trap", 1, 6, 0x85f5_f834, false),
    ("Components", 0, 1, 0x0000_0002, true),
    ("Synth", 2, 0, 0xf110_e63a, false),
    ("Vocal", 2, 1, 0x0a0b_94b8, false),
    ("Beat", 2, 2, 0x2723_252e, false),
    ("Sub Bass", 2, 3, 0x3a02_e744, false),
    ("Percussion", 2, 4, 0x2b38_0bfd, false),
    ("Piano", 2, 5, 0xa34a_206b, false),
    ("Dark", 2, 6, 0x995b_2224, false),
    ("Upper", 2, 7, 0xd2d2_f2e1, false),
    ("Situation", 0, 2, 0x0000_0003, true),
    ("Main Floor", 3, 0, 0xce7f_2c3d, false),
    ("Second Floor", 3, 1, 0xe092_f07f, false),
    ("Lounge", 3, 2, 0x7333_805b, false),
    ("Mid Night", 3, 3, 0x81d6_3e03, false),
    ("Morning", 3, 4, 0x2481_0a9d, false),
    ("Build up", 3, 5, 0xbe08_d291, false),
    ("Peak Time", 3, 6, 0x67ee_afaa, false),
    ("Build down", 3, 7, 0x000e_6f9d, false),
    ("Untitled Column", 0, 3, 0x0000_0004, true),
    ("My Comment", 4, 0, 0x29d4_a76c, false),
];

/// The bytes of an `exportExt.pdb` for the library `library_id`, with rekordbox's default My
/// Tag categories and tags.
pub fn export_ext_pdb(library_id: LibraryId) -> Result<Vec<u8>, DeviceLibraryError> {
    let tags = DEFAULT_TAGS
        .into_iter()
        .map(|(name, category, position, id, is_category)| {
            encode_tag(&MyTag {
                name: name.to_owned(),
                category,
                position,
                id,
                is_category,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let tables: Vec<TableRows> = (0..TABLE_COUNT)
        .map(|type_number| TableRows {
            type_number,
            rows: match type_number {
                TAGS_TABLE => tags.clone(),
                SETTINGS_TABLE => vec![settings_row(library_id)],
                _ => Vec::new(),
            },
        })
        .collect();
    write_database(&tables)
}

fn encode_tag(tag: &MyTag) -> Result<EncodedRow, DeviceLibraryError> {
    let name = DeviceString::for_text(&tag.name);
    let mut bytes = Vec::with_capacity(TAG_NAME_POSITION + name.encoded_len() + 1);
    bytes.extend_from_slice(&TAG_SUBTYPE.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&tag.category.to_le_bytes());
    bytes.extend_from_slice(&tag.position.to_le_bytes());
    bytes.extend_from_slice(&tag.id.to_le_bytes());
    bytes.extend_from_slice(&[0, 0, 0, u8::from(tag.is_category)]);
    let after_name = TAG_NAME_POSITION + name.encoded_len();
    let after_name =
        u8::try_from(after_name).map_err(|_| DeviceLibraryError::StringTooLong(tag.name.len()))?;
    bytes.extend_from_slice(&[EMPTY_STRING, TAG_NAME_POSITION as u8, after_name]);
    name.write_to(&mut bytes)?;
    bytes.push(EMPTY_STRING);
    let allocation = align4(bytes.len()) + 8;
    Ok(EncodedRow::with_index_shift(bytes, allocation))
}

fn settings_row(library_id: LibraryId) -> EncodedRow {
    let mut bytes = vec![0_u8; SETTINGS_LIBRARY_POSITION];
    bytes[..2].copy_from_slice(&SETTINGS_SUBTYPE.to_le_bytes());
    bytes.extend_from_slice(&library_id.0.wrapping_add(1).to_le_bytes());
    bytes.push(EMPTY_STRING);
    let first_string = (bytes.len() + usize::from(SETTINGS_STRING_COUNT)) as u8;
    bytes.extend((0..SETTINGS_STRING_COUNT).map(|place| first_string + place));
    bytes.extend(std::iter::repeat_n(
        EMPTY_STRING,
        usize::from(SETTINGS_STRING_COUNT),
    ));
    EncodedRow::with_index_shift(bytes, SETTINGS_ALLOCATION)
}

/// The My Tag categories and tags of an `exportExt.pdb`, in file order.
pub fn parse_export_ext_tags(bytes: &[u8]) -> Result<Vec<MyTag>, DeviceLibraryError> {
    let database = DatabaseFile::parse(bytes)?;
    let Some(table) = database.table(TAGS_TABLE) else {
        return Ok(Vec::new());
    };
    table
        .rows
        .iter()
        .map(|row| {
            let field = |position| {
                get_u32(row, position).ok_or_else(|| {
                    DeviceLibraryError::NotADatabase("a My Tag row is cut short".to_owned())
                })
            };
            let name_position = usize::from(*row.get(0x1d).ok_or_else(|| {
                DeviceLibraryError::NotADatabase("a My Tag row is cut short".to_owned())
            })?);
            Ok(MyTag {
                name: read_string(row, name_position)?,
                category: field(0x0c)?,
                position: field(0x10)?,
                id: field(0x14)?,
                is_category: row.get(0x1b).is_some_and(|flag| *flag != 0),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn tag_rows_match_rekordbox_s() {
        let genre = encode_tag(&MyTag {
            name: "Genre".to_owned(),
            category: 0,
            position: 0,
            id: 1,
            is_category: true,
        })
        .unwrap();
        assert_eq!(
            hex(&genre.bytes),
            "80060000000000000000000000000000000000000100000000000001031f250d47656e726503"
        );
        assert_eq!(genre.allocation, 48);
    }

    #[test]
    fn the_settings_row_matches_rekordbox_s() {
        let row = settings_row(LibraryId(0x05e7_fa80));
        assert_eq!(
            hex(&row.bytes),
            "00070000000000000000000000000000000000000000000081fae7050322232425260303030303"
        );
    }

    #[test]
    fn the_default_tags_read_back() {
        let bytes = export_ext_pdb(LibraryId(7)).unwrap();
        let tags = parse_export_ext_tags(&bytes).unwrap();
        assert_eq!(tags.len(), 28);
        assert_eq!(tags[0].name, "Genre");
        assert!(tags[0].is_category);
        assert_eq!(tags[27].name, "My Comment");
        assert_eq!(tags[27].category, 4);
    }
}
