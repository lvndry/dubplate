//! Where the stems and the model's weights are kept.

use std::path::{Path, PathBuf};

use dubplate_library::DataFolder;
use euphoria_core::stems as separation;

/// Where the network's weights are downloaded from the first time: the htdemucs model as its
/// author publishes it on Hugging Face, pinned to one revision, 84 MB.
pub const WEIGHTS_URL: &str = separation::WEIGHTS_URL;

/// The most room the stems take before the ones used longest ago are removed: 20 GB, about
/// 70 tracks of seven minutes, so a whole set and its shortlist fit.
pub const DEFAULT_CACHE_LIMIT: CacheLimit = CacheLimit::from_bytes(20_000_000_000);

/// The most room the stem cache takes on disk, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CacheLimit(u64);

impl CacheLimit {
    /// A limit of `bytes`.
    pub const fn from_bytes(bytes: u64) -> Self {
        Self(bytes)
    }

    /// The limit in bytes.
    pub const fn bytes(self) -> u64 {
        self.0
    }
}

impl Default for CacheLimit {
    fn default() -> Self {
        DEFAULT_CACHE_LIMIT
    }
}

/// Where the stems and the network's weights are kept: Dubplate's own folder, never one of
/// rekordbox's and never next to the music.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StemsLocation {
    cache: PathBuf,
    weights: PathBuf,
    limit: CacheLimit,
}

impl StemsLocation {
    /// The `stems` folder of Dubplate's data folder, with the weights in its `models` folder and
    /// the [`DEFAULT_CACHE_LIMIT`].
    pub fn in_data_folder(folder: &DataFolder) -> Self {
        Self::new(
            folder.stems(),
            folder.models().join(weights_file_name()),
            DEFAULT_CACHE_LIMIT,
        )
    }

    /// Stems kept in `cache` up to `limit`, with the weights at `weights`.
    pub fn new(cache: PathBuf, weights: PathBuf, limit: CacheLimit) -> Self {
        Self {
            cache,
            weights,
            limit,
        }
    }

    /// The folder the stems are kept in, one folder per track.
    pub fn cache_directory(&self) -> &Path {
        &self.cache
    }

    /// The network's weights file, downloaded there the first time.
    pub fn weights_path(&self) -> &Path {
        &self.weights
    }

    /// The most room the stems take.
    pub fn limit(&self) -> CacheLimit {
        self.limit
    }
}

/// The weights file's name: the model's name, as Euphoria names it in its own folder.
fn weights_file_name() -> String {
    format!("{}.safetensors", separation::STEMS_MODEL)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stems_and_weights_live_in_dubplates_own_folder() {
        let location = StemsLocation::in_data_folder(&DataFolder::at(PathBuf::from(
            "/Users/dj/Library/Application Support/dubplate",
        )));
        assert_eq!(
            location.cache_directory(),
            Path::new("/Users/dj/Library/Application Support/dubplate/stems")
        );
        assert_eq!(
            location.weights_path(),
            Path::new(
                "/Users/dj/Library/Application Support/dubplate/models/htdemucs-955717e8.safetensors"
            )
        );
        assert_eq!(location.limit(), DEFAULT_CACHE_LIMIT);
    }
}
