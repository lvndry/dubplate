use std::f32::consts::FRAC_PI_2;

use crate::channel::KNOB_CENTRE;

const KNOB_DEAD_ZONE: f32 = 0.01;
const EQ_BOOST_DECIBELS: f32 = 6.0;

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

/// The gain of an EQ knob at a position from 0.0 to 1.0, like a DJM's isolator: the centre is
/// unity, fully left kills the band and fully right boosts it by 6 dB.
///
/// The left half follows the square of the travel, the same audio taper as the channel fader:
/// half way down is about -12 dB, so most of the knob shapes the cut and only its last few
/// degrees fall into the kill. The right half rises evenly in decibels. A small dead zone around
/// the centre keeps a knob resting on its detent exactly at unity.
pub fn eq_gain(position: f32) -> f32 {
    let offset = position.clamp(0.0, 1.0) - KNOB_CENTRE;
    if offset.abs() <= KNOB_DEAD_ZONE {
        return 1.0;
    }
    let travel = (offset.abs() - KNOB_DEAD_ZONE) / (KNOB_CENTRE - KNOB_DEAD_ZONE);
    if offset < 0.0 {
        let remaining = 1.0 - travel;
        remaining * remaining
    } else {
        decibels_to_gain(EQ_BOOST_DECIBELS * travel)
    }
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

    /// Whether the value has reached its target and stays there.
    pub(crate) fn is_settled(&self) -> bool {
        self.step == 0.0
    }

    /// Multiplies both channels by the value, moving it on one step per frame. A value at rest
    /// is one multiplication by a constant, which the compiler turns into vector code.
    pub(crate) fn apply(&mut self, left: &mut [f32], right: &mut [f32]) {
        if self.is_settled() {
            let gain = self.current;
            for (left, right) in left.iter_mut().zip(right.iter_mut()) {
                *left *= gain;
                *right *= gain;
            }
            return;
        }
        for (left, right) in left.iter_mut().zip(right.iter_mut()) {
            let gain = self.next_value();
            *left *= gain;
            *right *= gain;
        }
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
    fn applying_a_ramp_matches_stepping_it_frame_by_frame() {
        let samples: Vec<f32> = (0..12).map(|index| index as f32 - 4.5).collect();
        for (start, target) in [(1.0, 1.0), (0.25, 0.25), (0.0, 1.0), (1.0, 0.2)] {
            let mut applied = Ramp::new(start, 5);
            applied.set_target(target);
            let mut stepped = applied;
            let mut left = samples.clone();
            let mut right: Vec<f32> = samples.iter().map(|sample| -sample).collect();
            applied.apply(&mut left, &mut right);
            for (index, sample) in samples.iter().enumerate() {
                let gain = stepped.next_value();
                assert_eq!(left[index], sample * gain);
                assert_eq!(right[index], -sample * gain);
            }
            assert_eq!(applied.current(), stepped.current());
            assert!(applied.is_settled());
        }
    }

    #[test]
    fn the_eq_knob_kills_at_the_left_is_unity_at_the_centre_and_boosts_six_decibels() {
        assert_eq!(eq_gain(0.0), 0.0);
        assert_eq!(eq_gain(0.5), 1.0);
        assert_eq!(eq_gain(0.505), 1.0);
        assert!((gain_to_decibels(eq_gain(1.0)) - 6.0).abs() < 1e-4);
        assert!((gain_to_decibels(eq_gain(0.245)) + 12.0).abs() < 0.3);
        let positions: Vec<f32> = (0..=100).map(|step| step as f32 / 100.0).collect();
        assert!(
            positions
                .windows(2)
                .all(|pair| eq_gain(pair[0]) <= eq_gain(pair[1]))
        );
    }

    #[test]
    fn decibel_conversions_round_trip() {
        assert!((decibels_to_gain(-6.0) - 0.501_187).abs() < 1e-5);
        assert!((gain_to_decibels(decibels_to_gain(-12.0)) + 12.0).abs() < 1e-4);
    }
}
