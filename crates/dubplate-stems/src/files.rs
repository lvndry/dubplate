//! The stem files of one track in the cache.

use std::path::{Path, PathBuf};

use dubplate_engine::{EngineError, Stem, StemTracks};
use euphoria_core::stems as separation;

/// Why a track's stems could not be loaded for a deck.
#[derive(Debug, thiserror::Error)]
pub enum StemLoadError {
    /// A stem file could not be read.
    #[error("could not read {}: {source}", path.display())]
    Read {
        /// The stem file.
        path: PathBuf,
        /// What went wrong.
        source: hound::Error,
    },
    /// A stem file is not 16-bit stereo at the rate of the others.
    #[error("{} is not a 16-bit stereo stem at {sample_rate} Hz", path.display())]
    Format {
        /// The stem file.
        path: PathBuf,
        /// The rate the stems must share.
        sample_rate: u32,
    },
    /// The engine refused the stems, for example because they differ in length.
    #[error(transparent)]
    Engine(#[from] EngineError),
}

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
    pub fn in_folder(directory: PathBuf) -> Option<Self> {
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

    /// Reads the four files into memory for a deck: about 300 MB for seven minutes, which
    /// takes a fraction of a second. Run it off the audio and interface threads.
    pub fn load(&self) -> Result<StemTracks, StemLoadError> {
        let stems = self.paths().map(|path| read_stem(&path, STEMS_SAMPLE_RATE));
        let [drums, bass, melody, vocals] = stems;
        let samples = [drums?, bass?, melody?, vocals?];
        Ok(StemTracks::from_interleaved_stereo(
            std::array::from_fn(|stem| samples[stem].as_slice()),
            STEMS_SAMPLE_RATE,
        )?)
    }

    /// The name of the folder inside the cache, which names the model and the track's content.
    pub(crate) fn entry_name(&self) -> Option<&str> {
        self.directory.file_name()?.to_str()
    }
}

/// The interleaved 16-bit samples of the stereo WAV file at `path`, which must be at
/// `sample_rate`.
fn read_stem(path: &Path, sample_rate: u32) -> Result<Vec<i16>, StemLoadError> {
    let read_error = |source| StemLoadError::Read {
        path: path.to_path_buf(),
        source,
    };
    let reader = hound::WavReader::open(path).map_err(read_error)?;
    let spec = reader.spec();
    let expected = spec.channels == 2
        && spec.bits_per_sample == 16
        && spec.sample_format == hound::SampleFormat::Int
        && spec.sample_rate == sample_rate;
    if !expected {
        return Err(StemLoadError::Format {
            path: path.to_path_buf(),
            sample_rate,
        });
    }
    reader
        .into_samples::<i16>()
        .collect::<Result<Vec<_>, _>>()
        .map_err(read_error)
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

    fn write_stem(path: &Path, samples: &[i16], sample_rate: u32) {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        for sample in samples {
            writer.write_sample(*sample).unwrap();
        }
        writer.finalize().unwrap();
    }

    #[test]
    fn stems_load_for_a_deck_and_refuse_a_file_of_another_kind() {
        let folder = tempfile::tempdir().unwrap();
        let files = StemFiles {
            directory: folder.path().to_path_buf(),
        };
        for stem in Stem::ALL {
            write_stem(&files.path(stem), &[0; 88_200], STEMS_SAMPLE_RATE);
        }
        let stems = files.load().unwrap();
        assert_eq!(stems.duration_seconds(), 1.0);

        write_stem(&files.path(Stem::Bass), &[0; 96_000], 48_000);
        assert!(matches!(
            files.load(),
            Err(StemLoadError::Format { sample_rate, .. }) if sample_rate == STEMS_SAMPLE_RATE
        ));
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
        assert_eq!(StemFiles::in_folder(directory.clone()), None);
        std::fs::write(directory.join("vocals.wav"), b"").unwrap();
        assert!(StemFiles::in_folder(directory).is_some());
    }
}
