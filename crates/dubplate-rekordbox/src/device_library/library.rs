use std::collections::{BTreeMap, HashSet};

use serde::Serialize;

use super::TableKind;
use super::database::{DatabaseFile, EncodedRow, TableRows, align4, write_database};
use super::error::DeviceLibraryError;
use super::fixed;
use super::ids::{
    AlbumId, ArtistId, ArtworkId, GenreId, HistoryId, KeyId, LabelId, LibraryId, PlaylistId,
    TrackId,
};
use super::rows;
use super::string::read_string;

/// A row of an id and a name: an artist, a genre, a label or a musical key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Named<Id> {
    /// The row's id.
    pub id: Id,
    /// The name.
    pub name: String,
}

/// An album.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Album {
    /// The album's id.
    pub id: AlbumId,
    /// Its artist. rekordbox leaves it out of almost every album row it writes.
    pub artist: Option<ArtistId>,
    /// The album's name.
    pub name: String,
}

/// An artwork image on the stick.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Artwork {
    /// The image's id.
    pub id: ArtworkId,
    /// Where the image is on the stick, such as `/PIONEER/Artwork/00001/a1.jpg`.
    pub path: String,
}

/// A folder or a playlist in the playlist tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlaylistNode {
    /// The node's id.
    pub id: PlaylistId,
    /// The folder it sits in, or `None` at the top.
    pub parent: Option<PlaylistId>,
    /// Its place among the nodes of its folder, from 0.
    pub sort_order: u32,
    /// Whether it is a folder of playlists rather than a playlist.
    pub is_folder: bool,
    /// The name players show.
    pub name: String,
}

/// A track's place in a playlist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PlaylistEntry {
    /// The position in the playlist, from 1.
    pub entry_index: u32,
    /// The track.
    pub track: TrackId,
    /// The playlist.
    pub playlist: PlaylistId,
}

/// A track's place in a history players recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct HistoryEntry {
    /// The track played.
    pub track: TrackId,
    /// The history.
    pub history: HistoryId,
    /// The position in the history, from 1 for the first track played.
    pub entry_index: u32,
}

/// A history players recorded on the stick: the tracks played while it was in a player, in the
/// order they played. The format keeps no time or date for it, only its name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeviceHistory {
    /// The history's id.
    pub id: HistoryId,
    /// The name players gave it, such as `HISTORY 001`.
    pub name: String,
    /// The tracks in the order they played. A track the library does not hold is kept, so the
    /// reader can say what is missing.
    pub tracks: Vec<TrackId>,
}

/// The colour a DJ gives a track, from rekordbox's eight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum DeviceColor {
    /// Pink.
    Pink,
    /// Red.
    Red,
    /// Orange.
    Orange,
    /// Yellow.
    Yellow,
    /// Green.
    Green,
    /// Aqua.
    Aqua,
    /// Blue.
    Blue,
    /// Purple.
    Purple,
}

impl DeviceColor {
    /// Every colour, in the order of their ids.
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

    /// The id of the colour's row, from 1 for pink to 8 for purple.
    pub fn id(self) -> u8 {
        match self {
            Self::Pink => 1,
            Self::Red => 2,
            Self::Orange => 3,
            Self::Yellow => 4,
            Self::Green => 5,
            Self::Aqua => 6,
            Self::Blue => 7,
            Self::Purple => 8,
        }
    }

    /// The colour whose row has id `id`.
    pub fn from_id(id: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|color| color.id() == id)
    }

    /// The name rekordbox gives the colour's row.
    pub fn name(self) -> &'static str {
        match self {
            Self::Pink => "Pink",
            Self::Red => "Red",
            Self::Orange => "Orange",
            Self::Yellow => "Yellow",
            Self::Green => "Green",
            Self::Aqua => "Aqua",
            Self::Blue => "Blue",
            Self::Purple => "Purple",
        }
    }
}

/// The kind of audio file, as the track row codes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum FileType {
    /// MP3.
    Mp3,
    /// AAC or ALAC in an M4A file.
    M4a,
    /// FLAC.
    Flac,
    /// WAV.
    Wav,
    /// AIFF.
    Aiff,
    /// A code this version of Dubplate does not know.
    Other(u16),
}

impl FileType {
    /// The kind of file whose extension is `extension`, in any case.
    pub fn from_extension(extension: &str) -> Self {
        match extension.to_ascii_lowercase().as_str() {
            "mp3" => Self::Mp3,
            "m4a" | "mp4" | "aac" | "alac" => Self::M4a,
            "flac" => Self::Flac,
            "wav" | "wave" => Self::Wav,
            "aif" | "aiff" => Self::Aiff,
            _ => Self::Other(0),
        }
    }

    /// The code the track row holds.
    pub fn code(self) -> u16 {
        match self {
            Self::Mp3 => 0x01,
            Self::M4a => 0x04,
            Self::Flac => 0x05,
            Self::Wav => 0x0b,
            Self::Aiff => 0x0c,
            Self::Other(code) => code,
        }
    }

    /// The kind of file the code `code` stands for.
    pub fn from_code(code: u16) -> Self {
        [Self::Mp3, Self::M4a, Self::Flac, Self::Wav, Self::Aiff]
            .into_iter()
            .find(|kind| kind.code() == code)
            .unwrap_or(Self::Other(code))
    }
}

/// A track as players see it: its tags, tempo and length, its links to the other tables, and
/// where its audio and analysis files are on the stick.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeviceTrack {
    /// The track's id.
    pub id: TrackId,
    /// The title.
    pub title: String,
    /// The artist.
    pub artist: Option<ArtistId>,
    /// The composer.
    pub composer: Option<ArtistId>,
    /// The original artist of a cover.
    pub original_artist: Option<ArtistId>,
    /// The remixer.
    pub remixer: Option<ArtistId>,
    /// The album.
    pub album: Option<AlbumId>,
    /// The genre.
    pub genre: Option<GenreId>,
    /// The record label.
    pub label: Option<LabelId>,
    /// The musical key.
    pub key: Option<KeyId>,
    /// The artwork.
    pub artwork: Option<ArtworkId>,
    /// The DJ's colour for the track.
    pub color: Option<DeviceColor>,
    /// The rating, from 0 to 5 stars.
    pub rating: u8,
    /// The tempo at the start of the track, in hundredths of a BPM.
    pub tempo_hundredths: u32,
    /// The length in whole seconds.
    pub duration_seconds: u16,
    /// The sample rate in hertz.
    pub sample_rate: u32,
    /// The bits per sample.
    pub sample_depth: u16,
    /// The bit rate in kilobits a second.
    pub bitrate: u32,
    /// The audio file's size in bytes.
    pub file_size: u32,
    /// The kind of audio file.
    pub file_type: FileType,
    /// The track's number on its album.
    pub track_number: u32,
    /// The disc number on a multi-disc album.
    pub disc_number: u16,
    /// The year of release.
    pub year: u16,
    /// How many times it was played.
    pub play_count: u16,
    /// A number rekordbox writes per track, whose meaning is unknown; any value reads.
    pub checksum: u32,
    /// The audio file on the stick, such as `/Contents/Artist/Album/Track.mp3`.
    pub file_path: String,
    /// The analysis file on the stick, such as `/PIONEER/USBANLZ/P016/0000875E/ANLZ0000.DAT`.
    pub analyze_path: String,
    /// When the track was analysed, as `yyyy-mm-dd`.
    pub analyze_date: String,
    /// When the track joined the library, as `yyyy-mm-dd`.
    pub date_added: String,
    /// When the track was released, as `yyyy-mm-dd`, or empty.
    pub release_date: String,
    /// The DJ's comment.
    pub comment: String,
    /// The mix name.
    pub mix_name: String,
    /// The ISRC.
    pub isrc: String,
    /// The lyricist.
    pub lyricist: String,
    /// rekordbox's message field.
    pub message: String,
    /// Whether the track's details may be shown on KUVO.
    pub kuvo_public: bool,
    /// Whether players load the track's hot cues with it when set to.
    pub autoload_hot_cues: bool,
}

impl DeviceTrack {
    /// The file name, the last part of [`Self::file_path`].
    pub fn file_name(&self) -> &str {
        self.file_path.rsplit('/').next().unwrap_or(&self.file_path)
    }
}

/// Rows of tables Dubplate does not model, kept as read so a library read from a stick can be
/// written back with them: players record history there.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PreservedRows {
    rows: BTreeMap<u32, Vec<Vec<u8>>>,
}

impl PreservedRows {
    /// The tables whose rows are kept as read.
    pub const TABLES: [TableKind; 7] = [
        TableKind::Unknown9,
        TableKind::Unknown10,
        TableKind::HistoryPlaylists,
        TableKind::HistoryEntries,
        TableKind::Unknown14,
        TableKind::Unknown15,
        TableKind::HistorySync,
    ];

    /// The rows kept for `table`.
    pub fn rows(&self, table: TableKind) -> &[Vec<u8>] {
        self.rows
            .get(&table.type_number())
            .map_or(&[], Vec::as_slice)
    }

    /// Whether any table other than the history sync table has rows, which happens once
    /// players record a history on the stick.
    pub fn has_history(&self) -> bool {
        Self::TABLES
            .into_iter()
            .filter(|table| *table != TableKind::HistorySync)
            .any(|table| !self.rows(table).is_empty())
    }

    fn insert(&mut self, table: TableKind, rows: Vec<Vec<u8>>) {
        if !rows.is_empty() {
            self.rows.insert(table.type_number(), rows);
        }
    }

    fn push(&mut self, table: TableKind, row: Vec<u8>) {
        self.rows.entry(table.type_number()).or_default().push(row);
    }
}

/// A Device Library: what `export.pdb` holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeviceLibrary {
    /// The number stamped into every track row.
    pub library_id: LibraryId,
    /// When the library was first written, as `yyyy-mm-dd`, for the history sync row of a stick
    /// players have not written to.
    pub created: String,
    /// The tracks.
    pub tracks: Vec<DeviceTrack>,
    /// The artists.
    pub artists: Vec<Named<ArtistId>>,
    /// The albums.
    pub albums: Vec<Album>,
    /// The genres.
    pub genres: Vec<Named<GenreId>>,
    /// The record labels.
    pub labels: Vec<Named<LabelId>>,
    /// The musical keys.
    pub keys: Vec<Named<KeyId>>,
    /// The artwork images.
    pub artwork: Vec<Artwork>,
    /// The folders and playlists.
    pub playlists: Vec<PlaylistNode>,
    /// The tracks of every playlist.
    pub playlist_entries: Vec<PlaylistEntry>,
    /// Rows of the tables Dubplate keeps as read.
    pub preserved: PreservedRows,
}

impl DeviceLibrary {
    /// An empty library with `library_id`, first written on `created` (`yyyy-mm-dd`).
    pub fn new(library_id: LibraryId, created: impl Into<String>) -> Self {
        Self {
            library_id,
            created: created.into(),
            tracks: Vec::new(),
            artists: Vec::new(),
            albums: Vec::new(),
            genres: Vec::new(),
            labels: Vec::new(),
            keys: Vec::new(),
            artwork: Vec::new(),
            playlists: Vec::new(),
            playlist_entries: Vec::new(),
            preserved: PreservedRows::default(),
        }
    }

    /// The histories players recorded, oldest first by id, each with its tracks in the order
    /// they played.
    ///
    /// Fails when a history row or entry does not match the layout players write.
    pub fn histories(&self) -> Result<Vec<DeviceHistory>, DeviceLibraryError> {
        let mut histories = BTreeMap::new();
        for row in self.preserved.rows(TableKind::HistoryPlaylists) {
            let (id, name) = rows::decode_history_playlist(row)?;
            histories.insert(id, (name, Vec::new()));
        }
        for row in self.preserved.rows(TableKind::HistoryEntries) {
            let entry = rows::decode_history_entry(row)?;
            let Some((_, entries)) = histories.get_mut(&entry.history) else {
                return Err(DeviceLibraryError::DanglingReference(format!(
                    "a history entry belongs to history {}, which the library does not hold",
                    entry.history
                )));
            };
            entries.push((entry.entry_index, entry.track));
        }
        Ok(histories
            .into_iter()
            .map(|(id, (name, mut entries))| {
                entries.sort_by_key(|(entry_index, _)| *entry_index);
                DeviceHistory {
                    id,
                    name,
                    tracks: entries.into_iter().map(|(_, track)| track).collect(),
                }
            })
            .collect())
    }

    /// Records a history named `name` with `tracks` in playing order, as a player does, and
    /// returns its id. Dubplate never records histories on a DJ's stick; this builds sticks
    /// that look played on, for tests and demonstrations.
    pub fn record_history(
        &mut self,
        name: &str,
        tracks: &[TrackId],
    ) -> Result<HistoryId, DeviceLibraryError> {
        let taken = self.preserved.rows(TableKind::HistoryPlaylists).len();
        let next = taken.saturating_add(1);
        let id = u32::try_from(next)
            .ok()
            .and_then(HistoryId::new)
            .ok_or(DeviceLibraryError::ExhaustedIds(taken))?;
        let row = rows::encode_history_playlist(id, name)?;
        self.preserved.push(TableKind::HistoryPlaylists, row.bytes);
        for (entry_index, track) in (1..).zip(tracks) {
            let entry = HistoryEntry {
                track: *track,
                history: id,
                entry_index,
            };
            self.preserved.push(
                TableKind::HistoryEntries,
                rows::encode_history_entry(&entry).bytes,
            );
        }
        Ok(id)
    }

    /// The library as the bytes of `export.pdb`.
    ///
    /// Fails when a row refers to an id the library does not hold, when two rows of a table
    /// share an id, or when a row is too big for a page.
    pub fn to_export_pdb(&self) -> Result<Vec<u8>, DeviceLibraryError> {
        self.check_references()?;
        let mut tables = Vec::with_capacity(TableKind::ALL.len());
        for kind in TableKind::ALL {
            tables.push(TableRows {
                type_number: kind.type_number(),
                rows: self.rows_of(kind)?,
            });
        }
        write_database(&tables)
    }

    fn rows_of(&self, kind: TableKind) -> Result<Vec<EncodedRow>, DeviceLibraryError> {
        let rows = match kind {
            TableKind::Tracks => sorted(&self.tracks, |track| track.id)
                .map(|track| rows::encode_track(track, self.library_id))
                .collect::<Result<_, _>>()?,
            TableKind::Genres => sorted(&self.genres, |genre| genre.id)
                .map(|genre| rows::encode_id_and_text(genre.id.get(), &genre.name))
                .collect::<Result<_, _>>()?,
            TableKind::Artists => sorted(&self.artists, |artist| artist.id)
                .map(rows::encode_artist)
                .collect::<Result<_, _>>()?,
            TableKind::Albums => sorted(&self.albums, |album| album.id)
                .map(rows::encode_album)
                .collect::<Result<_, _>>()?,
            TableKind::Labels => sorted(&self.labels, |label| label.id)
                .map(|label| rows::encode_id_and_text(label.id.get(), &label.name))
                .collect::<Result<_, _>>()?,
            TableKind::Keys => sorted(&self.keys, |key| key.id)
                .map(rows::encode_key)
                .collect::<Result<_, _>>()?,
            TableKind::Colors => fixed::color_rows()?,
            TableKind::PlaylistTree => sorted(&self.playlists, |node| node.id)
                .map(rows::encode_playlist)
                .collect::<Result<_, _>>()?,
            TableKind::PlaylistEntries => {
                let mut entries: Vec<&PlaylistEntry> = self.playlist_entries.iter().collect();
                entries.sort_by_key(|entry| (entry.playlist, entry.entry_index));
                entries
                    .into_iter()
                    .map(rows::encode_playlist_entry)
                    .collect()
            }
            TableKind::Artwork => sorted(&self.artwork, |artwork| artwork.id)
                .map(rows::encode_artwork)
                .collect::<Result<_, _>>()?,
            TableKind::Columns => fixed::column_rows()?,
            TableKind::Unknown17 => fixed::table_17_rows(),
            TableKind::Unknown18 => fixed::table_18_rows(),
            TableKind::HistorySync if self.preserved.rows(kind).is_empty() => {
                vec![fixed::history_sync_row(&self.created)?]
            }
            TableKind::HistorySync => preserved_rows(self.preserved.rows(kind), true),
            TableKind::Unknown9
            | TableKind::Unknown10
            | TableKind::HistoryPlaylists
            | TableKind::HistoryEntries
            | TableKind::Unknown14
            | TableKind::Unknown15 => preserved_rows(self.preserved.rows(kind), false),
        };
        Ok(rows)
    }

    fn check_references(&self) -> Result<(), DeviceLibraryError> {
        let artists = unique_ids("artist", self.artists.iter().map(|artist| artist.id))?;
        let albums = unique_ids("album", self.albums.iter().map(|album| album.id))?;
        let genres = unique_ids("genre", self.genres.iter().map(|genre| genre.id))?;
        let labels = unique_ids("label", self.labels.iter().map(|label| label.id))?;
        let keys = unique_ids("key", self.keys.iter().map(|key| key.id))?;
        let artwork = unique_ids("artwork", self.artwork.iter().map(|artwork| artwork.id))?;
        let tracks = unique_ids("track", self.tracks.iter().map(|track| track.id))?;
        let playlists = unique_ids("playlist", self.playlists.iter().map(|node| node.id))?;
        let folders: HashSet<PlaylistId> = self
            .playlists
            .iter()
            .filter(|node| node.is_folder)
            .map(|node| node.id)
            .collect();

        for album in &self.albums {
            refers("album", album.id, "artist", album.artist, &artists)?;
        }
        for track in &self.tracks {
            for artist in [
                track.artist,
                track.composer,
                track.original_artist,
                track.remixer,
            ] {
                refers("track", track.id, "artist", artist, &artists)?;
            }
            refers("track", track.id, "album", track.album, &albums)?;
            refers("track", track.id, "genre", track.genre, &genres)?;
            refers("track", track.id, "label", track.label, &labels)?;
            refers("track", track.id, "key", track.key, &keys)?;
            refers("track", track.id, "artwork", track.artwork, &artwork)?;
        }
        for node in &self.playlists {
            refers("playlist", node.id, "folder", node.parent, &folders)?;
        }
        for entry in &self.playlist_entries {
            refers("entry", entry.playlist, "track", Some(entry.track), &tracks)?;
            if !playlists.contains(&entry.playlist) || folders.contains(&entry.playlist) {
                return Err(DeviceLibraryError::DanglingReference(format!(
                    "a playlist entry belongs to {}, which is not a playlist of the library",
                    entry.playlist
                )));
            }
        }
        Ok(())
    }

    /// Reads a library from the bytes of an `export.pdb`, written by Dubplate or by rekordbox.
    ///
    /// The tables Dubplate models are decoded; the history tables and the tables nobody has
    /// decoded are kept as read in [`Self::preserved`]; the colour, column and other tables
    /// rekordbox fills the same way on every stick are left out. A library read from a file
    /// with no track says its id is 0.
    pub fn parse_export_pdb(bytes: &[u8]) -> Result<Self, DeviceLibraryError> {
        let database = DatabaseFile::parse(bytes)?;
        let rows_of = |kind: TableKind| {
            database
                .table(kind.type_number())
                .map_or(&[][..], |table| table.rows.as_slice())
        };
        let mut library_id = LibraryId(0);
        let mut tracks = Vec::new();
        for row in rows_of(TableKind::Tracks) {
            let (track, id) = rows::decode_track(row)?;
            if tracks.is_empty() {
                library_id = id;
            } else if id != library_id {
                return Err(DeviceLibraryError::DanglingReference(format!(
                    "the track rows carry two library ids, {library_id:?} and {id:?}"
                )));
            }
            tracks.push(track);
        }
        let mut preserved = PreservedRows::default();
        for kind in PreservedRows::TABLES {
            preserved.insert(kind, rows_of(kind).to_vec());
        }
        let created = rows_of(TableKind::HistorySync)
            .first()
            .map(|row| read_string(row, fixed::HISTORY_SYNC_DATE_POSITION))
            .transpose()?
            .unwrap_or_default();
        Ok(Self {
            library_id,
            created,
            tracks,
            artists: decode_all(rows_of(TableKind::Artists), rows::decode_artist)?,
            albums: decode_all(rows_of(TableKind::Albums), rows::decode_album)?,
            genres: decode_named(rows_of(TableKind::Genres), TableKind::Genres, GenreId::new)?,
            labels: decode_named(rows_of(TableKind::Labels), TableKind::Labels, LabelId::new)?,
            keys: decode_all(rows_of(TableKind::Keys), rows::decode_key)?,
            artwork: decode_all(rows_of(TableKind::Artwork), rows::decode_artwork)?,
            playlists: decode_all(rows_of(TableKind::PlaylistTree), rows::decode_playlist)?,
            playlist_entries: decode_all(
                rows_of(TableKind::PlaylistEntries),
                rows::decode_playlist_entry,
            )?,
            preserved,
        })
    }
}

fn sorted<Item, Key: Ord>(
    items: &[Item],
    key: impl Fn(&Item) -> Key,
) -> impl Iterator<Item = &Item> {
    let mut ordered: Vec<&Item> = items.iter().collect();
    ordered.sort_by_key(|item| key(item));
    ordered.into_iter()
}

fn preserved_rows(rows: &[Vec<u8>], has_index_shift: bool) -> Vec<EncodedRow> {
    rows.iter()
        .map(|bytes| {
            let allocation = align4(bytes.len());
            if has_index_shift {
                EncodedRow::with_index_shift(bytes.clone(), allocation)
            } else {
                EncodedRow::new(bytes.clone(), allocation)
            }
        })
        .collect()
}

fn decode_all<Item>(
    rows: &[Vec<u8>],
    decode: fn(&[u8]) -> Result<Item, DeviceLibraryError>,
) -> Result<Vec<Item>, DeviceLibraryError> {
    rows.iter().map(|row| decode(row)).collect()
}

fn decode_named<Id>(
    rows: &[Vec<u8>],
    kind: TableKind,
    make: fn(u32) -> Option<Id>,
) -> Result<Vec<Named<Id>>, DeviceLibraryError> {
    rows.iter()
        .map(|row| {
            let (id, name) = rows::decode_id_and_text(row, kind)?;
            let id = make(id).ok_or_else(|| DeviceLibraryError::MalformedRow {
                table: kind,
                reason: "its id is 0".to_owned(),
            })?;
            Ok(Named { id, name })
        })
        .collect()
}

fn unique_ids<Id: std::hash::Hash + Eq + std::fmt::Display + Copy>(
    what: &str,
    ids: impl Iterator<Item = Id>,
) -> Result<HashSet<Id>, DeviceLibraryError> {
    let mut seen = HashSet::new();
    for id in ids {
        if !seen.insert(id) {
            return Err(DeviceLibraryError::DanglingReference(format!(
                "two {what} rows share the id {id}"
            )));
        }
    }
    Ok(seen)
}

fn refers<Owner: std::fmt::Display, Id: std::hash::Hash + Eq + std::fmt::Display>(
    owner_kind: &str,
    owner: Owner,
    target_kind: &str,
    target: Option<Id>,
    known: &HashSet<Id>,
) -> Result<(), DeviceLibraryError> {
    match target {
        Some(id) if !known.contains(&id) => Err(DeviceLibraryError::DanglingReference(format!(
            "{owner_kind} {owner} refers to {target_kind} {id}, which the library does not hold"
        ))),
        _ => Ok(()),
    }
}
