use std::f32::consts::FRAC_PI_2;

const GLIDE_SECONDS: f32 = 0.05;
const CROSSFADE_SECONDS: f32 = 0.04;
const JUMP_RATIO: f64 = 0.12;
const SETTLED_SAMPLES: f64 = 1e-4;

/// Where to read a delay line this sample: the current time, and while a crossfade runs, the
/// time being faded out.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DelayTaps {
    pub(crate) current: f32,
    pub(crate) current_weight: f32,
    pub(crate) previous: f32,
    pub(crate) previous_weight: f32,
}

/// A delay time that follows a moving target the way a DJ expects.
///
/// Small changes, such as a tempo nudge, glide, so the echoes bend in pitch like tape. Large
/// changes, such as a new beat length, crossfade from the old time to the new one, so the
/// echoes jump cleanly instead of sweeping through every time in between.
#[derive(Clone, Debug)]
pub(crate) struct DelayTime {
    current: f64,
    target: f64,
    previous: f64,
    crossfade_position: f32,
    crossfade_step: f32,
    glide_coefficient: f64,
    sample_rate: f32,
    primed: bool,
}

impl DelayTime {
    pub(crate) fn new() -> Self {
        Self {
            current: 0.0,
            target: 0.0,
            previous: 0.0,
            crossfade_position: 1.0,
            crossfade_step: 0.0,
            glide_coefficient: 0.0,
            sample_rate: 0.0,
            primed: false,
        }
    }

    pub(crate) fn set_target(&mut self, samples: f32, sample_rate: f32) {
        if sample_rate != self.sample_rate {
            self.sample_rate = sample_rate;
            self.crossfade_step = 1.0 / (CROSSFADE_SECONDS * sample_rate);
            self.glide_coefficient = (-1.0 / f64::from(GLIDE_SECONDS * sample_rate)).exp();
        }
        self.target = f64::from(samples);
        if !self.primed {
            self.current = self.target;
            self.primed = true;
        }
    }

    pub(crate) fn longest(&self) -> f32 {
        self.current.max(self.previous).max(self.target) as f32
    }

    #[inline]
    pub(crate) fn next(&mut self) -> DelayTaps {
        if self.crossfade_position >= 1.0 && self.current != self.target {
            let difference = (self.target - self.current).abs();
            if difference > self.current * JUMP_RATIO {
                self.previous = self.current;
                self.current = self.target;
                self.crossfade_position = 0.0;
            } else {
                self.current = self.target + (self.current - self.target) * self.glide_coefficient;
                if difference < SETTLED_SAMPLES {
                    self.current = self.target;
                }
            }
        }
        if self.crossfade_position < 1.0 {
            self.crossfade_position = (self.crossfade_position + self.crossfade_step).min(1.0);
            let angle = self.crossfade_position * FRAC_PI_2;
            DelayTaps {
                current: self.current as f32,
                current_weight: angle.sin(),
                previous: self.previous as f32,
                previous_weight: angle.cos(),
            }
        } else {
            DelayTaps {
                current: self.current as f32,
                current_weight: 1.0,
                previous: self.current as f32,
                previous_weight: 0.0,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DelayTime;

    #[test]
    fn starts_at_its_first_target() {
        let mut time = DelayTime::new();
        time.set_target(12_000.0, 48_000.0);
        let taps = time.next();
        assert_eq!(taps.current, 12_000.0);
        assert_eq!(taps.previous_weight, 0.0);
    }

    #[test]
    fn crossfades_on_a_large_jump_and_glides_on_a_small_one() {
        let mut time = DelayTime::new();
        time.set_target(12_000.0, 48_000.0);
        time.next();
        time.set_target(24_000.0, 48_000.0);
        let taps = time.next();
        assert_eq!(taps.current, 24_000.0);
        assert_eq!(taps.previous, 12_000.0);
        assert!(taps.previous_weight > 0.99);

        let mut time = DelayTime::new();
        time.set_target(12_000.0, 48_000.0);
        time.next();
        time.set_target(11_250.0, 48_000.0);
        let taps = time.next();
        assert!(taps.current < 12_000.0 && taps.current > 11_900.0);
        assert_eq!(taps.previous_weight, 0.0);
    }
}
