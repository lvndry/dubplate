/// The limits an effect allocates its buffers for.
///
/// Effects allocate everything in their constructor, so these limits are fixed for the life of
/// an effect. A block at a higher sample rate still plays, with delay times capped to what the
/// buffers hold.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EffectConfig {
    /// The highest sample rate the effect will run at, in hertz.
    pub max_sample_rate: f32,
    /// The longest delay or stored audio the effect keeps, in seconds.
    pub max_delay_seconds: f32,
}

impl EffectConfig {
    pub(crate) fn max_delay_samples(&self) -> usize {
        self.samples_for(self.max_delay_seconds)
    }

    pub(crate) fn samples_for(&self, seconds: f32) -> usize {
        let samples = (seconds.max(0.0) * self.max_sample_rate.max(1.0)).ceil();
        samples as usize
    }
}

impl Default for EffectConfig {
    /// 192 kHz and 10 seconds: sixteen beats of echo at 96 BPM.
    fn default() -> Self {
        Self {
            max_sample_rate: 192_000.0,
            max_delay_seconds: 10.0,
        }
    }
}
