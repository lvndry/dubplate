//! The stem files of one track in the cache.

use std::path::{Path, PathBuf};

use dubplate_engine::{EngineError, Stem, StemTracks};
use euphoria_core::stems as separation;

/// How far into a track, from either end, a seam's overlap reaches: 32 seconds, 16 bars at
/// 120 BPM, the longest blend a set plans.
pub const SEAM_EDGE_SECONDS: f32 = 32.0;

/// Whether the vocals sound near the start and near the end of a track, where it meets its
/// neighbours in a set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VocalPresence {
    /// The vocals sound in the first [`SEAM_EDGE_SECONDS`].
    pub at_start: bool,
    /// The vocals sound in the last [`SEAM_EDGE_SECONDS`].
    pub at_end: bool,
}

impl VocalPresence {
    /// Where the vocals sound, from windows of the track as `(start, length, vocals)`, in
    /// seconds.
    fn from_windows(windows: impl IntoIterator<Item = (f32, f32, bool)>) -> Self {
        let windows: Vec<(f32, f32, bool)> = windows.into_iter().collect();
        let end = windows
            .iter()
            .map(|(start, length, _)| start + length)
            .fold(0.0, f32::max);
        let sung = |inside: &dyn Fn(f32, f32) -> bool| {
            windows
                .iter()
                .any(|(start, length, vocals)| *vocals && inside(*start, start + length))
        };
        Self {
            at_start: sung(&|start, _| start < SEAM_EDGE_SECONDS),
            at_end: sung(&|_, window_end| window_end > end - SEAM_EDGE_SECONDS),
        }
    }
}

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
    /// A stem could not be resampled to the track's rate.
    #[error("could not resample {}: {reason}", path.display())]
    Resample {
        /// The stem file.
        path: PathBuf,
        /// What went wrong.
        reason: String,
    },
    /// The vocals stem could not be measured.
    #[error("could not measure the vocals in {}: {reason}", directory.display())]
    Vocals {
        /// The stems folder.
        directory: PathBuf,
        /// What went wrong.
        reason: String,
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

    /// Reads the four files into memory for a deck playing a track at `sample_rate`, the
    /// track's own rate: stems separated at [`STEMS_SAMPLE_RATE`] are resampled to it, so the
    /// deck reads stems and track at the same positions. About 320 MB for seven minutes at
    /// 48 kHz, read and resampled in a few seconds: run it off the audio and interface threads.
    pub fn load(&self, sample_rate: u32) -> Result<StemTracks, StemLoadError> {
        let stems = self.paths().map(|path| {
            let samples = read_stem(&path, STEMS_SAMPLE_RATE)?;
            resample(&samples, STEMS_SAMPLE_RATE, sample_rate)
                .map_err(|reason| StemLoadError::Resample { path, reason })
        });
        let [drums, bass, melody, vocals] = stems;
        let samples = [drums?, bass?, melody?, vocals?];
        Ok(StemTracks::from_interleaved_stereo(
            std::array::from_fn(|stem| samples[stem].as_slice()),
            sample_rate,
        )?)
    }

    /// Where the vocals sound near the track's start and end. The vocals stem is measured once
    /// and the levels are kept next to the stems, so later calls only read them back.
    pub fn vocal_presence(&self) -> Result<VocalPresence, StemLoadError> {
        let activity = separation::vocal_activity(&separation::StemFiles::in_directory(
            self.directory.clone(),
        ))
        .map_err(|error| StemLoadError::Vocals {
            directory: self.directory.clone(),
            reason: error.to_string(),
        })?;
        Ok(VocalPresence::from_windows(activity.windows.iter().map(
            |window| (window.start_seconds, window.duration_seconds, window.active),
        )))
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

/// Interleaved 16-bit stereo `samples` at `source_rate`, resampled to `target_rate` with the
/// same FFT resampler the separation decodes with. The same samples when the rates match.
fn resample(samples: &[i16], source_rate: u32, target_rate: u32) -> Result<Vec<i16>, String> {
    use rubato::audioadapter_buffers::direct::SequentialSliceOfVecs;
    use rubato::{Fft, FixedSync, Resampler};

    const CHUNK_FRAMES: usize = 4096;
    const SCALE: f32 = 32_768.0;
    if source_rate == target_rate {
        return Ok(samples.to_vec());
    }
    let frames = samples.len() / 2;
    let channels: Vec<Vec<f32>> = (0..2)
        .map(|channel| {
            samples
                .iter()
                .skip(channel)
                .step_by(2)
                .map(|sample| f32::from(*sample) / SCALE)
                .collect()
        })
        .collect();
    let mut resampler = Fft::<f32>::new(
        source_rate as usize,
        target_rate as usize,
        CHUNK_FRAMES,
        2,
        FixedSync::Input,
    )
    .map_err(|error| error.to_string())?;
    let input =
        SequentialSliceOfVecs::new(&channels, 2, frames).map_err(|error| error.to_string())?;
    let output = resampler
        .process_all(&input, frames, None)
        .map_err(|error| error.to_string())?;
    Ok(output
        .take_data()
        .into_iter()
        .map(|sample| (sample * SCALE).round().clamp(-SCALE, SCALE - 1.0) as i16)
        .collect())
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
        let stems = files.load(STEMS_SAMPLE_RATE).unwrap();
        assert_eq!(stems.duration_seconds(), 1.0);
        let resampled = files.load(48_000).unwrap();
        assert_eq!(resampled.sample_rate(), 48_000);
        assert!((resampled.duration_seconds() - 1.0).abs() < 1e-3);

        write_stem(&files.path(Stem::Bass), &[0; 96_000], 48_000);
        assert!(matches!(
            files.load(STEMS_SAMPLE_RATE),
            Err(StemLoadError::Format { sample_rate, .. }) if sample_rate == STEMS_SAMPLE_RATE
        ));
    }

    #[test]
    fn vocals_count_only_near_either_end_of_the_track() {
        let window = |start: f32, vocals| (start, 4.0, vocals);
        let verse_in_the_middle = (0..60).map(|index| {
            let start = index as f32 * 4.0;
            window(start, (100.0..140.0).contains(&start))
        });
        assert_eq!(
            VocalPresence::from_windows(verse_in_the_middle),
            VocalPresence::default()
        );
        let sung_intro_and_outro = (0..60).map(|index| {
            let start = index as f32 * 4.0;
            window(start, start < 8.0 || start > 220.0)
        });
        assert_eq!(
            VocalPresence::from_windows(sung_intro_and_outro),
            VocalPresence {
                at_start: true,
                at_end: true
            }
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
        assert_eq!(StemFiles::in_folder(directory.clone()), None);
        std::fs::write(directory.join("vocals.wav"), b"").unwrap();
        assert!(StemFiles::in_folder(directory).is_some());
    }
}
