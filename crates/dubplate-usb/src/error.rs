use std::path::PathBuf;

use dubplate_rekordbox::analysis::AnalysisWriteError;
use dubplate_rekordbox::device_library::DeviceLibraryError;

/// Why an export to a stick could not be made. Nothing on the stick that Dubplate did not write
/// is ever changed, whatever the error.
#[derive(Debug, thiserror::Error)]
pub enum UsbExportError {
    /// The destination does not exist or is not a folder.
    #[error("{0} is not a folder; choose a USB stick or a folder")]
    NotAFolder(PathBuf),
    /// The destination is inside one of rekordbox's folders, which Dubplate never writes.
    #[error("{0} is inside a rekordbox folder; Dubplate never writes there")]
    InsideRekordbox(PathBuf),
    /// The stick holds a library another program wrote, which Dubplate does not merge into.
    #[error(
        "{destination} already holds a rekordbox library ({found}). Dubplate does not add to a \
         library rekordbox wrote: export to an empty stick or folder, or one Dubplate exported to"
    )]
    ForeignLibrary {
        /// The stick or folder.
        destination: PathBuf,
        /// What was found there.
        found: String,
    },
    /// Updating only the classic part of this stick would leave OneLibrary out of sync.
    #[error(
        "{destination} contains a OneLibrary file ({found}). Choose a different stick or folder."
    )]
    OneLibrary {
        /// The stick or folder.
        destination: PathBuf,
        /// The OneLibrary catalog or sidecar that was found.
        found: String,
    },
    /// The library Dubplate wrote to the stick before can no longer be read, so it is left as it
    /// is.
    #[error("the library Dubplate wrote to {path} can no longer be read: {source}")]
    UnreadableLibrary {
        /// The `export.pdb` file.
        path: PathBuf,
        /// Why it cannot be read.
        source: DeviceLibraryError,
    },
    /// Dubplate's record of what it wrote to the stick cannot be read.
    #[error("Dubplate's record of what it wrote, {path}, cannot be read: {reason}")]
    UnreadableManifest {
        /// The record's file.
        path: PathBuf,
        /// Why it cannot be read.
        reason: String,
    },
    /// Nothing to export.
    #[error("there is nothing to export: the playlists hold no track")]
    Empty,
    /// A file or folder could not be read or written.
    #[error("{path}: {source}")]
    Io {
        /// The file or folder.
        path: PathBuf,
        /// What went wrong.
        source: std::io::Error,
    },
    /// The library could not be written.
    #[error("the library could not be written: {0}")]
    DeviceLibrary(#[from] DeviceLibraryError),
    /// A track's analysis files could not be written.
    #[error("the analysis of {path} could not be written: {source}")]
    Analysis {
        /// The track's audio file.
        path: PathBuf,
        /// What went wrong.
        source: AnalysisWriteError,
    },
}

impl UsbExportError {
    pub(crate) fn io(path: impl Into<PathBuf>) -> impl FnOnce(std::io::Error) -> Self {
        let path = path.into();
        move |source| Self::Io { path, source }
    }
}
