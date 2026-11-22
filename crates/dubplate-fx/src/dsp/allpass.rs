use super::delay_line::DelayLine;
use super::shaping::flush_denormal;
use crate::config::EffectConfig;

/// A Schroeder all-pass diffuser: it passes every frequency at the same level but smears it in
/// time, the building block of reverbs and of the Spiral's bloom.
#[derive(Clone, Debug)]
pub(crate) struct Allpass {
    line: DelayLine,
    delay_samples: f32,
}

impl Allpass {
    /// Builds a diffuser that can delay up to `milliseconds` at the highest sample rate. This
    /// allocates.
    pub(crate) fn new(config: &EffectConfig, milliseconds: f32) -> Self {
        Self {
            line: DelayLine::new(config.samples_for(milliseconds / 1000.0) + 4),
            delay_samples: 1.0,
        }
    }

    /// Sets the delay to `milliseconds` at `sample_rate`, rounded to whole samples.
    pub(crate) fn prepare(&mut self, milliseconds: f32, sample_rate: f32) {
        let samples = (milliseconds / 1000.0 * sample_rate).round();
        self.delay_samples = samples.clamp(1.0, self.line.max_delay());
    }

    pub(crate) fn clear(&mut self) {
        self.line.clear();
    }

    #[inline]
    pub(crate) fn tick(&mut self, input: f32, gain: f32) -> f32 {
        let delayed = self.line.read_linear(self.delay_samples);
        let stored = flush_denormal(input + gain * delayed);
        self.line.write(stored);
        delayed - gain * stored
    }
}
