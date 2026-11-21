use std::f32::consts::PI;

use super::shaping::{flush_denormal, soft_limit};

const MIN_CUTOFF_HZ: f32 = 10.0;
const MAX_CUTOFF_RATIO: f32 = 0.48;
const RESONANCE_STATE_CEILING: f32 = 6.0;

/// The prewarped gain of a topology-preserving integrator at `cutoff_hz`.
#[inline]
pub(crate) fn integrator_gain(cutoff_hz: f32, sample_rate: f32) -> f32 {
    let cutoff = cutoff_hz.clamp(MIN_CUTOFF_HZ, sample_rate * MAX_CUTOFF_RATIO);
    (PI * cutoff / sample_rate).tan()
}

/// Coefficients of a topology-preserving state-variable filter.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SvfCoefficients {
    damping: f32,
    first: f32,
    second: f32,
    third: f32,
}

impl SvfCoefficients {
    pub(crate) fn new(cutoff_hz: f32, quality: f32, sample_rate: f32) -> Self {
        let gain = integrator_gain(cutoff_hz, sample_rate);
        let damping = 1.0 / quality.max(0.1);
        let first = 1.0 / (1.0 + gain * (gain + damping));
        let second = gain * first;
        let third = gain * second;
        Self {
            damping,
            first,
            second,
            third,
        }
    }
}

/// The three outputs of one state-variable filter step.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SvfOutput {
    pub(crate) low: f32,
    pub(crate) band: f32,
    pub(crate) high: f32,
}

/// A two-pole state-variable filter in the zero-delay-feedback form, which stays stable and
/// click-free while its cutoff moves every sample.
///
/// The band state saturates softly far above full scale, so a high resonance rings like an
/// analogue filter instead of growing without limit.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Svf {
    band_state: f32,
    low_state: f32,
}

impl Svf {
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }

    #[inline]
    pub(crate) fn tick(&mut self, input: f32, coefficients: &SvfCoefficients) -> SvfOutput {
        let high_part = input - self.low_state;
        let band = coefficients.first * self.band_state + coefficients.second * high_part;
        let low =
            self.low_state + coefficients.second * self.band_state + coefficients.third * high_part;
        self.band_state = flush_denormal(soft_limit(
            2.0 * band - self.band_state,
            RESONANCE_STATE_CEILING,
        ));
        self.low_state = flush_denormal(2.0 * low - self.low_state);
        SvfOutput {
            low,
            band: band * coefficients.damping,
            high: input - coefficients.damping * band - low,
        }
    }
}

/// A one-pole topology-preserving filter giving low-pass, high-pass and all-pass outputs.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct OnePole {
    state: f32,
}

/// The gain of a [`OnePole`] at a cutoff, computed once and shared by every channel.
#[inline]
pub(crate) fn one_pole_gain(cutoff_hz: f32, sample_rate: f32) -> f32 {
    let integrator = integrator_gain(cutoff_hz, sample_rate);
    integrator / (1.0 + integrator)
}

impl OnePole {
    pub(crate) fn reset(&mut self) {
        self.state = 0.0;
    }

    #[inline]
    pub(crate) fn low_pass(&mut self, input: f32, gain: f32) -> f32 {
        let step = (input - self.state) * gain;
        let low = step + self.state;
        self.state = flush_denormal(low + step);
        low
    }

    #[inline]
    pub(crate) fn high_pass(&mut self, input: f32, gain: f32) -> f32 {
        input - self.low_pass(input, gain)
    }

    #[inline]
    pub(crate) fn all_pass(&mut self, input: f32, gain: f32) -> f32 {
        2.0 * self.low_pass(input, gain) - input
    }
}

#[cfg(test)]
mod tests {
    use super::{OnePole, Svf, SvfCoefficients, one_pole_gain};

    #[test]
    fn low_pass_settles_to_unity_for_a_constant() {
        let coefficients = SvfCoefficients::new(1_000.0, std::f32::consts::FRAC_1_SQRT_2, 48_000.0);
        let mut filter = Svf::default();
        let mut last = 0.0;
        for _ in 0..4_800 {
            last = filter.tick(0.5, &coefficients).low;
        }
        assert!((last - 0.5).abs() < 1e-4);
    }

    #[test]
    fn one_pole_high_pass_removes_a_constant() {
        let gain = one_pole_gain(100.0, 48_000.0);
        let mut filter = OnePole::default();
        let mut last = 1.0;
        for _ in 0..48_000 {
            last = filter.high_pass(0.5, gain);
        }
        assert!(last.abs() < 1e-4);
    }
}
