//! Level control shared by the effects that add a trail on top of the music: echo, ping-pong
//! and reverb.
//!
//! Three pieces keep those effects at club-safe levels without touching the dry signal:
//!
//! - [`loop_loudness_gain`] scales a trail by its feedback, so the knob that sets how long the
//!   trail lasts does not also set how loud it is.
//! - [`FeedbackGovernor`] lowers the feedback whenever the trail would build up past the level of
//!   the music that went in.
//! - [`Headroom`] holds the trail under whatever room the dry signal leaves below full scale.

use crate::dsp::shaping::flush_denormal;

/// The loudest the dry and wet signals may add up to: -0.3 dBFS, just under full scale.
const HEADROOM_CEILING: f32 = 0.966;
const HEADROOM_HOLD_SECONDS: f32 = 0.02;
const HEADROOM_RELEASE_SECONDS: f32 = 0.06;

const GOVERNOR_HOLD_MARGIN_SECONDS: f32 = 0.05;
const GOVERNOR_REFERENCE_RELEASE_SECONDS: f32 = 4.0;
const GOVERNOR_TRAIL_HOLD_SECONDS: f32 = 0.02;
const GOVERNOR_TRAIL_RELEASE_SECONDS: f32 = 0.1;
const GOVERNOR_ATTACK_SECONDS: f32 = 0.005;
const GOVERNOR_RECOVERY_SECONDS: f32 = 0.3;

/// The gain that keeps the steady loudness of a decaying loop at the loudness of a single pass.
///
/// A loop that keeps `loop_gain` of its signal on each pass piles up `1 / (1 - loop_gain²)`
/// times the power of one pass when it is fed continuously, so this returns the square root of
/// the inverse. A loop gain at or above 1 returns 0.
#[inline]
pub(crate) fn loop_loudness_gain(loop_gain: f32) -> f32 {
    (1.0 - loop_gain * loop_gain).max(0.0).sqrt()
}

/// A peak level that rises at once, holds, then falls away exponentially.
///
/// It is never below the latest input, so it bounds the signal it follows.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PeakFollower {
    level: f32,
    hold_remaining: u32,
    hold_samples: u32,
    release_coefficient: f32,
}

impl PeakFollower {
    pub(crate) const fn new() -> Self {
        Self {
            level: 0.0,
            hold_remaining: 0,
            hold_samples: 0,
            release_coefficient: 0.0,
        }
    }

    pub(crate) fn prepare(&mut self, hold_seconds: f32, release_seconds: f32, sample_rate: f32) {
        self.set_hold_samples((hold_seconds * sample_rate) as u32);
        self.release_coefficient = (-1.0 / (release_seconds * sample_rate)).exp();
    }

    pub(crate) fn set_hold_samples(&mut self, samples: u32) {
        self.hold_samples = samples;
        self.hold_remaining = self.hold_remaining.min(samples);
    }

    pub(crate) fn reset(&mut self) {
        self.level = 0.0;
        self.hold_remaining = 0;
    }

    /// Follows one sample's magnitude and returns the level.
    #[inline]
    pub(crate) fn next(&mut self, magnitude: f32) -> f32 {
        if magnitude >= self.level {
            self.level = magnitude;
            self.hold_remaining = self.hold_samples;
        } else if self.hold_remaining > 0 {
            self.hold_remaining -= 1;
        } else {
            self.level = flush_denormal(self.level * self.release_coefficient).max(magnitude);
        }
        self.level
    }
}

/// Lowers a feedback loop's gain whenever what comes back out of the loop is louder than the
/// loudest music that went in.
///
/// The level of the music is held for as long as the loop takes to play it back, so a single
/// hit never pulls its own repeats down. Once the music stops, that level falls away slowly,
/// and a trail at full feedback fades with it instead of ringing forever.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FeedbackGovernor {
    reference: PeakFollower,
    trail: PeakFollower,
    gain: f32,
    attack_coefficient: f32,
    recovery_coefficient: f32,
    sample_rate: f32,
}

impl FeedbackGovernor {
    pub(crate) const fn new() -> Self {
        Self {
            reference: PeakFollower::new(),
            trail: PeakFollower::new(),
            gain: 1.0,
            attack_coefficient: 0.0,
            recovery_coefficient: 0.0,
            sample_rate: 0.0,
        }
    }

    pub(crate) fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.reference
            .prepare(0.0, GOVERNOR_REFERENCE_RELEASE_SECONDS, sample_rate);
        self.trail.prepare(
            GOVERNOR_TRAIL_HOLD_SECONDS,
            GOVERNOR_TRAIL_RELEASE_SECONDS,
            sample_rate,
        );
        self.attack_coefficient = (-1.0 / (GOVERNOR_ATTACK_SECONDS * sample_rate)).exp();
        self.recovery_coefficient = (-1.0 / (GOVERNOR_RECOVERY_SECONDS * sample_rate)).exp();
    }

    /// Sets the longest time, in samples, the loop takes to play back what went in.
    pub(crate) fn set_loop_samples(&mut self, samples: f32) {
        let margin = GOVERNOR_HOLD_MARGIN_SECONDS * self.sample_rate;
        self.reference.set_hold_samples((samples + margin) as u32);
    }

    pub(crate) fn reset(&mut self) {
        self.reference.reset();
        self.trail.reset();
        self.gain = 1.0;
    }

    /// Takes the loudest sample going into the loop and the loudest coming out of it, and
    /// returns the factor to multiply the feedback by.
    #[inline]
    pub(crate) fn next(&mut self, input_magnitude: f32, trail_magnitude: f32) -> f32 {
        let reference = self.reference.next(input_magnitude);
        let trail = self.trail.next(trail_magnitude);
        let target = if trail > reference {
            reference / trail
        } else {
            1.0
        };
        let coefficient = if target < self.gain {
            self.attack_coefficient
        } else {
            self.recovery_coefficient
        };
        self.gain = target + (self.gain - target) * coefficient;
        self.gain
    }
}

/// Holds an effect's trail under the room the dry signal leaves below [`HEADROOM_CEILING`].
///
/// It follows the peak level of the dry and of the trail on each channel and picks the one wet
/// gain for both channels that keeps `dry + wet` under the ceiling, so the dry signal is never
/// touched and the stereo image never shifts. The trail is pushed down at once when a peak
/// arrives, usually a kick, and comes back over about a tenth of a second, so the repeats
/// bloom in the gaps between beats.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Headroom {
    dry: [PeakFollower; 2],
    wet: [PeakFollower; 2],
}

impl Headroom {
    pub(crate) const fn new() -> Self {
        Self {
            dry: [PeakFollower::new(); 2],
            wet: [PeakFollower::new(); 2],
        }
    }

    pub(crate) fn prepare(&mut self, sample_rate: f32) {
        for follower in self.dry.iter_mut().chain(&mut self.wet) {
            follower.prepare(HEADROOM_HOLD_SECONDS, HEADROOM_RELEASE_SECONDS, sample_rate);
        }
    }

    pub(crate) fn reset(&mut self) {
        for follower in self.dry.iter_mut().chain(&mut self.wet) {
            follower.reset();
        }
    }

    /// Returns `wanted_gain`, lowered as far as needed for `dry * dry_gain + wet * gain` to stay
    /// under the ceiling on both channels. It updates the peak followers and returns the clamped
    /// gain for the wet signal.
    #[inline]
    pub(crate) fn limit_wet_gain(
        &mut self,
        dry: [f32; 2],
        dry_gain: f32,
        wet: [f32; 2],
        wanted_gain: f32,
    ) -> f32 {
        let mut gain = wanted_gain;
        for channel in 0..2 {
            let dry_level = self.dry[channel].next(dry[channel].abs()) * dry_gain;
            let wet_level = self.wet[channel].next(wet[channel].abs());
            let room = (HEADROOM_CEILING - dry_level).max(0.0);
            if wet_level * gain > room {
                gain = room / wet_level;
            }
        }
        gain
    }
}

#[cfg(test)]
mod tests {
    use super::{FeedbackGovernor, HEADROOM_CEILING, Headroom, PeakFollower, loop_loudness_gain};

    const SAMPLE_RATE: f32 = 48_000.0;

    #[test]
    fn loop_loudness_gain_matches_the_power_of_a_decaying_series() {
        assert_eq!(loop_loudness_gain(0.0), 1.0);
        let loop_gain: f32 = 0.6;
        let power: f32 = (0..400).map(|pass| loop_gain.powi(2 * pass)).sum();
        assert!((loop_loudness_gain(loop_gain) - power.recip().sqrt()).abs() < 1e-5);
        assert_eq!(loop_loudness_gain(1.0), 0.0);
        assert_eq!(loop_loudness_gain(1.5), 0.0);
    }

    #[test]
    fn peak_follower_bounds_its_input_and_holds_before_falling() {
        let mut follower = PeakFollower::new();
        follower.prepare(0.01, 0.05, SAMPLE_RATE);
        assert_eq!(follower.next(0.8), 0.8);
        for _ in 0..480 {
            assert_eq!(follower.next(0.1), 0.8);
        }
        let falling = follower.next(0.1);
        assert!(falling < 0.8 && falling > 0.79);
        for _ in 0..48_000 {
            assert!(follower.next(0.1) >= 0.1);
        }
    }

    #[test]
    fn governor_leaves_a_quieter_trail_alone_and_pulls_a_louder_one_down() {
        let mut governor = FeedbackGovernor::new();
        governor.prepare(SAMPLE_RATE);
        governor.set_loop_samples(12_000.0);
        let mut gain = 1.0;
        for _ in 0..4_800 {
            gain = governor.next(0.8, 0.5);
        }
        assert_eq!(gain, 1.0);
        for _ in 0..4_800 {
            gain = governor.next(0.8, 1.6);
        }
        assert!((gain - 0.5).abs() < 0.01, "gain {gain}");
    }

    #[test]
    fn headroom_keeps_the_sum_under_the_ceiling() {
        let mut headroom = Headroom::new();
        headroom.prepare(SAMPLE_RATE);
        let gain = headroom.limit_wet_gain([0.9, -0.2], 1.0, [0.5, 0.1], 1.0);
        assert!(0.9 + 0.5 * gain <= HEADROOM_CEILING + 1e-6);
        let mut headroom = Headroom::new();
        headroom.prepare(SAMPLE_RATE);
        assert_eq!(
            headroom.limit_wet_gain([0.2, 0.2], 1.0, [0.3, 0.3], 1.0),
            1.0
        );
        assert_eq!(
            headroom.limit_wet_gain([1.0, 0.0], 1.0, [0.3, 0.3], 1.0),
            0.0
        );
    }
}
