use std::sync::atomic::{AtomicU8, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use crate::error::EngineError;

const CHUNK_FRAMES_LOG2: u32 = 16;
const CHUNK_FRAMES: usize = 1 << CHUNK_FRAMES_LOG2;
const CHUNK_FRAME_MASK: usize = CHUNK_FRAMES - 1;
/// Longest track a deck can hold, in seconds.
pub const MAX_TRACK_SECONDS: usize = 4 * 60 * 60;
/// Lowest sample rate a track can have, in Hz.
pub const MIN_SAMPLE_RATE: u32 = 8_000;
/// Highest sample rate a track can have, in Hz.
pub const MAX_SAMPLE_RATE: u32 = 384_000;
const UNKNOWN_LENGTH: usize = usize::MAX;

const STATUS_DECODING: u8 = 0;
const STATUS_COMPLETE: u8 = 1;
const STATUS_FAILED: u8 = 2;

/// A track's decoded audio, shared between the thread that decodes it and the audio thread.
///
/// Stereo frames are stored in chunks that the decoder allocates as it goes. The decoder
/// publishes how many frames are ready with a release store, and the audio thread only reads
/// frames below that count, so reading never waits and never sees a half-written frame. Samples
/// are kept as `f32` bits in `AtomicU32`, which costs nothing over plain loads and stores on the
/// platforms Dubplate targets.
#[derive(Clone)]
pub struct Track {
    inner: Arc<TrackBuffer>,
}

pub(crate) struct TrackBuffer {
    sample_rate: u32,
    source_channels: u16,
    chunks: Box<[OnceLock<Box<[AtomicU32]>>]>,
    decoded_frames: AtomicUsize,
    total_frames: AtomicUsize,
    status: AtomicU8,
    failure: OnceLock<String>,
}

/// Where a track's decoding stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeStatus {
    /// Frames are still being decoded.
    Decoding,
    /// Every frame is decoded.
    Complete,
    /// Decoding stopped with an error; frames decoded before it still play.
    Failed(String),
}

/// What reading one frame found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum FrameRead {
    Frame(f32, f32),
    /// Before the start or after the end of the track.
    Outside,
    /// Not decoded yet.
    Missing,
}

impl Track {
    /// Builds a complete track from interleaved stereo samples already in memory. Fails when
    /// the sample rate is outside 8 kHz to 384 kHz or the samples last longer than
    /// [`MAX_TRACK_SECONDS`].
    pub fn from_interleaved_stereo(samples: &[f32], sample_rate: u32) -> Result<Self, EngineError> {
        check_sample_rate(sample_rate)?;
        let mut writer = TrackWriter::new(sample_rate, 2);
        for [left, right] in samples.as_chunks::<2>().0 {
            if !writer.push_frame(*left, *right) {
                return Err(EngineError::InvalidParameter {
                    name: "track length in seconds",
                    value: samples.len() as f64 / 2.0 / f64::from(sample_rate),
                });
            }
        }
        writer.finish();
        Ok(writer.track())
    }

    pub(crate) fn buffer(&self) -> &Arc<TrackBuffer> {
        &self.inner
    }

    pub(crate) fn from_buffer(inner: Arc<TrackBuffer>) -> Self {
        Self { inner }
    }

    /// Sample rate of the decoded audio in Hz, which is the file's own rate.
    pub fn sample_rate(&self) -> u32 {
        self.inner.sample_rate
    }

    /// Number of channels in the source file. The engine plays mono as stereo and keeps the
    /// first two channels of files with more.
    pub fn source_channels(&self) -> u16 {
        self.inner.source_channels
    }

    /// Number of frames decoded so far.
    pub fn decoded_frames(&self) -> usize {
        self.inner.decoded_frames.load(Ordering::Acquire)
    }

    /// Seconds of audio decoded so far.
    pub fn decoded_seconds(&self) -> f64 {
        self.decoded_frames() as f64 / f64::from(self.inner.sample_rate)
    }

    /// Length in seconds once decoding is complete.
    pub fn duration_seconds(&self) -> Option<f64> {
        self.inner
            .total_frames()
            .map(|frames| frames as f64 / f64::from(self.inner.sample_rate))
    }

    /// One decoded frame as `[left, right]`, or `None` when it is not decoded yet or past the
    /// end. Useful for drawing waveforms.
    pub fn frame(&self, index: usize) -> Option<[f32; 2]> {
        match self.inner.read_frame(i64::try_from(index).ok()?) {
            FrameRead::Frame(left, right) => Some([left, right]),
            FrameRead::Outside | FrameRead::Missing => None,
        }
    }

    /// Copies the decoded frames from `first_frame` on into `frames`, as `[left, right]`, and
    /// returns how many it copied: fewer than `frames` holds near the end of what is decoded.
    /// Reading a whole track this way costs a fraction of reading it frame by frame.
    pub fn read_frames(&self, first_frame: usize, frames: &mut [[f32; 2]]) -> usize {
        let readable = self
            .inner
            .total_frames()
            .unwrap_or_else(|| self.decoded_frames());
        let wanted = frames.len().min(readable.saturating_sub(first_frame));
        let mut copied = 0;
        while copied < wanted {
            let index = first_frame + copied;
            let Some(chunk) = self
                .inner
                .chunks
                .get(index >> CHUNK_FRAMES_LOG2)
                .and_then(OnceLock::get)
            else {
                break;
            };
            let offset = index & CHUNK_FRAME_MASK;
            let run = (CHUNK_FRAMES - offset).min(wanted - copied);
            let samples = chunk[offset * 2..(offset + run) * 2].as_chunks::<2>().0;
            for (frame, [left, right]) in frames[copied..copied + run].iter_mut().zip(samples) {
                *frame = [
                    f32::from_bits(left.load(Ordering::Relaxed)),
                    f32::from_bits(right.load(Ordering::Relaxed)),
                ];
            }
            copied += run;
        }
        copied
    }

    /// Where decoding stands.
    pub fn status(&self) -> DecodeStatus {
        match self.inner.status.load(Ordering::Acquire) {
            STATUS_COMPLETE => DecodeStatus::Complete,
            STATUS_FAILED => DecodeStatus::Failed(
                self.inner
                    .failure
                    .get()
                    .cloned()
                    .unwrap_or_else(|| "unknown decoding error".to_owned()),
            ),
            _ => DecodeStatus::Decoding,
        }
    }
}

impl std::fmt::Debug for Track {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Track")
            .field("sample_rate", &self.sample_rate())
            .field("decoded_frames", &self.decoded_frames())
            .field("status", &self.status())
            .finish()
    }
}

impl TrackBuffer {
    pub(crate) fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Total frames, known once decoding has finished or failed.
    pub(crate) fn total_frames(&self) -> Option<usize> {
        match self.total_frames.load(Ordering::Acquire) {
            UNKNOWN_LENGTH => None,
            frames => Some(frames),
        }
    }

    /// Reads one frame without waiting. Safe to call on the audio thread.
    pub(crate) fn read_frame(&self, frame_index: i64) -> FrameRead {
        let Ok(index) = usize::try_from(frame_index) else {
            return FrameRead::Outside;
        };
        match self.total_frames() {
            Some(total_frames) if index >= total_frames => return FrameRead::Outside,
            Some(_) => {}
            None if index >= self.decoded_frames.load(Ordering::Acquire) => {
                return FrameRead::Missing;
            }
            None => {}
        }
        let Some(chunk) = self
            .chunks
            .get(index >> CHUNK_FRAMES_LOG2)
            .and_then(OnceLock::get)
        else {
            return FrameRead::Missing;
        };
        let offset = (index & CHUNK_FRAME_MASK) * 2;
        FrameRead::Frame(
            f32::from_bits(chunk[offset].load(Ordering::Relaxed)),
            f32::from_bits(chunk[offset + 1].load(Ordering::Relaxed)),
        )
    }
}

/// Checks that a sample rate is one a track can have.
pub(crate) fn check_sample_rate(sample_rate: u32) -> Result<(), EngineError> {
    if (MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&sample_rate) {
        Ok(())
    } else {
        Err(EngineError::InvalidParameter {
            name: "sample rate",
            value: f64::from(sample_rate),
        })
    }
}

/// Appends decoded frames to a track. Lives on the decoding thread. If it is dropped before
/// `finish` or `fail`, for example because the decoder panicked, the track is marked failed so
/// a deck never waits for frames that will not come.
pub(crate) struct TrackWriter {
    buffer: Arc<TrackBuffer>,
    written_frames: usize,
    closed: bool,
}

impl TrackWriter {
    pub(crate) fn new(sample_rate: u32, source_channels: u16) -> Self {
        let max_frames = MAX_TRACK_SECONDS * sample_rate.max(1) as usize;
        let chunk_count = max_frames.div_ceil(CHUNK_FRAMES);
        let chunks = (0..chunk_count).map(|_| OnceLock::new()).collect();
        Self {
            buffer: Arc::new(TrackBuffer {
                sample_rate,
                source_channels,
                chunks,
                decoded_frames: AtomicUsize::new(0),
                total_frames: AtomicUsize::new(UNKNOWN_LENGTH),
                status: AtomicU8::new(STATUS_DECODING),
                failure: OnceLock::new(),
            }),
            written_frames: 0,
            closed: false,
        }
    }

    pub(crate) fn track(&self) -> Track {
        Track::from_buffer(Arc::clone(&self.buffer))
    }

    /// True when nothing but this writer holds the track, so decoding further is wasted.
    pub(crate) fn is_abandoned(&self) -> bool {
        Arc::strong_count(&self.buffer) == 1
    }

    /// Writes one frame; it becomes readable at the next `publish`. Returns false once the
    /// track has reached the maximum length.
    pub(crate) fn push_frame(&mut self, left: f32, right: f32) -> bool {
        let index = self.written_frames;
        let Some(slot) = self.buffer.chunks.get(index >> CHUNK_FRAMES_LOG2) else {
            return false;
        };
        let chunk = slot.get_or_init(|| {
            (0..CHUNK_FRAMES * 2)
                .map(|_| AtomicU32::new(0))
                .collect::<Box<[AtomicU32]>>()
        });
        let offset = (index & CHUNK_FRAME_MASK) * 2;
        chunk[offset].store(left.to_bits(), Ordering::Relaxed);
        chunk[offset + 1].store(right.to_bits(), Ordering::Relaxed);
        self.written_frames += 1;
        true
    }

    /// Makes every frame written so far readable.
    pub(crate) fn publish(&self) {
        self.buffer
            .decoded_frames
            .store(self.written_frames, Ordering::Release);
    }

    pub(crate) fn finish(&mut self) {
        self.closed = true;
        self.publish();
        self.buffer
            .total_frames
            .store(self.written_frames, Ordering::Release);
        self.buffer.status.store(STATUS_COMPLETE, Ordering::Release);
    }

    pub(crate) fn fail(&mut self, message: String) {
        self.closed = true;
        self.publish();
        let _ = self.buffer.failure.set(message);
        self.buffer
            .total_frames
            .store(self.written_frames, Ordering::Release);
        self.buffer.status.store(STATUS_FAILED, Ordering::Release);
    }
}

impl Drop for TrackWriter {
    fn drop(&mut self) {
        if !self.closed {
            self.fail("the decoder stopped unexpectedly".to_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_become_readable_only_after_publishing() {
        let mut writer = TrackWriter::new(44_100, 2);
        let track = writer.track();
        assert!(writer.push_frame(0.25, -0.25));
        assert_eq!(track.buffer().read_frame(0), FrameRead::Missing);
        writer.publish();
        assert_eq!(track.buffer().read_frame(0), FrameRead::Frame(0.25, -0.25));
        assert_eq!(track.buffer().read_frame(1), FrameRead::Missing);
        assert_eq!(track.buffer().read_frame(-1), FrameRead::Outside);
        writer.finish();
        assert_eq!(track.buffer().read_frame(1), FrameRead::Outside);
        assert_eq!(track.status(), DecodeStatus::Complete);
    }

    #[test]
    fn frames_are_read_in_runs_across_chunks_up_to_what_is_decoded() {
        let frame_count = CHUNK_FRAMES + 10;
        let samples: Vec<f32> = (0..frame_count)
            .flat_map(|index| [index as f32, -(index as f32)])
            .collect();
        let track = Track::from_interleaved_stereo(&samples, 48_000).unwrap();
        let first_frame = CHUNK_FRAMES - 5;
        let mut frames = vec![[0.0; 2]; 100];
        assert_eq!(track.read_frames(first_frame, &mut frames), 15);
        for (offset, frame) in frames[..15].iter().enumerate() {
            let index = (first_frame + offset) as f32;
            assert_eq!(*frame, [index, -index]);
        }
        assert_eq!(track.read_frames(frame_count, &mut frames), 0);

        let mut writer = TrackWriter::new(44_100, 2);
        let decoding = writer.track();
        assert!(writer.push_frame(0.5, -0.5));
        assert!(writer.push_frame(0.25, -0.25));
        assert_eq!(decoding.read_frames(0, &mut frames), 0);
        writer.publish();
        assert_eq!(decoding.read_frames(0, &mut frames), 2);
        assert_eq!(frames[..2], [[0.5, -0.5], [0.25, -0.25]]);
    }

    #[test]
    fn frames_cross_chunk_boundaries() {
        let frame_count = CHUNK_FRAMES + 10;
        let samples: Vec<f32> = (0..frame_count)
            .flat_map(|index| [index as f32, -(index as f32)])
            .collect();
        let track = Track::from_interleaved_stereo(&samples, 48_000).unwrap();
        assert_eq!(track.decoded_frames(), frame_count);
        let boundary = CHUNK_FRAMES as i64;
        assert_eq!(
            track.buffer().read_frame(boundary),
            FrameRead::Frame(boundary as f32, -(boundary as f32))
        );
        assert_eq!(
            track.duration_seconds(),
            Some(frame_count as f64 / 48_000.0)
        );
    }

    #[test]
    fn failure_keeps_decoded_frames_and_reports_the_message() {
        let mut writer = TrackWriter::new(44_100, 1);
        writer.push_frame(0.5, 0.5);
        let track = writer.track();
        writer.fail("broken packet".to_owned());
        assert_eq!(
            track.status(),
            DecodeStatus::Failed("broken packet".to_owned())
        );
        assert_eq!(track.buffer().read_frame(0), FrameRead::Frame(0.5, 0.5));
    }

    #[test]
    fn a_writer_dropped_before_finishing_marks_the_track_failed() {
        let mut writer = TrackWriter::new(44_100, 2);
        let track = writer.track();
        writer.push_frame(0.5, 0.5);
        drop(writer);
        assert!(matches!(track.status(), DecodeStatus::Failed(_)));
        assert_eq!(track.duration_seconds(), Some(1.0 / 44_100.0));
    }

    #[test]
    fn sample_rates_outside_the_supported_range_are_rejected() {
        assert!(Track::from_interleaved_stereo(&[0.0, 0.0], 0).is_err());
        assert!(Track::from_interleaved_stereo(&[0.0, 0.0], 100_000_000).is_err());
        assert!(Track::from_interleaved_stereo(&[0.0, 0.0], 44_100).is_ok());
    }
}
