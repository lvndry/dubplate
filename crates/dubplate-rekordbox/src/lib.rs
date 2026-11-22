//! Reads the library of rekordbox 6 and 7.
//!
//! rekordbox keeps its collection in `master.db`, a SQLCipher database. [`Library::open_copy`]
//! copies that file into a temporary directory and opens the copy, so rekordbox's own file is
//! never opened, locked or changed.
//!
//! [`Library::analysis`] reads a track's beat grid, cues, phrases and waveforms from the
//! analysis files rekordbox keeps in its `share` directory. Those files are only read.
//!
//! [`device_library`] writes and reads the Device Library a rekordbox USB export carries for
//! CDJs: `export.pdb` and `exportExt.pdb`.
//!
//! [`xml`] writes a library in rekordbox's XML format, the way to hand rekordbox tracks,
//! grids, cues and playlists without touching its own files.

pub mod analysis;
mod cue_colors;
pub mod device_library;
mod error;
mod library;
mod location;
mod model;
mod track_cues;
pub mod xml;

pub use analysis::TrackAnalysis;
pub use cue_colors::{MemoryCueColor, PaletteColor, Rgb};
pub use error::Error;
pub use library::{DatabaseKey, Library, LibrarySummary};
pub use location::{KEY_ENVIRONMENT_VARIABLE, default_master_db, key_file};
pub use model::{Cue, CueKind, HotCuePad, Playlist, PlaylistKind, Track};
pub use track_cues::{HotCue, MEMORY_CUE_LIMIT, MemoryCue, TrackCues, cue_milliseconds};
