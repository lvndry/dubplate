use std::path::PathBuf;

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
}
