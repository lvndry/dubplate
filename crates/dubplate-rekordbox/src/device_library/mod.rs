//! Reads and writes rekordbox's Device Library: the `export.pdb` and `exportExt.pdb` files a
//! rekordbox USB export keeps in `PIONEER/rekordbox/`, which CDJ and XDJ players read to browse
//! the stick.
//!
//! Both files are DeviceSQL databases: little-endian, cut into 4096-byte pages, each table a
//! chain of pages whose rows sit in a heap that grows up from the page header while a directory
//! of row offsets grows down from the page end. The layout follows Deep Symmetry's crate-digger
//! analysis (`rekordbox_pdb.ksy`, <https://djl-analysis.deepsymmetry.org>), the byte-level notes
//! of the `rekordbox-pdb` project (`FORMAT.md`), and the exports rekordbox 6 and 7 write, which
//! the writer reproduces page for page: every table starts with an index page, data follows on
//! the next page, and the tables rekordbox fills on a fresh stick (colours, columns, the
//! unnamed tables 17 and 18, and the history sync table) carry rekordbox's own rows.
//!
//! [`DeviceLibrary`] is the library as Dubplate models it. [`DeviceLibrary::to_export_pdb`]
//! writes it and [`DeviceLibrary::parse_export_pdb`] reads one back, from Dubplate or from
//! rekordbox. [`export_ext_pdb`] writes the companion `exportExt.pdb` with rekordbox's default
//! My Tag categories. [`analysis_path`] gives the folder rekordbox and the players use for a
//! track's analysis files.

mod analysis_path;
mod database;
mod error;
mod export_ext;
mod fixed;
mod ids;
mod library;
mod rows;
mod string;

pub use analysis_path::{AnalysisPath, analysis_path};
pub use database::{DatabaseFile, PageIndex, PageSummary, PageType, TableSummary};
pub use error::DeviceLibraryError;
pub use export_ext::{MyTag, export_ext_pdb, parse_export_ext_tags};
pub use ids::{
    AlbumId, ArtistId, ArtworkId, GenreId, KeyId, LabelId, LibraryId, PlaylistId, TrackId,
};
pub use library::{
    Album, Artwork, DeviceColor, DeviceLibrary, DeviceTrack, FileType, Named, PlaylistEntry,
    PlaylistNode, PreservedRows,
};
pub use string::DeviceString;

/// The tables of `export.pdb`, by the type number rekordbox gives each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TableKind {
    /// Tracks, with their tags, tempo, length and file paths.
    Tracks,
    /// Genre names.
    Genres,
    /// Artist names.
    Artists,
    /// Album names.
    Albums,
    /// Record label names.
    Labels,
    /// Musical key names.
    Keys,
    /// The eight track colours.
    Colors,
    /// The playlist tree: folders and playlists.
    PlaylistTree,
    /// The tracks of each playlist, in order.
    PlaylistEntries,
    /// Table 9, whose rows nobody has decoded.
    Unknown9,
    /// Table 10, whose rows nobody has decoded.
    Unknown10,
    /// The history playlists players record.
    HistoryPlaylists,
    /// The tracks of each history playlist.
    HistoryEntries,
    /// Paths of artwork images.
    Artwork,
    /// Table 14, whose rows nobody has decoded.
    Unknown14,
    /// Table 15, whose rows nobody has decoded.
    Unknown15,
    /// The names of the browse columns players show.
    Columns,
    /// Table 17, which rekordbox fills with the same rows on every stick.
    Unknown17,
    /// Table 18, which rekordbox fills with the same rows on every stick.
    Unknown18,
    /// What rekordbox needs to sync the players' histories back.
    HistorySync,
}

impl TableKind {
    /// Every table, in the order of their type numbers, as the file header lists them.
    pub const ALL: [Self; 20] = [
        Self::Tracks,
        Self::Genres,
        Self::Artists,
        Self::Albums,
        Self::Labels,
        Self::Keys,
        Self::Colors,
        Self::PlaylistTree,
        Self::PlaylistEntries,
        Self::Unknown9,
        Self::Unknown10,
        Self::HistoryPlaylists,
        Self::HistoryEntries,
        Self::Artwork,
        Self::Unknown14,
        Self::Unknown15,
        Self::Columns,
        Self::Unknown17,
        Self::Unknown18,
        Self::HistorySync,
    ];

    /// The type number rekordbox writes for this table.
    pub fn type_number(self) -> u32 {
        match self {
            Self::Tracks => 0,
            Self::Genres => 1,
            Self::Artists => 2,
            Self::Albums => 3,
            Self::Labels => 4,
            Self::Keys => 5,
            Self::Colors => 6,
            Self::PlaylistTree => 7,
            Self::PlaylistEntries => 8,
            Self::Unknown9 => 9,
            Self::Unknown10 => 10,
            Self::HistoryPlaylists => 11,
            Self::HistoryEntries => 12,
            Self::Artwork => 13,
            Self::Unknown14 => 14,
            Self::Unknown15 => 15,
            Self::Columns => 16,
            Self::Unknown17 => 17,
            Self::Unknown18 => 18,
            Self::HistorySync => 19,
        }
    }

    /// The table with rekordbox's type number `number`.
    pub fn from_type_number(number: u32) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.type_number() == number)
    }
}
