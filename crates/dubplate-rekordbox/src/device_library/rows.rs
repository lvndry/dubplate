//! The row layouts of `export.pdb`'s tables: each library record to the bytes rekordbox writes
//! for it, and back.

use super::TableKind;
use super::database::{EncodedRow, align4, get_u16, get_u32};
use super::error::DeviceLibraryError;
use super::ids::{
    AlbumId, ArtistId, ArtworkId, GenreId, KeyId, LabelId, LibraryId, PlaylistId, TrackId,
    number_or_zero,
};
use super::library::{
    Album, Artwork, DeviceColor, DeviceTrack, FileType, Named, PlaylistEntry, PlaylistNode,
};
use super::string::{DeviceString, read_string};

const TRACK_SUBTYPE: u16 = 0x0024;
const TRACK_FLAGS: u32 = 0x000c_0700;
const TRACK_UNKNOWN_56: u16 = 0x0029;
const TRACK_UNKNOWN_5C: u16 = 0x0003;
const TRACK_STRINGS_POSITION: usize = 0x5e;
const TRACK_STRING_COUNT: usize = 21;
const TRACK_FIXED_LENGTH: usize = 0x88;
const ARTIST_SUBTYPE: u16 = 0x0060;
const ARTIST_NAME_POSITION: usize = 0x0a;
const ALBUM_SUBTYPE: u16 = 0x0080;
const ALBUM_NAME_POSITION: usize = 0x16;
const FAR_OFFSET_FLAG: u16 = 0x0004;
const EMPTY_STRING: u8 = 0x03;
const PLAYLIST_NAME_POSITION: usize = 20;
/// What rekordbox writes in track string slots 2 and 3, whose meaning nobody knows.
const TRACK_UNKNOWN_STRING: &str = "2";
const ON: &str = "ON";

/// The track string slots, in the order of their offsets after the fixed part of the row.
#[derive(Debug, Clone, Copy)]
enum TrackString {
    Isrc = 0,
    Lyricist = 1,
    Unknown2 = 2,
    Unknown3 = 3,
    Message = 5,
    KuvoPublic = 6,
    AutoloadHotCues = 7,
    DateAdded = 10,
    ReleaseDate = 11,
    MixName = 12,
    AnalyzePath = 14,
    AnalyzeDate = 15,
    Comment = 16,
    Title = 17,
    FileName = 19,
    FilePath = 20,
}

fn on_or_empty(flag: bool) -> &'static str {
    if flag { ON } else { "" }
}

pub(crate) fn encode_track(
    track: &DeviceTrack,
    library_id: LibraryId,
) -> Result<EncodedRow, DeviceLibraryError> {
    let file_name = track.file_name();
    let mut strings: [&str; TRACK_STRING_COUNT] = [""; TRACK_STRING_COUNT];
    strings[TrackString::Isrc as usize] = &track.isrc;
    strings[TrackString::Lyricist as usize] = &track.lyricist;
    strings[TrackString::Unknown2 as usize] = TRACK_UNKNOWN_STRING;
    strings[TrackString::Unknown3 as usize] = TRACK_UNKNOWN_STRING;
    strings[TrackString::Message as usize] = &track.message;
    strings[TrackString::KuvoPublic as usize] = on_or_empty(track.kuvo_public);
    strings[TrackString::AutoloadHotCues as usize] = on_or_empty(track.autoload_hot_cues);
    strings[TrackString::DateAdded as usize] = &track.date_added;
    strings[TrackString::ReleaseDate as usize] = &track.release_date;
    strings[TrackString::MixName as usize] = &track.mix_name;
    strings[TrackString::AnalyzePath as usize] = &track.analyze_path;
    strings[TrackString::AnalyzeDate as usize] = &track.analyze_date;
    strings[TrackString::Comment as usize] = &track.comment;
    strings[TrackString::Title as usize] = &track.title;
    strings[TrackString::FileName as usize] = file_name;
    strings[TrackString::FilePath as usize] = &track.file_path;

    let mut bytes = vec![0_u8; TRACK_FIXED_LENGTH];
    let mut fixed = FieldWriter(&mut bytes);
    fixed.u16(0x00, TRACK_SUBTYPE);
    fixed.u32(0x04, TRACK_FLAGS);
    fixed.u32(0x08, track.sample_rate);
    fixed.u32(0x0c, number_or_zero(track.composer));
    fixed.u32(0x10, track.file_size);
    fixed.u32(0x14, track.checksum);
    fixed.u32(0x18, library_id.0);
    fixed.u32(0x1c, number_or_zero(track.artwork));
    fixed.u32(0x20, number_or_zero(track.key));
    fixed.u32(0x24, number_or_zero(track.original_artist));
    fixed.u32(0x28, number_or_zero(track.label));
    fixed.u32(0x2c, number_or_zero(track.remixer));
    fixed.u32(0x30, track.bitrate);
    fixed.u32(0x34, track.track_number);
    fixed.u32(0x38, track.tempo_hundredths);
    fixed.u32(0x3c, number_or_zero(track.genre));
    fixed.u32(0x40, number_or_zero(track.album));
    fixed.u32(0x44, number_or_zero(track.artist));
    fixed.u32(0x48, track.id.get());
    fixed.u16(0x4c, track.disc_number);
    fixed.u16(0x4e, track.play_count);
    fixed.u16(0x50, track.year);
    fixed.u16(0x52, track.sample_depth);
    fixed.u16(0x54, track.duration_seconds);
    fixed.u16(0x56, TRACK_UNKNOWN_56);
    fixed.u8(0x58, track.color.map_or(0, DeviceColor::id));
    fixed.u8(0x59, track.rating);
    fixed.u16(0x5a, track.file_type.code());
    fixed.u16(0x5c, TRACK_UNKNOWN_5C);

    let mut reserved = TRACK_FIXED_LENGTH;
    for (slot, text) in strings.into_iter().enumerate() {
        let string = DeviceString::for_text(text);
        if string.is_long() {
            bytes.resize(align4(bytes.len()), 0);
        }
        let offset = u16::try_from(bytes.len()).map_err(|_| DeviceLibraryError::RowTooLarge {
            table: TableKind::Tracks,
            length: bytes.len(),
        })?;
        FieldWriter(&mut bytes).u16(TRACK_STRINGS_POSITION + 2 * slot, offset);
        string.write_to(&mut bytes)?;
        reserved += align4(string.encoded_len());
    }
    let allocation = reserved.max(bytes.len()) + 4;
    Ok(EncodedRow::with_index_shift(bytes, allocation))
}

pub(crate) fn decode_track(row: &[u8]) -> Result<(DeviceTrack, LibraryId), DeviceLibraryError> {
    let fields = FieldReader::new(row, TableKind::Tracks);
    let string = |slot: TrackString| -> Result<String, DeviceLibraryError> {
        let offset = fields.u16(TRACK_STRINGS_POSITION + 2 * slot as usize)?;
        read_string(row, usize::from(offset))
    };
    let id = TrackId::new(fields.u32(0x48)?).ok_or_else(|| fields.malformed("its id is 0"))?;
    let color = fields.u8(0x58)?;
    let track = DeviceTrack {
        id,
        title: string(TrackString::Title)?,
        artist: ArtistId::new(fields.u32(0x44)?),
        composer: ArtistId::new(fields.u32(0x0c)?),
        original_artist: ArtistId::new(fields.u32(0x24)?),
        remixer: ArtistId::new(fields.u32(0x2c)?),
        album: AlbumId::new(fields.u32(0x40)?),
        genre: GenreId::new(fields.u32(0x3c)?),
        label: LabelId::new(fields.u32(0x28)?),
        key: KeyId::new(fields.u32(0x20)?),
        artwork: ArtworkId::new(fields.u32(0x1c)?),
        color: DeviceColor::from_id(color),
        rating: fields.u8(0x59)?,
        tempo_hundredths: fields.u32(0x38)?,
        duration_seconds: fields.u16(0x54)?,
        sample_rate: fields.u32(0x08)?,
        sample_depth: fields.u16(0x52)?,
        bitrate: fields.u32(0x30)?,
        file_size: fields.u32(0x10)?,
        file_type: FileType::from_code(fields.u16(0x5a)?),
        track_number: fields.u32(0x34)?,
        disc_number: fields.u16(0x4c)?,
        year: fields.u16(0x50)?,
        play_count: fields.u16(0x4e)?,
        checksum: fields.u32(0x14)?,
        file_path: string(TrackString::FilePath)?,
        analyze_path: string(TrackString::AnalyzePath)?,
        analyze_date: string(TrackString::AnalyzeDate)?,
        date_added: string(TrackString::DateAdded)?,
        release_date: string(TrackString::ReleaseDate)?,
        comment: string(TrackString::Comment)?,
        mix_name: string(TrackString::MixName)?,
        isrc: string(TrackString::Isrc)?,
        lyricist: string(TrackString::Lyricist)?,
        message: string(TrackString::Message)?,
        kuvo_public: string(TrackString::KuvoPublic)? == ON,
        autoload_hot_cues: string(TrackString::AutoloadHotCues)? == ON,
    };
    Ok((track, LibraryId(fields.u32(0x18)?)))
}

pub(crate) fn encode_artist(artist: &Named<ArtistId>) -> Result<EncodedRow, DeviceLibraryError> {
    let name = DeviceString::for_text(&artist.name);
    let mut bytes = Vec::with_capacity(ARTIST_NAME_POSITION + name.encoded_len());
    bytes.extend_from_slice(&ARTIST_SUBTYPE.to_le_bytes());
    bytes.extend_from_slice(&[0, 0]);
    bytes.extend_from_slice(&artist.id.get().to_le_bytes());
    bytes.extend_from_slice(&[EMPTY_STRING, ARTIST_NAME_POSITION as u8]);
    name.write_to(&mut bytes)?;
    let allocation = align4(ARTIST_NAME_POSITION + align4(name.encoded_len())) + 4;
    Ok(EncodedRow::with_index_shift(bytes, allocation))
}

pub(crate) fn decode_artist(row: &[u8]) -> Result<Named<ArtistId>, DeviceLibraryError> {
    let fields = FieldReader::new(row, TableKind::Artists);
    let name_position = if fields.u16(0)? & FAR_OFFSET_FLAG == FAR_OFFSET_FLAG {
        usize::from(fields.u16(0x0a)?)
    } else {
        usize::from(fields.u8(0x09)?)
    };
    Ok(Named {
        id: ArtistId::new(fields.u32(0x04)?).ok_or_else(|| fields.malformed("its id is 0"))?,
        name: read_string(row, name_position)?,
    })
}

pub(crate) fn encode_album(album: &Album) -> Result<EncodedRow, DeviceLibraryError> {
    let name = DeviceString::for_text(&album.name);
    let mut bytes = Vec::with_capacity(ALBUM_NAME_POSITION + name.encoded_len());
    bytes.extend_from_slice(&ALBUM_SUBTYPE.to_le_bytes());
    bytes.extend_from_slice(&[0, 0]);
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&number_or_zero(album.artist).to_le_bytes());
    bytes.extend_from_slice(&album.id.get().to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&[EMPTY_STRING, ALBUM_NAME_POSITION as u8]);
    name.write_to(&mut bytes)?;
    let allocation = align4(ALBUM_NAME_POSITION + align4(name.encoded_len())) + 4;
    Ok(EncodedRow::with_index_shift(bytes, allocation))
}

pub(crate) fn decode_album(row: &[u8]) -> Result<Album, DeviceLibraryError> {
    let fields = FieldReader::new(row, TableKind::Albums);
    let name_position = if fields.u16(0)? & FAR_OFFSET_FLAG == FAR_OFFSET_FLAG {
        usize::from(fields.u16(ALBUM_NAME_POSITION)?)
    } else {
        usize::from(fields.u8(0x15)?)
    };
    Ok(Album {
        id: AlbumId::new(fields.u32(0x0c)?).ok_or_else(|| fields.malformed("its id is 0"))?,
        artist: ArtistId::new(fields.u32(0x08)?),
        name: read_string(row, name_position)?,
    })
}

/// A row of an id and a name: genres, labels and artwork paths.
pub(crate) fn encode_id_and_text(id: u32, text: &str) -> Result<EncodedRow, DeviceLibraryError> {
    let text = DeviceString::for_text(text);
    let mut bytes = Vec::with_capacity(4 + text.encoded_len());
    bytes.extend_from_slice(&id.to_le_bytes());
    text.write_to(&mut bytes)?;
    Ok(EncodedRow::new(bytes, align4(4 + text.encoded_len())))
}

pub(crate) fn decode_id_and_text(
    row: &[u8],
    table: TableKind,
) -> Result<(u32, String), DeviceLibraryError> {
    let fields = FieldReader::new(row, table);
    Ok((fields.u32(0)?, read_string(row, 4)?))
}

pub(crate) fn encode_key(key: &Named<KeyId>) -> Result<EncodedRow, DeviceLibraryError> {
    let name = DeviceString::for_text(&key.name);
    let mut bytes = Vec::with_capacity(8 + name.encoded_len());
    bytes.extend_from_slice(&key.id.get().to_le_bytes());
    bytes.extend_from_slice(&key.id.get().to_le_bytes());
    name.write_to(&mut bytes)?;
    Ok(EncodedRow::new(bytes, align4(8 + name.encoded_len())))
}

pub(crate) fn decode_key(row: &[u8]) -> Result<Named<KeyId>, DeviceLibraryError> {
    let fields = FieldReader::new(row, TableKind::Keys);
    Ok(Named {
        id: KeyId::new(fields.u32(0)?).ok_or_else(|| fields.malformed("its id is 0"))?,
        name: read_string(row, 8)?,
    })
}

pub(crate) fn encode_playlist(node: &PlaylistNode) -> Result<EncodedRow, DeviceLibraryError> {
    let name = DeviceString::for_text(&node.name);
    let mut bytes = Vec::with_capacity(PLAYLIST_NAME_POSITION + name.encoded_len());
    bytes.extend_from_slice(&number_or_zero(node.parent).to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&node.sort_order.to_le_bytes());
    bytes.extend_from_slice(&node.id.get().to_le_bytes());
    bytes.extend_from_slice(&u32::from(node.is_folder).to_le_bytes());
    name.write_to(&mut bytes)?;
    Ok(EncodedRow::new(
        bytes,
        align4(PLAYLIST_NAME_POSITION + name.encoded_len()),
    ))
}

pub(crate) fn decode_playlist(row: &[u8]) -> Result<PlaylistNode, DeviceLibraryError> {
    let fields = FieldReader::new(row, TableKind::PlaylistTree);
    Ok(PlaylistNode {
        id: PlaylistId::new(fields.u32(0x0c)?).ok_or_else(|| fields.malformed("its id is 0"))?,
        parent: PlaylistId::new(fields.u32(0x00)?),
        sort_order: fields.u32(0x08)?,
        is_folder: fields.u32(0x10)? != 0,
        name: read_string(row, PLAYLIST_NAME_POSITION)?,
    })
}

pub(crate) fn encode_playlist_entry(entry: &PlaylistEntry) -> EncodedRow {
    let bytes = [entry.entry_index, entry.track.get(), entry.playlist.get()]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    EncodedRow::new(bytes, 12)
}

pub(crate) fn decode_playlist_entry(row: &[u8]) -> Result<PlaylistEntry, DeviceLibraryError> {
    let fields = FieldReader::new(row, TableKind::PlaylistEntries);
    Ok(PlaylistEntry {
        entry_index: fields.u32(0)?,
        track: TrackId::new(fields.u32(4)?).ok_or_else(|| fields.malformed("its track is 0"))?,
        playlist: PlaylistId::new(fields.u32(8)?)
            .ok_or_else(|| fields.malformed("its playlist is 0"))?,
    })
}

pub(crate) fn encode_artwork(artwork: &Artwork) -> Result<EncodedRow, DeviceLibraryError> {
    encode_id_and_text(artwork.id.get(), &artwork.path)
}

pub(crate) fn decode_artwork(row: &[u8]) -> Result<Artwork, DeviceLibraryError> {
    let (id, path) = decode_id_and_text(row, TableKind::Artwork)?;
    Ok(Artwork {
        id: ArtworkId::new(id)
            .ok_or_else(|| FieldReader::new(row, TableKind::Artwork).malformed("its id is 0"))?,
        path,
    })
}

struct FieldWriter<'bytes>(&'bytes mut Vec<u8>);

impl FieldWriter<'_> {
    fn u8(&mut self, position: usize, value: u8) {
        self.0[position] = value;
    }

    fn u16(&mut self, position: usize, value: u16) {
        self.0[position..position + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn u32(&mut self, position: usize, value: u32) {
        self.0[position..position + 4].copy_from_slice(&value.to_le_bytes());
    }
}

struct FieldReader<'row> {
    row: &'row [u8],
    table: TableKind,
}

impl<'row> FieldReader<'row> {
    fn new(row: &'row [u8], table: TableKind) -> Self {
        Self { row, table }
    }

    fn malformed(&self, reason: &str) -> DeviceLibraryError {
        DeviceLibraryError::MalformedRow {
            table: self.table,
            reason: reason.to_owned(),
        }
    }

    fn short(&self, position: usize) -> DeviceLibraryError {
        self.malformed(&format!(
            "it is {} bytes, too short for its field at byte {position:#x}",
            self.row.len()
        ))
    }

    fn u8(&self, position: usize) -> Result<u8, DeviceLibraryError> {
        self.row
            .get(position)
            .copied()
            .ok_or_else(|| self.short(position))
    }

    fn u16(&self, position: usize) -> Result<u16, DeviceLibraryError> {
        get_u16(self.row, position).ok_or_else(|| self.short(position))
    }

    fn u32(&self, position: usize) -> Result<u32, DeviceLibraryError> {
        get_u32(self.row, position).ok_or_else(|| self.short(position))
    }
}
