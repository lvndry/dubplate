//! Exports sets and playlists to a USB stick, or a folder, that Pioneer CDJs and XDJs read as a
//! rekordbox export: the audio under `Contents`, each track's analysis files (beat grid, hot
//! cues, memory cues, loops, waveforms and phrases) under `PIONEER/USBANLZ`, and the Device
//! Library (`export.pdb` and `exportExt.pdb`) in `PIONEER/rekordbox`.
//!
//! - [`export_to_usb`] writes the export. Every file is written beside its place and moved there
//!   once flushed, audio first and the library last, so the library on a stick only points at
//!   files that are there.
//! - A stick with no library, or one Dubplate wrote, is written to; Dubplate adds to its own
//!   library and keeps what players recorded there. A stick with a library rekordbox or another
//!   program wrote is refused. Dubplate keeps a record of the files it wrote and never replaces
//!   or removes any other file.
//! - Nothing is ever written inside rekordbox's own folders.
//! - [`playlists_from_set`] turns a set prepared in Dubplate into the playlists to export, with
//!   each track's grid as the DJ fixed it and the cues the DJ kept.
//!
//! rekordbox 7 also writes a OneLibrary database (`exportLibrary.db`) for the newest players;
//! Dubplate writes the Device Library only, which the CDJ-2000NXS2, the CDJ-3000 and XDJs read.

mod analysis;
mod contents;
mod error;
mod export;
mod files;
mod from_set;
mod library_builder;
mod request;
mod stick;

pub use contents::{contents_path, unnumbered_path};
/// Whether any folder in a path is one of rekordbox's, `Pioneer` or `rekordbox`, in any case:
/// the same check every file Dubplate writes for itself goes through.
pub use dubplate_library::is_inside_rekordbox as is_inside_rekordbox_folder;
pub use error::UsbExportError;
pub use export::{UsbExportProgress, UsbExportReport, UsbExportStep, export_to_usb, local_today};
pub use files::same_contents;
pub use from_set::{analysis_cues, playlists_from_set};
pub use request::{RekordboxAnalysis, TrackTags, UsbPlaylist, UsbTrack};
