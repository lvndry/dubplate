use crate::tempo::BeatClock;

/// A block of stereo audio handed to processors, stored as one buffer per channel.
///
/// The buffers are allocated once with a fixed capacity and never grow, so a block can be
/// reused on the audio thread without allocating.
#[derive(Debug, Clone)]
pub struct StereoBlock {
    left: Vec<f32>,
    right: Vec<f32>,
    frame_count: usize,
}

impl StereoBlock {
    /// Creates a silent block that can hold up to `capacity` frames, with all of them in use.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            left: vec![0.0; capacity],
            right: vec![0.0; capacity],
            frame_count: capacity,
        }
    }

    /// Maximum number of frames the block can hold.
    pub fn capacity(&self) -> usize {
        self.left.len()
    }

    /// Number of frames in use.
    pub fn frame_count(&self) -> usize {
        self.frame_count
    }

    /// Sets how many frames are in use, clamped to the capacity.
    pub fn set_frame_count(&mut self, frame_count: usize) {
        self.frame_count = frame_count.min(self.capacity());
    }

    /// Left channel samples in use.
    pub fn left(&self) -> &[f32] {
        &self.left[..self.frame_count]
    }

    /// Right channel samples in use.
    pub fn right(&self) -> &[f32] {
        &self.right[..self.frame_count]
    }

    /// Both channels in use, for writing.
    pub fn channels_mut(&mut self) -> (&mut [f32], &mut [f32]) {
        (
            &mut self.left[..self.frame_count],
            &mut self.right[..self.frame_count],
        )
    }

    /// Sets every sample in use to silence.
    pub fn silence(&mut self) {
        let (left, right) = self.channels_mut();
        left.fill(0.0);
        right.fill(0.0);
    }

    /// Largest absolute sample value in use, across both channels.
    pub fn peak(&self) -> f32 {
        self.left()
            .iter()
            .chain(self.right())
            .fold(0.0_f32, |peak, sample| peak.max(sample.abs()))
    }
}

/// What a processor needs to know about the block it is processing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProcessContext {
    /// Output sample rate in Hz.
    pub sample_rate: u32,
    /// Playback speed of the deck the processor sits on, where 1.0 is the track's own tempo.
    /// Always 1.0 on the master bus.
    pub tempo: f64,
    /// The tempo and beat position of the music, when its beat grid is known. On the master bus
    /// it is the tempo master deck's.
    pub beat_clock: Option<BeatClock>,
}

/// An effect or other processing stage that sits in a deck's or the master's insert slot.
///
/// `process` runs on the audio thread. It must not allocate, lock, block, or do I/O, and it must
/// finish well within the block's duration. Allocate everything the processor needs when it is
/// created, before it is handed to the engine; when it is replaced, the engine hands the old
/// processor back to the control thread so it is never dropped on the audio thread.
pub trait AudioProcessor: Send {
    /// Processes one block in place.
    fn process(&mut self, block: &mut StereoBlock, context: &ProcessContext);
}
