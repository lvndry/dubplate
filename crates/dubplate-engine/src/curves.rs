use std::f32::consts::FRAC_PI_2;

/// Gains of the left (deck A) and right (deck B) sides for a crossfader position.
///
/// Position 0.0 is fully left and 1.0 fully right. The law is constant power: the two gains
/// are the cosine and sine of the same angle, so a blend of two tracks keeps the same loudness
/// across the throw and both sides sit at -3 dB in the middle.
pub fn crossfader_gains(position: f32) -> (f32, f32) {
    if position <= 0.0 {
        return (1.0, 0.0);
    }
    if position >= 1.0 {
        return (0.0, 1.0);
    }
    let angle = position * FRAC_PI_2;
    (angle.cos(), angle.sin())
}

/// Gain of a channel fader at a position from 0.0 (closed) to 1.0 (fully open).
///
/// The curve is the square of the position, close to the audio taper of a club mixer's
/// channel fader: half way up is about -12 dB.
pub fn channel_fader_gain(position: f32) -> f32 {
    let position = position.clamp(0.0, 1.0);
    position * position
}

/// Converts decibels to a linear gain factor.
pub fn decibels_to_gain(decibels: f32) -> f32 {
    10.0_f32.powf(decibels / 20.0)
}

/// Converts a linear gain factor to decibels.
pub fn gain_to_decibels(gain: f32) -> f32 {
    20.0 * gain.max(f32::MIN_POSITIVE).log10()
}

/// A value that moves to a new target in a straight line over a fixed number of frames, so
/// gain changes never jump between two samples.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Ramp {
    current: f32,
    target: f32,
    step: f32,
    ramp_frames: u32,
}

impl Ramp {
    pub(crate) fn new(value: f32, ramp_frames: u32) -> Self {
        Self {
            current: value,
            target: value,
            step: 0.0,
            ramp_frames: ramp_frames.max(1),
        }
    }

    pub(crate) fn set_target(&mut self, target: f32) {
        self.target = target;
        self.step = (target - self.current) / self.ramp_frames as f32;
    }

    pub(crate) fn jump_to(&mut self, value: f32) {
        self.current = value;
        self.target = value;
        self.step = 0.0;
    }

    pub(crate) fn target(&self) -> f32 {
        self.target
    }

    pub(crate) fn current(&self) -> f32 {
        self.current
    }

    pub(crate) fn next_value(&mut self) -> f32 {
        if self.step != 0.0 {
            self.current += self.step;
            let arrived = (self.step > 0.0 && self.current >= self.target)
                || (self.step < 0.0 && self.current <= self.target);
            if arrived {
                self.current = self.target;
                self.step = 0.0;
            }
        }
        self.current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ramp_reaches_target_in_the_given_frames() {
        let mut ramp = Ramp::new(0.0, 4);
        ramp.set_target(1.0);
        let values: Vec<f32> = (0..6).map(|_| ramp.next_value()).collect();
        assert_eq!(values, vec![0.25, 0.5, 0.75, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn decibel_conversions_round_trip() {
        assert!((decibels_to_gain(-6.0) - 0.501_187).abs() < 1e-5);
        assert!((gain_to_decibels(decibels_to_gain(-12.0)) + 12.0).abs() < 1e-4);
    }
}
