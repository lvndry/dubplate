use crate::unit::Frame;

/// Sample-rate and bit-depth reduction: holds each sample for as long as the reduced rate asks
/// and rounds it to fewer levels. The Crusher and the Crush Echo share it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SampleCrush {
    hold_phase: f32,
    held: Frame,
}

impl Default for SampleCrush {
    fn default() -> Self {
        Self {
            hold_phase: 1.0,
            held: [0.0; 2],
        }
    }
}

impl SampleCrush {
    /// Crushes one frame to `levels` steps either side of zero, taking a new sample every
    /// `1 / rate_step` samples.
    #[inline]
    pub(crate) fn tick(&mut self, input: Frame, levels: f32, rate_step: f32) -> Frame {
        self.hold_phase += rate_step;
        if self.hold_phase >= 1.0 {
            self.hold_phase = (self.hold_phase - 1.0).min(1.0);
            self.held = input;
        }
        self.held.map(|sample| (sample * levels).round() / levels)
    }
}
