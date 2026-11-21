//! Reads the library of rekordbox 6 and 7.
//!
//! rekordbox keeps its collection in `master.db`, a SQLCipher database. [`Library::open_copy`]
//! copies that file into a temporary directory and opens the copy, so rekordbox's own file is
//! never opened, locked or changed.

mod error;
mod library;
mod model;

pub use error::Error;
pub use library::{DatabaseKey, Library, LibrarySummary};
pub use model::{Cue, CueKind, Playlist, PlaylistKind, Track};
