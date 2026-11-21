use crate::dsp::delay_line::DelayLine;
use crate::dsp::delay_time::DelayTaps;
use crate::dsp::filters::{OnePole, one_pole_gain};
use crate::dsp::loudness::{FeedbackGovernor, loop_loudness_gain};
use crate::dsp::shaping::saturate;

const REPEAT_LOW_PASS_HZ: f32 = 6_500.0;
const REPEAT_HIGH_PASS_HZ: f32 = 90.0;
const LOUDNESS_KEPT_PER_PASS: f32 = 0.9;
const TRAIL_LEVEL: f32 = 0.7;

/// Where the depth knob of the echo and the ping-pong delay starts: a trail that is clearly
/// heard but sits under the music.
pub(crate) const DEFAULT_DEPTH: f32 = 0.6;

/// The mix the depth knob of the echo and the ping-pong delay sets: up to the middle of the
/// knob, where the trail plays at its full level and the music still plays at its own.
pub(crate) const fn mix_for_depth(depth: f32) -> f32 {
    0.5 * depth
}

/// The feedback the depth knob of the echo and the ping-pong delay sets, from 0.35 to 0.85.
pub(crate) const fn feedback_for_depth(depth: f32) -> f32 {
    0.35 + 0.5 * depth
}

/// Ages each echo repeat like tape: a little darker, a little thinner, gently saturated.
///
/// The first repeat is untouched; only what goes back round the loop is shaped, so long
/// feedback fades into a warm wash instead of a harsh or boomy one.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RepeatTone {
    low_pass: OnePole,
    high_pass: OnePole,
}

/// The filter gains of [`RepeatTone`] at one sample rate.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RepeatToneGains {
    low_pass: f32,
    high_pass: f32,
}

impl RepeatToneGains {
    pub(crate) fn new(sample_rate: f32) -> Self {
        Self {
            low_pass: one_pole_gain(REPEAT_LOW_PASS_HZ, sample_rate),
            high_pass: one_pole_gain(REPEAT_HIGH_PASS_HZ, sample_rate),
        }
    }
}

impl RepeatTone {
    pub(crate) fn reset(&mut self) {
        self.low_pass.reset();
        self.high_pass.reset();
    }

    #[inline]
    pub(crate) fn shape(&mut self, repeat: f32, gains: &RepeatToneGains) -> f32 {
        let darker = self.low_pass.low_pass(repeat, gains.low_pass);
        saturate(self.high_pass.high_pass(darker, gains.high_pass))
    }
}

/// Reads a delay line at the taps of a [`crate::dsp::delay_time::DelayTime`].
#[inline]
pub(crate) fn read_taps(line: &DelayLine, taps: &DelayTaps) -> f32 {
    let current = line.read_cubic(taps.current) * taps.current_weight;
    if taps.previous_weight > 0.0 {
        current + line.read_cubic(taps.previous) * taps.previous_weight
    } else {
        current
    }
}

/// The feedback and output level of a trail of repeats.
///
/// The output is scaled by the feedback so a trail sounds about as loud at any feedback: more
/// feedback makes it last longer, not louder. The scaling matches the trail's power, not its
/// peak, and the Headroom stage is the peak safety net. The tape tone takes a little off every
/// pass, so even full feedback leaves a finite trail to scale by. At full mix the trail plays
/// about 3 dB under the music that feeds it. The feedback itself is governed so the repeats
/// never build up past the music that went in.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TrailLevel {
    governor: FeedbackGovernor,
}

/// The gains of one sample of a trail.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TrailGains {
    /// How much of each repeat goes back round the loop.
    pub(crate) feedback: f32,
    /// How loud the trail plays.
    pub(crate) output: f32,
}

impl TrailLevel {
    pub(crate) const fn new() -> Self {
        Self {
            governor: FeedbackGovernor::new(),
        }
    }

    pub(crate) fn prepare(&mut self, sample_rate: f32) {
        self.governor.prepare(sample_rate);
    }

    /// Sets the longest time, in samples, the loop takes to play back what went in.
    pub(crate) fn set_loop_samples(&mut self, samples: f32) {
        self.governor.set_loop_samples(samples);
    }

    pub(crate) fn reset(&mut self) {
        self.governor.reset();
    }

    /// Works out the gains for one sample from the feedback knob, the loudest sample going into
    /// the loop and the loudest coming out of it.
    #[inline]
    pub(crate) fn next(
        &mut self,
        feedback: f32,
        input_magnitude: f32,
        trail_magnitude: f32,
    ) -> TrailGains {
        TrailGains {
            feedback: feedback * self.governor.next(input_magnitude, trail_magnitude),
            // The knob's feedback, not the governed one: the buildup to compensate depends on the
            // intended loop gain.
            output: TRAIL_LEVEL * loop_loudness_gain(feedback * LOUDNESS_KEPT_PER_PASS),
        }
    }
}
