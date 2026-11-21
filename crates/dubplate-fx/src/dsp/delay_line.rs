const INTERPOLATION_MARGIN: usize = 4;

/// The shortest delay a cubic read can serve: it needs one sample newer than the read point.
pub(crate) const MIN_DELAY_SAMPLES: f32 = 2.0;

/// A circular buffer of past samples with fractional reads.
///
/// Within one sample period, read before writing: `read(delay)` then returns the sample written
/// `delay` periods ago.
#[derive(Clone, Debug)]
pub(crate) struct DelayLine {
    buffer: Vec<f32>,
    mask: usize,
    write_index: usize,
}

impl DelayLine {
    pub(crate) fn new(max_delay_samples: usize) -> Self {
        let length = (max_delay_samples + INTERPOLATION_MARGIN).next_power_of_two();
        Self {
            buffer: vec![0.0; length],
            mask: length - 1,
            write_index: 0,
        }
    }

    pub(crate) fn max_delay(&self) -> f32 {
        (self.buffer.len() - INTERPOLATION_MARGIN) as f32
    }

    pub(crate) fn clear(&mut self) {
        self.buffer.fill(0.0);
    }

    #[inline]
    pub(crate) fn write(&mut self, sample: f32) {
        self.buffer[self.write_index] = sample;
        self.write_index = (self.write_index + 1) & self.mask;
    }

    #[inline]
    fn at(&self, delay: usize) -> f32 {
        self.buffer[self.write_index.wrapping_sub(delay) & self.mask]
    }

    /// Reads with four-point cubic interpolation, exact at whole-sample delays.
    #[inline]
    pub(crate) fn read_cubic(&self, delay_samples: f32) -> f32 {
        let delay = delay_samples.clamp(MIN_DELAY_SAMPLES, self.max_delay());
        let whole = delay.floor();
        let fraction = delay - whole;
        let whole = whole as usize;
        if fraction == 0.0 {
            return self.at(whole);
        }
        let newer = self.at(whole - 1);
        let near = self.at(whole);
        let far = self.at(whole + 1);
        let oldest = self.at(whole + 2);
        let position = 1.0 - fraction;
        let slope_term = 0.5 * (near - oldest);
        let curve_term = oldest - 2.5 * far + 2.0 * near - 0.5 * newer;
        let cubic_term = 0.5 * (newer - oldest) + 1.5 * (far - near);
        ((cubic_term * position + curve_term) * position + slope_term) * position + far
    }

    /// Reads with linear interpolation, which never boosts any frequency inside a feedback loop.
    #[inline]
    pub(crate) fn read_linear(&self, delay_samples: f32) -> f32 {
        let delay = delay_samples.clamp(1.0, self.max_delay());
        let whole = delay.floor();
        let fraction = delay - whole;
        let whole = whole as usize;
        let near = self.at(whole);
        let far = self.at(whole + 1);
        near + (far - near) * fraction
    }
}

#[cfg(test)]
mod tests {
    use super::DelayLine;

    #[test]
    fn whole_sample_reads_return_the_written_sample() {
        let mut line = DelayLine::new(64);
        for sample_index in 0..40 {
            line.write(sample_index as f32);
        }
        assert_eq!(line.read_cubic(5.0), 35.0);
        assert_eq!(line.read_linear(5.0), 35.0);
    }

    #[test]
    fn fractional_reads_follow_a_ramp_exactly() {
        let mut line = DelayLine::new(64);
        for sample_index in 0..40 {
            line.write(sample_index as f32);
        }
        assert!((line.read_cubic(5.25) - 34.75).abs() < 1e-4);
        assert!((line.read_linear(5.5) - 34.5).abs() < 1e-4);
    }
}
