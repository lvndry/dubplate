use crate::beat::BeatLength;
use crate::config::EffectConfig;
use crate::context::BlockTiming;
use crate::dsp::delay_line::{DelayLine, MIN_DELAY_SAMPLES};
use crate::dsp::smoothing::Smoothed;
use crate::effect::EffectKind;
use crate::unit::{Controls, Frame, Kernel, MixLaw, Unit, implement_effect};

const MOST_TAPS: usize = 7;
const TAP_SPACING_BEATS: f64 = 0.125;
const TAP_DECAY: f32 = 0.85;
const GAIN_SMOOTHING_SECONDS: f32 = 0.02;
const TRAIL_MARGIN_SECONDS: f32 = 0.05;

/// A multi-tap delay: up to seven repeats an eighth of a beat apart, as many as fit in the beat
/// length, each a little quieter than the one before, laid over the music.
///
/// The depth knob raises the odd repeats (the 1st, 3rd, 5th and 7th) over its first half and
/// the even ones over its second half, so it starts as a sparse, syncopated delay and fills in
/// to a stutter.
pub(crate) struct MultiTapDelay {
    unit: Unit<MultiTapKernel>,
}

impl MultiTapDelay {
    pub(crate) fn new(config: &EffectConfig) -> Self {
        Self {
            unit: Unit::new(MultiTapKernel::new(config), 1.0),
        }
    }
}

implement_effect!(MultiTapDelay);

impl Controls for MultiTapDelay {
    fn effect_kind(&self) -> EffectKind {
        EffectKind::MtDelay
    }

    fn apply_depth(&mut self, depth: f32) {
        let odd = (2.0 * depth).clamp(0.0, 1.0);
        let even = (2.0 * depth - 1.0).clamp(0.0, 1.0);
        let mut level = 1.0;
        for (index, gain) in self.unit.kernel.tap_gains.iter_mut().enumerate() {
            let rises = if index % 2 == 0 { odd } else { even };
            gain.set_target(rises * level);
            level *= TAP_DECAY;
        }
    }

    fn apply_beat(&mut self, beat: BeatLength) {
        self.unit.kernel.beat = beat;
    }

    fn current_beat(&self) -> Option<BeatLength> {
        Some(self.unit.kernel.beat)
    }
}

pub(crate) struct MultiTapKernel {
    lines: [DelayLine; 2],
    beat: BeatLength,
    tap_gains: [Smoothed; MOST_TAPS],
    tap_count: usize,
    spacing_samples: f32,
    sample_rate: f32,
}

impl MultiTapKernel {
    fn new(config: &EffectConfig) -> Self {
        let capacity = config.max_delay_samples();
        Self {
            lines: [DelayLine::new(capacity), DelayLine::new(capacity)],
            beat: BeatLength::ONE,
            tap_gains: std::array::from_fn(|_| Smoothed::new(0.0, GAIN_SMOOTHING_SECONDS)),
            tap_count: MOST_TAPS,
            spacing_samples: MIN_DELAY_SAMPLES,
            sample_rate: 48_000.0,
        }
    }
}

impl Kernel for MultiTapKernel {
    const TRAILS: bool = true;
    const MIX_LAW: MixLaw = MixLaw::Layer;

    fn begin_block(&mut self, timing: &BlockTiming) {
        self.sample_rate = timing.sample_rate;
        for gain in &mut self.tap_gains {
            gain.prepare(timing.sample_rate);
        }
        let fitting = (self.beat.beats() / TAP_SPACING_BEATS).round() as usize;
        self.tap_count = fitting.clamp(1, MOST_TAPS);
        let longest = (self.lines[0].max_delay() / self.tap_count as f32).max(MIN_DELAY_SAMPLES);
        self.spacing_samples =
            (timing.beats_to_samples(TAP_SPACING_BEATS) as f32).clamp(MIN_DELAY_SAMPLES, longest);
    }

    #[inline]
    fn tick(&mut self, input: Frame, _beat_position: f64) -> Frame {
        let gains = self.tap_gains.each_mut().map(|gain| gain.next());
        let mut output = [0.0; 2];
        for (line, (channel_input, channel_output)) in
            self.lines.iter_mut().zip(input.iter().zip(&mut output))
        {
            for (tap, gain) in gains.iter().take(self.tap_count).enumerate() {
                let delay = self.spacing_samples * (tap + 1) as f32;
                *channel_output += line.read_linear(delay) * gain;
            }
            line.write(*channel_input);
        }
        output
    }

    fn reset(&mut self) {
        for line in &mut self.lines {
            line.clear();
        }
        for gain in &mut self.tap_gains {
            gain.snap_to_target();
        }
    }

    fn tail_seconds(&self) -> f32 {
        self.spacing_samples * self.tap_count as f32 / self.sample_rate + TRAIL_MARGIN_SECONDS
    }
}

#[cfg(test)]
mod tests {
    use super::MultiTapKernel;
    use crate::config::EffectConfig;
    use crate::context::BlockTiming;
    use crate::unit::Kernel;

    #[test]
    fn begin_block_does_not_panic_with_a_tiny_delay_line() {
        let mut kernel = MultiTapKernel::new(&EffectConfig {
            max_sample_rate: 48_000.0,
            max_delay_seconds: 0.0,
        });
        let timing = BlockTiming {
            sample_rate: 48_000.0,
            beat_position: 0.0,
            beats_per_sample: 120.0 / 60.0 / 48_000.0,
        };
        kernel.begin_block(&timing);
        assert!(kernel.spacing_samples >= super::MIN_DELAY_SAMPLES);
    }
}
