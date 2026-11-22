//! The stem files of one track in the cache.

use std::path::{Path, PathBuf};

use dubplate_engine::Stem;
use euphoria_core::stems as separation;

/// The sample rate of the stem files: 44.1 kHz, the rate the network was trained at, whatever
/// the rate of the track.
pub const STEMS_SAMPLE_RATE: u32 = separation::STEMS_SAMPLE_RATE;

/// The four WAV files of a track's stems in Dubplate's cache, 16-bit stereo at
/// [`STEMS_SAMPLE_RATE`] and as long as the track.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StemFiles {
    directory: PathBuf,
}

impl StemFiles {
    /// The stems in `directory` when all four files are there.
    pub(crate) fn complete_in(directory: PathBuf) -> Option<Self> {
        let files = Self { directory };
        Stem::ALL
            .iter()
            .all(|stem| files.path(*stem).is_file())
            .then_some(files)
    }

    /// The WAV file of `stem`.
    pub fn path(&self, stem: Stem) -> PathBuf {
        separation::StemFiles::in_directory(self.directory.clone()).path(separated_as(stem))
    }

    /// The four files, in the order of [`Stem::ALL`].
    pub fn paths(&self) -> [PathBuf; Stem::COUNT] {
        Stem::ALL.map(|stem| self.path(stem))
    }

    /// The folder holding the four files.
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// The name of the folder inside the cache, which names the model and the track's content.
    pub(crate) fn entry_name(&self) -> Option<&str> {
        self.directory.file_name()?.to_str()
    }
}

/// The stem of the separating network that holds `stem`: the melody is what the network calls
/// the rest.
fn separated_as(stem: Stem) -> separation::Stem {
    match stem {
        Stem::Drums => separation::Stem::Drums,
        Stem::Bass => separation::Stem::Bass,
        Stem::Melody => separation::Stem::Other,
        Stem::Vocals => separation::Stem::Vocals,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_stem_has_its_own_file_named_as_the_network_names_it() {
        let files = StemFiles {
            directory: PathBuf::from("/cache/entry"),
        };
        assert_eq!(
            files.paths(),
            [
                PathBuf::from("/cache/entry/drums.wav"),
                PathBuf::from("/cache/entry/bass.wav"),
                PathBuf::from("/cache/entry/other.wav"),
                PathBuf::from("/cache/entry/vocals.wav"),
            ]
        );
    }

    #[test]
    fn stems_count_only_when_all_four_files_are_there() {
        let folder = tempfile::tempdir().unwrap();
        let directory = folder.path().to_path_buf();
        for stem in &Stem::ALL[..3] {
            let files = StemFiles {
                directory: directory.clone(),
            };
            std::fs::write(files.path(*stem), b"").unwrap();
        }
        assert_eq!(StemFiles::complete_in(directory.clone()), None);
        std::fs::write(directory.join("vocals.wav"), b"").unwrap();
        assert!(StemFiles::complete_in(directory).is_some());
    }
}
