const FALLBACK_SAMPLE_RATE: f32 = 48_000.0;
const MIN_SAMPLE_RATE: f32 = 8_000.0;
const MAX_SAMPLE_RATE: f32 = 768_000.0;
/// Tempo used by beat effects when no track grid is available.
pub const FALLBACK_TEMPO_BPM: f64 = 120.0;
/// The slowest tempo effects follow, in beats per minute. A slower tempo is clamped to it.
pub const MIN_TEMPO_BPM: f64 = 20.0;
/// The fastest tempo effects follow, in beats per minute. A faster tempo is clamped to it.
pub const MAX_TEMPO_BPM: f64 = 999.0;

/// The format and musical timing of one block of audio.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProcessContext {
    /// The sample rate of the block in hertz.
    pub sample_rate: f32,
    /// The tempo of the music in beats per minute. Effects fall back to 120 BPM when it is zero,
    /// negative or not a number, for example on a track with no beat grid.
    pub tempo_bpm: f64,
    /// The beat phase of the block's first sample, counted in beats from the start of the beat
    /// grid. The whole part counts beats, so effects that span several beats line up with bars
    /// and phrases; the fractional part is the phase within the current beat.
    pub beat_position: f64,
}

impl ProcessContext {
    /// Builds a context for a block starting at `beat_position` beats.
    pub fn new(sample_rate: f32, tempo_bpm: f64, beat_position: f64) -> Self {
        Self {
            sample_rate,
            tempo_bpm,
            beat_position,
        }
    }

    /// The phase within the current beat, from 0 up to but not including 1.
    pub fn beat_phase(&self) -> f64 {
        self.beat_position.rem_euclid(1.0)
    }
}

/// A [`ProcessContext`] with every value checked and the per-sample beat step worked out.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BlockTiming {
    pub(crate) sample_rate: f32,
    pub(crate) beat_position: f64,
    pub(crate) beats_per_sample: f64,
}

impl BlockTiming {
    pub(crate) fn from_context(context: &ProcessContext) -> Self {
        let sample_rate = if context.sample_rate.is_finite() && context.sample_rate > 0.0 {
            context.sample_rate.clamp(MIN_SAMPLE_RATE, MAX_SAMPLE_RATE)
        } else {
            FALLBACK_SAMPLE_RATE
        };
        let tempo_bpm = if context.tempo_bpm.is_finite() && context.tempo_bpm > 0.0 {
            context.tempo_bpm.clamp(MIN_TEMPO_BPM, MAX_TEMPO_BPM)
        } else {
            FALLBACK_TEMPO_BPM
        };
        let beat_position = if context.beat_position.is_finite() {
            context.beat_position
        } else {
            0.0
        };
        Self {
            sample_rate,
            beat_position,
            beats_per_sample: tempo_bpm / 60.0 / f64::from(sample_rate),
        }
    }

    pub(crate) fn beats_to_samples(&self, beats: f64) -> f64 {
        beats / self.beats_per_sample
    }
}
