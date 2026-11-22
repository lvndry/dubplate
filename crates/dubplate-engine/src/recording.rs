use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use rtrb::{Consumer, Producer, RingBuffer};

use crate::processor::StereoBlock;

const CHANNELS: usize = 2;

/// The control side of a recording tap: the master output exactly as it goes to the speakers,
/// after the master level and the limiter, as interleaved stereo `f32` frames at the engine's
/// sample rate.
///
/// Read it often enough on a thread of your own, for example to write a WAV file. When the
/// buffer fills up, the audio thread drops the frames that do not fit, never waits, and counts
/// them in [`RecordingTap::dropped_frames`].
pub struct RecordingTap {
    consumer: Consumer<f32>,
    dropped_frames: Arc<AtomicU64>,
    sample_rate: u32,
}

impl std::fmt::Debug for RecordingTap {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RecordingTap")
            .field("sample_rate", &self.sample_rate)
            .field("available_frames", &self.available_frames())
            .field("dropped_frames", &self.dropped_frames())
            .finish()
    }
}

impl RecordingTap {
    /// The sample rate of the recorded frames, in hertz.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Frames waiting to be read.
    pub fn available_frames(&self) -> usize {
        self.consumer.slots() / CHANNELS
    }

    /// Moves as many waiting frames as fit into `interleaved`, left then right, and returns how
    /// many frames it moved.
    pub fn read_into(&mut self, interleaved: &mut [f32]) -> usize {
        let frames = self.available_frames().min(interleaved.len() / CHANNELS);
        let Ok(chunk) = self.consumer.read_chunk(frames * CHANNELS) else {
            return 0;
        };
        let (first, second) = chunk.as_slices();
        interleaved[..first.len()].copy_from_slice(first);
        interleaved[first.len()..first.len() + second.len()].copy_from_slice(second);
        chunk.commit_all();
        frames
    }

    /// Frames the audio thread could not fit into the buffer since the tap started.
    pub fn dropped_frames(&self) -> u64 {
        self.dropped_frames.load(Ordering::Relaxed)
    }
}

/// The audio-thread side of a recording tap.
pub(crate) struct RecordingSink {
    producer: Producer<f32>,
    dropped_frames: Arc<AtomicU64>,
}

impl RecordingSink {
    /// Writes the block's frames, dropping and counting the ones that do not fit. Never
    /// allocates or waits.
    pub(crate) fn write(&mut self, block: &StereoBlock) {
        let frames = block.frame_count();
        let writable = frames.min(self.producer.slots() / CHANNELS);
        if writable < frames {
            self.dropped_frames
                .fetch_add((frames - writable) as u64, Ordering::Relaxed);
        }
        if writable == 0 {
            return;
        }
        let Ok(chunk) = self.producer.write_chunk_uninit(writable * CHANNELS) else {
            return;
        };
        let samples = block
            .left()
            .iter()
            .zip(block.right())
            .take(writable)
            .flat_map(|(left, right)| [*left, *right]);
        chunk.fill_from_iter(samples);
    }
}

/// Builds a tap whose buffer holds `capacity_frames` frames: the tap for the control side and
/// the sink for the audio thread.
pub(crate) fn recording_tap(
    capacity_frames: usize,
    sample_rate: u32,
) -> (RecordingTap, RecordingSink) {
    let (producer, consumer) = RingBuffer::new(capacity_frames.max(1) * CHANNELS);
    let dropped_frames = Arc::new(AtomicU64::new(0));
    (
        RecordingTap {
            consumer,
            dropped_frames: Arc::clone(&dropped_frames),
            sample_rate,
        },
        RecordingSink {
            producer,
            dropped_frames,
        },
    )
}
