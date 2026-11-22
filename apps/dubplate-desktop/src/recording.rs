//! Recording the mix to a WAV file. The audio thread hands the master output over through a
//! lock-free ring buffer; a writer thread here empties it to disk, so the audio thread never
//! waits on the file.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// How often the writer looks for audio to write when the ring buffer was empty.
const WRITER_INTERVAL: Duration = Duration::from_millis(10);
/// The bit depth of the recordings.
const BITS_PER_SAMPLE: u16 = 24;
/// The largest 24-bit sample.
const FULL_SCALE_24_BIT: f32 = 8_388_607.0;

/// The master output as the engine hands it over for recording: interleaved stereo samples
/// through a ring buffer, the sample rate, and how many frames were lost because the buffer was
/// full.
pub struct RecordingFeed {
    /// Interleaved left and right samples, from -1.0 to 1.0.
    pub samples: rtrb::Consumer<f32>,
    /// The engine's sample rate.
    pub sample_rate: u32,
    /// Frames the audio thread could not hand over.
    pub dropped_frames: Arc<AtomicU64>,
}

/// Why a recording could not start or did not finish.
#[derive(Debug, thiserror::Error)]
pub enum RecordingError {
    /// The recordings folder or the file could not be made or written.
    #[error("{path}: {source}")]
    File {
        /// The file or folder.
        path: PathBuf,
        /// What went wrong.
        source: hound::Error,
    },
    /// The writer thread stopped unexpectedly.
    #[error("the recording stopped unexpectedly")]
    WriterStopped,
}

/// What a finished recording holds.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordingSummary {
    /// The file.
    pub path: PathBuf,
    /// How long it lasts.
    pub duration: Duration,
    /// Frames lost because the writer fell behind, which would be heard as a skip.
    pub dropped_frames: u64,
}

/// A recording in progress.
pub struct Recording {
    path: PathBuf,
    started: Instant,
    stop: Arc<AtomicBool>,
    dropped_frames: Arc<AtomicU64>,
    writer: Option<JoinHandle<Result<Duration, RecordingError>>>,
}

impl Recording {
    /// Starts writing `feed` to a new WAV file in `directory`, named after the time.
    pub fn start(directory: &Path, feed: RecordingFeed) -> Result<Self, RecordingError> {
        let now = jiff::tz::TimeZone::system().to_datetime(jiff::Timestamp::now());
        let path = directory.join(file_name(&now));
        std::fs::create_dir_all(directory).map_err(|source| RecordingError::File {
            path: directory.to_path_buf(),
            source: hound::Error::IoError(source),
        })?;
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: feed.sample_rate,
            bits_per_sample: BITS_PER_SAMPLE,
            sample_format: hound::SampleFormat::Int,
        };
        let writer =
            hound::WavWriter::create(&path, spec).map_err(|source| RecordingError::File {
                path: path.clone(),
                source,
            })?;
        let stop = Arc::new(AtomicBool::new(false));
        let dropped_frames = Arc::clone(&feed.dropped_frames);
        let thread_stop = Arc::clone(&stop);
        let thread_path = path.clone();
        let handle = std::thread::Builder::new()
            .name("dubplate-recording".to_owned())
            .spawn(move || write_until_stopped(writer, feed, &thread_stop, &thread_path))
            .map_err(|source| RecordingError::File {
                path: path.clone(),
                source: hound::Error::IoError(source),
            })?;
        Ok(Self {
            path,
            started: Instant::now(),
            stop,
            dropped_frames,
            writer: Some(handle),
        })
    }

    /// How long the recording has run.
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// Whether the writer stopped on its own, which means it failed.
    pub fn has_failed(&self) -> bool {
        self.writer
            .as_ref()
            .is_some_and(std::thread::JoinHandle::is_finished)
    }

    /// Writes what is left and closes the file.
    pub fn finish(mut self) -> Result<RecordingSummary, RecordingError> {
        self.stop.store(true, Ordering::Release);
        let duration = self
            .writer
            .take()
            .ok_or(RecordingError::WriterStopped)?
            .join()
            .map_err(|_| RecordingError::WriterStopped)??;
        Ok(RecordingSummary {
            path: self.path.clone(),
            duration,
            dropped_frames: self.dropped_frames.load(Ordering::Relaxed),
        })
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(writer) = self.writer.take() {
            writer.join().ok();
        }
    }
}

/// Empties the ring buffer into the file until asked to stop, then empties it once more and
/// closes the file. Returns the length written.
fn write_until_stopped(
    mut writer: hound::WavWriter<std::io::BufWriter<std::fs::File>>,
    mut feed: RecordingFeed,
    stop: &AtomicBool,
    path: &Path,
) -> Result<Duration, RecordingError> {
    let file_error = |source| RecordingError::File {
        path: path.to_path_buf(),
        source,
    };
    let mut samples_written: u64 = 0;
    loop {
        let stopping = stop.load(Ordering::Acquire);
        let available = feed.samples.slots();
        if available > 0 {
            let chunk = feed
                .samples
                .read_chunk(available)
                .map_err(|_| RecordingError::WriterStopped)?;
            let (first, second) = chunk.as_slices();
            for sample in first.iter().chain(second) {
                writer
                    .write_sample(to_24_bit(*sample))
                    .map_err(file_error)?;
            }
            samples_written += available as u64;
            chunk.commit_all();
        } else if stopping {
            break;
        } else {
            std::thread::sleep(WRITER_INTERVAL);
        }
    }
    writer.finalize().map_err(file_error)?;
    let frames = samples_written / 2;
    Ok(Duration::from_secs_f64(
        frames as f64 / f64::from(feed.sample_rate.max(1)),
    ))
}

/// A sample from -1.0 to 1.0 as a 24-bit integer, clipped at full scale.
fn to_24_bit(sample: f32) -> i32 {
    (sample.clamp(-1.0, 1.0) * FULL_SCALE_24_BIT).round() as i32
}

/// `Dubplate 2026-11-22 01-23-45.wav`.
fn file_name(now: &jiff::civil::DateTime) -> String {
    format!("Dubplate {}.wav", now.strftime("%Y-%m-%d %H-%M-%S"))
}

/// Where recordings go: `~/Music/Dubplate Recordings`.
pub fn recordings_directory() -> Option<PathBuf> {
    Some(dirs::audio_dir()?.join("Dubplate Recordings"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recording_writes_every_sample_it_was_handed_as_24_bit_stereo() {
        let directory = tempfile::tempdir().unwrap();
        let (mut producer, consumer) = rtrb::RingBuffer::new(4_800);
        let feed = RecordingFeed {
            samples: consumer,
            sample_rate: 48_000,
            dropped_frames: Arc::new(AtomicU64::new(3)),
        };
        let recording = Recording::start(directory.path(), feed).unwrap();
        for frame in 0..24_000 {
            let value = if frame % 2 == 0 { 0.5 } else { -0.5 };
            while producer.push(value).is_err() {
                std::thread::sleep(Duration::from_millis(1));
            }
            while producer.push(-value).is_err() {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        assert!(!recording.has_failed());
        let summary = recording.finish().unwrap();
        assert!(
            (summary.duration.as_secs_f64() - 0.5).abs() < 1e-9,
            "{summary:?}"
        );
        assert_eq!(summary.dropped_frames, 3);
        let reader = hound::WavReader::open(&summary.path).unwrap();
        let spec = reader.spec();
        assert_eq!(
            (spec.channels, spec.sample_rate, spec.bits_per_sample),
            (2, 48_000, 24)
        );
        let samples: Vec<i32> = reader.into_samples().map(Result::unwrap).collect();
        assert_eq!(samples.len(), 48_000);
        assert_eq!(samples[0], to_24_bit(0.5));
        assert_eq!(samples[1], to_24_bit(-0.5));
        assert!(
            summary
                .path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("Dubplate ")
        );
    }

    #[test]
    fn samples_past_full_scale_are_clipped() {
        assert_eq!(to_24_bit(2.0), 8_388_607);
        assert_eq!(to_24_bit(-2.0), -8_388_607);
        assert_eq!(to_24_bit(0.0), 0);
    }
}
