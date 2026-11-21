use crate::dsp::delay_line::DelayLine;
use crate::dsp::delay_time::DelayTaps;
use crate::dsp::filters::{OnePole, one_pole_gain};
use crate::dsp::shaping::saturate;

const REPEAT_LOW_PASS_HZ: f32 = 6_500.0;
const REPEAT_HIGH_PASS_HZ: f32 = 90.0;

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
