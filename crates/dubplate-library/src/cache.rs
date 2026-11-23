//! The scans kept on disk between launches.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::own_files::is_inside_rekordbox;
use crate::scan::FolderScan;

/// The cache format; a cache of another version is ignored and the folders scanned again.
const VERSION: u32 = 2;

/// Why the cache could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    /// The file could not be read or written.
    #[error("{path}: {source}")]
    Io {
        /// The cache file.
        path: PathBuf,
        /// What went wrong.
        source: std::io::Error,
    },
    /// The path is inside one of rekordbox's folders, where Dubplate never writes.
    #[error("{0} is inside a rekordbox folder; Dubplate's library cache must live elsewhere")]
    InsideRekordbox(PathBuf),
    /// The cache could not be written as JSON.
    #[error("the library cache could not be written: {0}")]
    Serialize(#[from] serde_json::Error),
}

/// The last scan of each music folder.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LibraryCache {
    version: u32,
    /// One scan per folder.
    pub scans: Vec<FolderScan>,
}

impl LibraryCache {
    /// A cache holding `scans`.
    pub fn new(scans: Vec<FolderScan>) -> Self {
        Self {
            version: VERSION,
            scans,
        }
    }

    /// Reads the cache at `path`. A missing, unreadable or outdated cache is empty: the folders
    /// are then scanned again, which is slower but loses nothing.
    pub fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Self>(&bytes).ok())
            .filter(|cache| cache.version == VERSION)
            .unwrap_or_default()
    }

    /// Writes the cache to `path`, through a temporary file so a crash never leaves half of it.
    /// Refuses a path inside one of rekordbox's folders.
    pub fn save(&self, path: &Path) -> Result<(), CacheError> {
        if is_inside_rekordbox(path) {
            return Err(CacheError::InsideRekordbox(path.to_path_buf()));
        }
        let bytes = serde_json::to_vec(self)?;
        let io_error = |source| CacheError::Io {
            path: path.to_path_buf(),
            source,
        };
        if let Some(directory) = path.parent() {
            std::fs::create_dir_all(directory).map_err(io_error)?;
        }
        let temporary = path.with_extension("json.partial");
        std::fs::write(&temporary, bytes).map_err(io_error)?;
        std::fs::rename(&temporary, path).map_err(io_error)
    }

    /// The last scan of `folder`, if there is one.
    pub fn scan(&self, folder: &Path) -> Option<&FolderScan> {
        self.scans.iter().find(|scan| scan.folder == folder)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(folder: &str) -> FolderScan {
        FolderScan {
            folder: PathBuf::from(folder),
            tracks: Vec::new(),
            problems: Vec::new(),
            scanned_at_ms: 42,
        }
    }

    #[test]
    fn a_cache_is_never_written_inside_rekordbox() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("Pioneer/rekordbox/library.json");
        let saved = LibraryCache::new(vec![scan("/Music/House")]).save(&path);
        assert!(matches!(saved, Err(CacheError::InsideRekordbox(_))));
        assert!(!path.exists());
    }

    #[test]
    fn a_cache_survives_a_save_and_a_load() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("nested/library.json");
        let cache = LibraryCache::new(vec![scan("/Music/House"), scan("/Music/Amapiano")]);
        cache.save(&path).unwrap();
        let loaded = LibraryCache::load(&path);
        assert_eq!(loaded, cache);
        assert_eq!(
            loaded
                .scan(Path::new("/Music/Amapiano"))
                .unwrap()
                .scanned_at_ms,
            42
        );
        assert!(loaded.scan(Path::new("/Music/Techno")).is_none());
    }

    #[test]
    fn a_missing_or_broken_cache_is_empty() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("library.json");
        assert!(LibraryCache::load(&path).scans.is_empty());
        std::fs::write(&path, b"{ not json").unwrap();
        assert!(LibraryCache::load(&path).scans.is_empty());
        std::fs::write(&path, br#"{"version": 99, "scans": []}"#).unwrap();
        assert_eq!(LibraryCache::load(&path), LibraryCache::default());
    }
}
