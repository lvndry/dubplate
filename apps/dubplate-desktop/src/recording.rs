//! Recording the mix to a WAV file, exactly as it goes to the speakers after the master level and
//! the limiter. The engine's recording tap hands the audio over without ever waiting; a writer
//! thread here empties it to disk.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;

use dubplate_engine::RecordingTap;
use std::time::{Duration, Instant};

/// How often the writer looks for audio to write when the tap was empty.
const WRITER_INTERVAL: Duration = Duration::from_millis(10);
/// Frames the writer moves from the tap at a time.
const CHUNK_FRAMES: usize = 4_096;
/// Channels in a recording.
const CHANNELS: u16 = 2;
/// The bit depth of the recordings.
const BITS_PER_SAMPLE: u16 = 24;
/// The largest 24-bit sample.
const FULL_SCALE_24_BIT: f32 = 8_388_607.0;

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
    /// The writer still closing the file after a timed-out `finish_within`.
    detached: Option<JoinHandle<Result<Duration, RecordingError>>>,
}

impl Recording {
    /// Starts writing `tap` to a new WAV file in `directory`, named after the time.
    pub fn start(directory: &Path, tap: RecordingTap) -> Result<Self, RecordingError> {
        let now = jiff::tz::TimeZone::system().to_datetime(jiff::Timestamp::now());
        std::fs::create_dir_all(directory).map_err(|source| RecordingError::File {
            path: directory.to_path_buf(),
            source: hound::Error::IoError(source),
        })?;
        let spec = hound::WavSpec {
            channels: CHANNELS,
            sample_rate: tap.sample_rate(),
            bits_per_sample: BITS_PER_SAMPLE,
            sample_format: hound::SampleFormat::Int,
        };
        let (path, file) = create_recording_file(directory, &now)?;
        let writer =
            hound::WavWriter::new(std::io::BufWriter::new(file), spec).map_err(|source| {
                RecordingError::File {
                    path: path.clone(),
                    source,
                }
            })?;
        let stop = Arc::new(AtomicBool::new(false));
        let dropped_frames = Arc::new(AtomicU64::new(0));
        let thread_stop = Arc::clone(&stop);
        let thread_dropped = Arc::clone(&dropped_frames);
        let thread_path = path.clone();
        let handle = std::thread::Builder::new()
            .name("dubplate-recording".to_owned())
            .spawn(move || {
                write_until_stopped(writer, tap, &thread_stop, &thread_dropped, &thread_path)
            })
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
            detached: None,
        })
    }

    /// How long the recording has run.
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// Frames lost so far because the writer fell behind.
    pub fn dropped_frames(&self) -> u64 {
        self.dropped_frames.load(Ordering::Relaxed)
    }

    /// Whether the writer stopped on its own, which means it failed.
    pub fn has_failed(&self) -> bool {
        self.writer.as_ref().is_some_and(JoinHandle::is_finished)
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

    /// As `finish`, but for quit: the writer is told to stop, waited on briefly, and when it is
    /// still closing the file, kept closing it in the background so the app is not held, while
    /// the file it already wrote is finished as is.
    pub fn finish_within(mut self, limit: Duration) {
        self.stop.store(true, Ordering::Release);
        let Some(writer) = self.writer.take() else {
            return;
        };
        let deadline = Instant::now() + limit;
        while !writer.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        if writer.is_finished() {
            writer.join().ok();
        } else {
            self.detached = Some(writer);
        }
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(writer) = self.writer.take() {
            writer.join().ok();
        }
        if let Some(writer) = self.detached.take() {
            writer.join().ok();
        }
    }
}

/// Empties the tap into the file until asked to stop, then empties it once more and closes the
/// file. Returns the length written.
fn write_until_stopped(
    mut writer: hound::WavWriter<std::io::BufWriter<std::fs::File>>,
    mut tap: RecordingTap,
    stop: &AtomicBool,
    dropped_frames: &AtomicU64,
    path: &Path,
) -> Result<Duration, RecordingError> {
    let file_error = |source| RecordingError::File {
        path: path.to_path_buf(),
        source,
    };
    let mut buffer = vec![0.0; CHUNK_FRAMES * usize::from(CHANNELS)];
    let mut frames_written: u64 = 0;
    loop {
        let stopping = stop.load(Ordering::Acquire);
        let frames = tap.read_into(&mut buffer);
        dropped_frames.store(tap.dropped_frames(), Ordering::Relaxed);
        if frames > 0 {
            for sample in &buffer[..frames * usize::from(CHANNELS)] {
                writer
                    .write_sample(to_24_bit(*sample))
                    .map_err(file_error)?;
            }
            frames_written += frames as u64;
        } else if stopping {
            break;
        } else {
            std::thread::sleep(WRITER_INTERVAL);
        }
    }
    writer.finalize().map_err(file_error)?;
    Ok(Duration::from_secs_f64(
        frames_written as f64 / f64::from(tap.sample_rate().max(1)),
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

/// Reserves a new filename atomically, including when two recordings start concurrently.
fn create_recording_file(
    directory: &Path,
    now: &jiff::civil::DateTime,
) -> Result<(PathBuf, std::fs::File), RecordingError> {
    let name = file_name(now);
    let stem = name.strip_suffix(".wav").unwrap_or(&name);
    let mut suffix = 0_u64;
    loop {
        let path = directory.join(if suffix == 0 {
            name.clone()
        } else {
            format!("{stem} ({suffix}).wav")
        });
        match std::fs::File::options()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => return Ok((path, file)),
            Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {
                suffix = suffix.checked_add(1).ok_or(RecordingError::File {
                    path,
                    source: hound::Error::IoError(source),
                })?;
            }
            Err(source) => {
                return Err(RecordingError::File {
                    path,
                    source: hound::Error::IoError(source),
                });
            }
        }
    }
}

/// Where recordings go: `~/Music/Dubplate Recordings`.
pub fn recordings_directory() -> Option<PathBuf> {
    Some(dirs::audio_dir()?.join("Dubplate Recordings"))
}

#[cfg(test)]
mod tests {
    use dubplate_engine::{DeckId, Track, offline_engine};

    use super::*;

    const SAMPLE_RATE: u32 = 48_000;
    const BLOCK_FRAMES: usize = 480;
    const BLOCKS: usize = 50;

    #[test]
    fn a_recording_writes_the_master_as_24_bit_stereo() {
        let directory = tempfile::tempdir().unwrap();
        let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
        let samples = vec![0.25; SAMPLE_RATE as usize * 2 * 2];
        let track = Track::from_interleaved_stereo(&samples, SAMPLE_RATE).unwrap();
        controller.load_track(DeckId::A, track).unwrap();
        controller.play(DeckId::A).unwrap();
        let tap = controller.start_recording(2.0).unwrap();
        let mut block = vec![0.0; BLOCK_FRAMES * 2];
        renderer.render(&mut block, 2);
        let recording = Recording::start(directory.path(), tap).unwrap();
        for _ in 0..BLOCKS {
            renderer.render(&mut block, 2);
        }
        controller.stop_recording().unwrap();
        renderer.render(&mut block, 2);
        assert!(!recording.has_failed());
        let summary = recording.finish().unwrap();
        assert_eq!(summary.dropped_frames, 0);
        let reader = hound::WavReader::open(&summary.path).unwrap();
        let spec = reader.spec();
        assert_eq!(
            (spec.channels, spec.sample_rate, spec.bits_per_sample),
            (2, SAMPLE_RATE, 24)
        );
        let written: Vec<i32> = reader.into_samples().map(Result::unwrap).collect();
        let frames = written.len() / 2;
        assert!(
            (BLOCKS * BLOCK_FRAMES..=(BLOCKS + 2) * BLOCK_FRAMES).contains(&frames),
            "{frames} frames"
        );
        assert!(
            written.iter().any(|sample| *sample != 0),
            "the mix is heard"
        );
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
    fn audio_the_writer_could_not_keep_up_with_is_counted() {
        let directory = tempfile::tempdir().unwrap();
        let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
        let tap = controller.start_recording(0.01).unwrap();
        let mut block = vec![0.0; BLOCK_FRAMES * 2];
        for _ in 0..BLOCKS {
            renderer.render(&mut block, 2);
        }
        let recording = Recording::start(directory.path(), tap).unwrap();
        let summary = recording.finish().unwrap();
        assert!(summary.dropped_frames > 0, "{summary:?}");
        assert!(summary.duration <= Duration::from_millis(20), "{summary:?}");
    }

    #[test]
    fn recordings_in_the_same_second_never_replace_each_other() {
        let directory = tempfile::tempdir().unwrap();
        let now: jiff::civil::DateTime = "2026-11-22T01:23:45".parse().unwrap();
        let (first, mut file) = create_recording_file(directory.path(), &now).unwrap();
        std::io::Write::write_all(&mut file, b"previous recording").unwrap();
        let (second, _) = create_recording_file(directory.path(), &now).unwrap();
        assert_ne!(first, second);
        assert_eq!(std::fs::read(first).unwrap(), b"previous recording");
    }

    #[test]
    fn concurrent_recordings_reserve_distinct_files() {
        let directory = tempfile::tempdir().unwrap();
        let now: jiff::civil::DateTime = "2026-11-22T01:23:45".parse().unwrap();
        let paths = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| scope.spawn(|| create_recording_file(directory.path(), &now).unwrap().0))
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect::<std::collections::HashSet<_>>()
        });
        assert_eq!(paths.len(), 8);
    }

    #[test]
    fn samples_past_full_scale_are_clipped() {
        assert_eq!(to_24_bit(2.0), 8_388_607);
        assert_eq!(to_24_bit(-2.0), -8_388_607);
        assert_eq!(to_24_bit(0.0), 0);
    }
}
