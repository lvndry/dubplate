//! Reads the library of rekordbox 6 and 7.
//!
//! rekordbox keeps its collection in `master.db`, a SQLCipher database. [`Library::open_copy`]
//! copies that file into a temporary directory and opens the copy, so rekordbox's own file is
//! never opened, locked or changed.
//!
//! [`Library::analysis`] reads a track's beat grid, cues, phrases and waveforms from the
//! analysis files rekordbox keeps in its `share` directory. Those files are only read.

pub mod analysis;
mod error;
mod library;
mod model;

pub use analysis::TrackAnalysis;
pub use error::Error;
pub use library::{DatabaseKey, Library, LibrarySummary};
pub use model::{Cue, CueKind, Playlist, PlaylistKind, Track};
