//! Dubplate's own music library: the folders a DJ chooses, scanned for audio files and their
//! tags.
//!
//! - [`scan_folder`] walks a folder and its subfolders, skipping hidden and non-audio files, and
//!   reads each track's title, artist, album, BPM, key and duration. A previous [`FolderScan`]
//!   lets it skip every file that has not changed since.
//! - [`LibraryCache`] keeps the scans on disk so the library opens at once on the next launch;
//!   a rescan then picks up the files added or removed meanwhile.
//! - Nothing here writes to the music folders.

mod cache;
mod scan;

pub use cache::{CacheError, LibraryCache};
pub use scan::{
    AUDIO_EXTENSIONS, FolderScan, LibraryTrack, ScanError, ScanProblem, is_audio_file, scan_folder,
};
