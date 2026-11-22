//! Which tracks have stems in the cache, found without reading the tracks.
//!
//! The cache keys stems by the content of the track, which takes reading the whole file to
//! hash. The index remembers, for each track Dubplate separated, the file's size and
//! modification time and the cache folder its stems went to, so the library can show which
//! tracks have stems at launch without reading a single track. A file changed since is not
//! trusted: asking for its stems again reads it and finds them, or separates it again.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

use crate::files::StemFiles;

const INDEX_FILE_NAME: &str = "index.json";
const INDEX_VERSION: u32 = 1;

/// A file's size and modification time, which change whenever its content does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct FileStamp {
    size: u64,
    modified_nanoseconds: u128,
}

impl FileStamp {
    fn of(path: &Path) -> io::Result<Self> {
        let metadata = std::fs::metadata(path)?;
        let modified_nanoseconds = metadata
            .modified()?
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        Ok(Self {
            size: metadata.len(),
            modified_nanoseconds,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct IndexEntry {
    track: PathBuf,
    stamp: FileStamp,
    entry: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct IndexFile {
    version: u32,
    entries: Vec<IndexEntry>,
}

/// The tracks whose stems Dubplate put in the cache.
#[derive(Debug)]
pub(crate) struct StemsIndex {
    cache: PathBuf,
    entries: HashMap<PathBuf, IndexEntry>,
}

impl StemsIndex {
    /// Reads the index of the cache folder `cache`. An index that is missing, unreadable or
    /// from another version starts empty: tracks are then found in the cache when asked for.
    pub(crate) fn open(cache: &Path) -> Self {
        let entries = std::fs::read(cache.join(INDEX_FILE_NAME))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<IndexFile>(&bytes).ok())
            .filter(|file| file.version == INDEX_VERSION)
            .map(|file| {
                file.entries
                    .into_iter()
                    .map(|entry| (entry.track.clone(), entry))
                    .collect()
            })
            .unwrap_or_default();
        Self {
            cache: cache.to_path_buf(),
            entries,
        }
    }

    /// The stems of `track` when Dubplate separated it, the file has not changed since and its
    /// stems are still in the cache.
    pub(crate) fn lookup(&self, track: &Path) -> Option<StemFiles> {
        let entry = self.entries.get(track)?;
        if FileStamp::of(track).ok()? != entry.stamp {
            return None;
        }
        StemFiles::in_folder(self.cache.join(&entry.entry))
    }

    /// Remembers that the stems of `track` are `files`, and saves the index.
    pub(crate) fn record(&mut self, track: &Path, files: &StemFiles) -> io::Result<()> {
        let entry = files
            .entry_name()
            .ok_or_else(|| io::Error::other("the stems folder has no name"))?
            .to_owned();
        let stamp = FileStamp::of(track)?;
        self.entries.insert(
            track.to_path_buf(),
            IndexEntry {
                track: track.to_path_buf(),
                stamp,
                entry,
            },
        );
        self.save()
    }

    /// Writes the index next to the stems, replacing the old one in one step so a crash never
    /// leaves half of it.
    fn save(&self) -> io::Result<()> {
        std::fs::create_dir_all(&self.cache)?;
        let mut entries: Vec<IndexEntry> = self
            .entries
            .values()
            .filter(|entry| self.cache.join(&entry.entry).is_dir())
            .cloned()
            .collect();
        entries.sort_by(|first, second| first.track.cmp(&second.track));
        let bytes = serde_json::to_vec(&IndexFile {
            version: INDEX_VERSION,
            entries,
        })
        .map_err(io::Error::other)?;
        let path = self.cache.join(INDEX_FILE_NAME);
        let partial = path.with_extension("json.partial");
        std::fs::write(&partial, bytes)?;
        std::fs::rename(&partial, &path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stems_in(cache: &Path, entry: &str) -> StemFiles {
        let directory = cache.join(entry);
        std::fs::create_dir_all(&directory).unwrap();
        for name in ["drums", "bass", "other", "vocals"] {
            std::fs::write(directory.join(format!("{name}.wav")), b"").unwrap();
        }
        StemFiles::in_folder(directory).unwrap()
    }

    #[test]
    fn recorded_stems_are_found_again_after_a_relaunch() {
        let folder = tempfile::tempdir().unwrap();
        let cache = folder.path().join("stems");
        let track = folder.path().join("track.m4a");
        std::fs::write(&track, b"audio").unwrap();
        let files = stems_in(&cache, "htdemucs-955717e8-aa");
        StemsIndex::open(&cache).record(&track, &files).unwrap();
        assert_eq!(StemsIndex::open(&cache).lookup(&track), Some(files));
    }

    #[test]
    fn a_changed_track_or_evicted_stems_are_not_trusted() {
        let folder = tempfile::tempdir().unwrap();
        let cache = folder.path().join("stems");
        let track = folder.path().join("track.m4a");
        std::fs::write(&track, b"audio").unwrap();
        let files = stems_in(&cache, "htdemucs-955717e8-bb");
        let mut index = StemsIndex::open(&cache);
        index.record(&track, &files).unwrap();
        std::fs::write(&track, b"retagged audio").unwrap();
        assert_eq!(index.lookup(&track), None);

        index.record(&track, &files).unwrap();
        std::fs::remove_dir_all(files.directory()).unwrap();
        assert_eq!(index.lookup(&track), None);
    }

    #[test]
    fn a_damaged_index_starts_empty() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join(INDEX_FILE_NAME), b"{ not json").unwrap();
        assert!(StemsIndex::open(folder.path()).entries.is_empty());
    }
}
