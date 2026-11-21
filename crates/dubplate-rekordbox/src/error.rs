use std::path::PathBuf;

use crate::analysis::AnalysisError;

/// Why a rekordbox library could not be read.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The library file does not exist.
    #[error("no rekordbox library at {0}")]
    NotFound(PathBuf),
    /// The library file or its copy could not be read or written.
    #[error("could not copy the rekordbox library from {path}")]
    Copy {
        /// The file that failed.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
    /// The key did not unlock the database.
    #[error("the key does not unlock this rekordbox library")]
    WrongKey,
    /// A query on the unlocked database failed.
    #[error("could not read the rekordbox library")]
    Database(#[from] rusqlite::Error),
    /// rekordbox has not analysed the track, so it has no analysis files.
    #[error("rekordbox has no analysis for track {0}")]
    NoAnalysis(String),
    /// The track's analysis path would lead outside rekordbox's share directory.
    #[error("the analysis path {0} does not stay inside rekordbox's share directory")]
    AnalysisPath(String),
    /// An analysis file could not be opened or read.
    #[error("could not read the analysis file {path}")]
    AnalysisRead {
        /// The file that failed.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
    /// An analysis file's bytes do not match the format.
    #[error("the analysis file {path} is not in the expected format")]
    Analysis {
        /// The file that failed.
        path: PathBuf,
        /// What is wrong with it.
        #[source]
        source: AnalysisError,
    },
}
