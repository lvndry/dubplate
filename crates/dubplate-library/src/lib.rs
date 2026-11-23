//! Dubplate's own music library: the folders a DJ chooses, scanned for audio files and their
//! tags.
//!
//! - [`scan_folder`] walks a folder and its subfolders, skipping hidden and non-audio files, and
//!   reads each track's title, artist, album, BPM, key and duration. A previous [`FolderScan`]
//!   lets it skip every file that has not changed since.
//! - [`LibraryCache`] keeps the scans on disk so the library opens at once on the next launch;
//!   a rescan then picks up the files added or removed meanwhile.
//! - Nothing here writes to the music folders, and nothing Dubplate writes for itself goes
//!   inside rekordbox's folders: [`is_inside_rekordbox`] is the one check for both crates.
//! - [`find_relocations`] finds, for a rekordbox track whose file was moved, the file again by
//!   its name, confirmed by size or length; [`search_folders_for`] and [`candidates_from_paths`]
//!   gather the candidates from a folder the DJ points at.
//! - [`DataFolder`] says where Dubplate keeps its own files, and [`LibrarySettings`] reads what
//!   the library is made of from the settings, relinks included.
//! - [`read_traktor_collection`] reads a collection file written by Traktor Pro 3 and 4, in
//!   the NML format, with its tracks, grids, cues and playlists, read-only.

mod cache;
mod own_files;
mod relocate;
mod scan;
mod settings;
mod traktor;

pub use cache::{CacheError, LibraryCache};
pub use own_files::{is_inside_rekordbox, unix_millis};
pub use relocate::{
    MissingTrack, Relocation, RelocationConfidence, candidates_from_paths, find_relocations,
    search_folders_for,
};
pub use scan::{
    AUDIO_EXTENSIONS, FolderScan, LibraryTrack, ScanError, ScanProblem, is_audio_file,
    read_audio_metadata, scan_folder,
};
pub use settings::{DataFolder, LibrarySettings, LibrarySettingsError, Relink};
pub use traktor::{
    TraktorCollection, TraktorCue, TraktorError, TraktorPlaylistNode, TraktorTrack,
    read_traktor_collection,
};
