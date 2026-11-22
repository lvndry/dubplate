use std::fmt;
use std::num::NonZeroU32;

use serde::Serialize;

macro_rules! row_id {
    ($(#[$documentation:meta])* $name:ident) => {
        $(#[$documentation])*
        ///
        /// The format writes 0 for "none", so an id is never 0 and `Option` of it stands for a
        /// link that may be missing.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
        #[serde(transparent)]
        pub struct $name(NonZeroU32);

        impl $name {
            /// The id with the number `value`, or `None` for 0, which the format reserves for
            /// "none".
            pub const fn new(value: u32) -> Option<Self> {
                match NonZeroU32::new(value) {
                    Some(value) => Some(Self(value)),
                    None => None,
                }
            }

            /// The number written in the file.
            pub const fn get(self) -> u32 {
                self.0.get()
            }

            /// The id after this one, or `None` past the last.
            pub fn next(self) -> Option<Self> {
                self.0.checked_add(1).map(Self)
            }

            /// The first id rekordbox hands out.
            pub const FIRST: Self = Self(NonZeroU32::MIN);
        }

        impl RowId for $name {
            fn number(self) -> u32 {
                self.get()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, "{}", self.0)
            }
        }
    };
}

/// An id of a row that other rows link to.
pub(crate) trait RowId: Copy {
    /// The number written in the file.
    fn number(self) -> u32;
}

/// The number a link to `id` is written as: the id, or 0 for no link.
pub(crate) fn number_or_zero<Id: RowId>(id: Option<Id>) -> u32 {
    id.map_or(0, RowId::number)
}

row_id!(
    /// A track's row in the tracks table, which players report while they play it.
    TrackId
);
row_id!(
    /// An artist's row in the artists table.
    ArtistId
);
row_id!(
    /// An album's row in the albums table.
    AlbumId
);
row_id!(
    /// A genre's row in the genres table.
    GenreId
);
row_id!(
    /// A record label's row in the labels table.
    LabelId
);
row_id!(
    /// A musical key's row in the keys table.
    KeyId
);
row_id!(
    /// An artwork image's row in the artwork table.
    ArtworkId
);
row_id!(
    /// A folder's or playlist's row in the playlist tree.
    PlaylistId
);

/// The number rekordbox stamps into every track row of one library and, plus one, into the
/// settings row of its `exportExt.pdb`. It differs between libraries and stays the same across
/// exports of one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct LibraryId(pub u32);
