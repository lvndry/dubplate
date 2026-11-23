use crate::beat::BeatLength;
use crate::config::EffectConfig;
use crate::context::BlockTiming;
use crate::dsp::delay_line::DelayLine;
use crate::effect::EffectKind;
use crate::unit::{Controls, Frame, Kernel, MixLaw, Unit, implement_effect};

const DIP_SECONDS: f32 = 0.003;
const TRAIL_MARGIN_SECONDS: f32 = 0.05;
const DEFAULT_DEPTH: f32 = 0.5;

/// A reverse delay: every beat length of music comes back played backwards, one beat length
/// later, each reversed slice faded in and out so the seams never click. A new beat length
/// comes in behind a dip of a few milliseconds, while tempo changes take effect at a seam.
///
/// The depth knob balances the music and the reversed sound, as on a DJ mixer.
pub(crate) struct ReverseDelay {
    unit: Unit<ReverseKernel>,
}

impl ReverseDelay {
    pub(crate) fn new(config: &EffectConfig) -> Self {
        Self {
            unit: Unit::new(ReverseKernel::new(config), DEFAULT_DEPTH),
        }
    }
}

implement_effect!(ReverseDelay);

impl Controls for ReverseDelay {
    fn effect_kind(&self) -> EffectKind {
        EffectKind::RevDelay
    }

    fn apply_depth(&mut self, depth: f32) {
        self.unit.set_mix(depth);
    }

    fn apply_beat(&mut self, beat: BeatLength) {
        let kernel = &mut self.unit.kernel;
        if kernel.beat != beat {
            kernel.beat = beat;
            kernel.switching_beat = true;
        }
    }

    fn current_beat(&self) -> Option<BeatLength> {
        Some(self.unit.kernel.beat)
    }
}

/// Records consecutive slices and reads the preceding slice from end to start.
pub(crate) struct ReverseKernel {
    lines: [DelayLine; 2],
    beat: BeatLength,
    window_samples: usize,
    wanted_window_samples: usize,
    position: usize,
    switching_beat: bool,
    dip: f32,
    dip_step: f32,
    sample_rate: f32,
}

impl ReverseKernel {
    fn new(config: &EffectConfig) -> Self {
        Self {
            lines: std::array::from_fn(|_| DelayLine::new(config.max_delay_samples().max(2))),
            beat: BeatLength::ONE,
            window_samples: 0,
            wanted_window_samples: 0,
            position: 0,
            switching_beat: false,
            dip: 1.0,
            dip_step: 1.0,
            sample_rate: 48_000.0,
        }
    }

    fn change_window(&mut self) {
        self.window_samples = self.wanted_window_samples;
        self.position = 0;
        self.switching_beat = false;
    }
}

impl Kernel for ReverseKernel {
    const TRAILS: bool = true;
    const MIX_LAW: MixLaw = MixLaw::Blend;

    fn begin_block(&mut self, timing: &BlockTiming) {
        self.sample_rate = timing.sample_rate;
        self.dip_step = 1.0 / (DIP_SECONDS * timing.sample_rate);
        let longest = (self.lines[0].max_delay() as usize).div_ceil(2);
        let samples = timing.beats_to_samples(self.beat.beats()).round() as usize;
        self.wanted_window_samples = samples.clamp(1, longest);
        if self.window_samples == 0 {
            self.change_window();
        }
    }

    #[inline]
    fn tick(&mut self, input: Frame, _beat_position: f64) -> Frame {
        if self.switching_beat && self.window_samples != self.wanted_window_samples {
            self.dip -= self.dip_step;
            if self.dip <= 0.0 {
                self.dip = 0.0;
                self.change_window();
            }
        } else {
            if self.position == 0 {
                self.change_window();
            }
            self.dip = (self.dip + self.dip_step).min(1.0);
        }
        // The write head advances by one sample while the read head walks back by one.
        let delay = (2 * self.position + 1) as f32;
        let edge = self.position.min(self.window_samples - 1 - self.position);
        let gain = (edge as f32 * self.dip_step).min(1.0) * self.dip;
        self.position = (self.position + 1) % self.window_samples;
        let [left, right] = &mut self.lines;
        let output = [
            left.read_linear(delay) * gain,
            right.read_linear(delay) * gain,
        ];
        left.write(input[0]);
        right.write(input[1]);
        output
    }

    fn reset(&mut self) {
        for line in &mut self.lines {
            line.clear();
        }
        self.dip = 1.0;
        self.change_window();
    }

    fn tail_seconds(&self) -> f32 {
        2.0 * self.window_samples.max(self.wanted_window_samples) as f32 / self.sample_rate
            + TRAIL_MARGIN_SECONDS
    }
}

#[cfg(test)]
mod tests {
    use super::ReverseKernel;
    use crate::config::EffectConfig;
    use crate::context::BlockTiming;
    use crate::unit::Kernel;

    #[test]
    fn reset_restores_the_dip_after_a_poisoned_reset() {
        let mut kernel = ReverseKernel::new(&EffectConfig::default());
        let timing = BlockTiming {
            sample_rate: 48_000.0,
            beat_position: 0.0,
            beats_per_sample: 120.0 / 60.0 / 48_000.0,
        };
        kernel.begin_block(&timing);
        kernel.dip = 0.0;
        kernel.window_samples = 1;
        kernel.reset();
        assert_eq!(kernel.dip, 1.0);
        assert_eq!(kernel.window_samples, kernel.wanted_window_samples);
    }
}
