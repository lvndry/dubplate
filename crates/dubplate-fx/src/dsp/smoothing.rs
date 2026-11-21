const SETTLED_DIFFERENCE: f32 = 1e-6;

/// A parameter that glides towards its target along a one-pole curve.
#[derive(Clone, Debug)]
pub(crate) struct Smoothed {
    current: f32,
    target: f32,
    time_constant_seconds: f32,
    coefficient: f32,
    sample_rate: f32,
}

impl Smoothed {
    pub(crate) fn new(value: f32, time_constant_seconds: f32) -> Self {
        Self {
            current: value,
            target: value,
            time_constant_seconds,
            coefficient: 0.0,
            sample_rate: 0.0,
        }
    }

    pub(crate) fn prepare(&mut self, sample_rate: f32) {
        if sample_rate != self.sample_rate {
            self.sample_rate = sample_rate;
            self.coefficient = (-1.0 / (self.time_constant_seconds * sample_rate)).exp();
        }
    }

    pub(crate) fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    pub(crate) fn snap_to_target(&mut self) {
        self.current = self.target;
    }

    pub(crate) fn target(&self) -> f32 {
        self.target
    }

    pub(crate) fn current(&self) -> f32 {
        self.current
    }

    pub(crate) fn is_settled(&self) -> bool {
        self.current == self.target
    }

    #[inline]
    pub(crate) fn next(&mut self) -> f32 {
        if self.current != self.target {
            let next = self.target + (self.current - self.target) * self.coefficient;
            let stalled = next == self.current;
            self.current = if stalled || (next - self.target).abs() <= SETTLED_DIFFERENCE {
                self.target
            } else {
                next
            };
        }
        self.current
    }
}

#[cfg(test)]
mod tests {
    use super::Smoothed;

    #[test]
    fn glides_to_the_target_without_jumping() {
        let mut smoothed = Smoothed::new(0.0, 0.01);
        smoothed.prepare(48_000.0);
        smoothed.set_target(1.0);
        let first = smoothed.next();
        assert!(first > 0.0 && first < 0.01);
        for _ in 0..48_000 {
            smoothed.next();
        }
        assert!(smoothed.is_settled());
        assert_eq!(smoothed.current(), 1.0);
    }
}
