use std::f32::consts::PI;

use crate::beat::BeatLength;
use crate::config::EffectConfig;
use crate::context::BlockTiming;
use crate::dsp::delay_line::{DelayLine, MIN_DELAY_SAMPLES};
use crate::effect::EffectKind;
use crate::unit::{Controls, Frame, Kernel, MixLaw, Unit, engaged, implement_effect};

const GRAIN_SECONDS: f32 = 0.06;
const LONGEST_STRETCH: f32 = 4.0;
const FULL_MIX_DEPTH: f32 = 0.1;

/// Slows the music down without changing its pitch, starting again on every retrigger.
///
/// From the start of each retrigger cycle (the beat length, 1 beat by default, counted on the
/// beat grid) the music plays stretched, as if time ran slower, and the next cycle starts again
/// from the live music. Overlapping grains a few tens of milliseconds long keep the pitch.
///
/// The depth knob sets the stretch, from none to four times slower, and blends it in over the
/// first tenth of its travel.
pub struct Stretch {
    unit: Unit<StretchKernel>,
}

impl Stretch {
    /// Builds a stretch retriggering every beat, switched off. This allocates.
    pub fn new(config: &EffectConfig) -> Self {
        Self {
            unit: Unit::new(StretchKernel::new(config), 1.0),
        }
    }
}

implement_effect!(Stretch);

impl Controls for Stretch {
    fn effect_kind(&self) -> EffectKind {
        EffectKind::Stretch
    }

    fn apply_depth(&mut self, depth: f32) {
        self.unit.set_mix(engaged(depth, FULL_MIX_DEPTH));
        self.unit.kernel.slowdown = 1.0 + (LONGEST_STRETCH - 1.0) * depth;
    }

    fn apply_beat(&mut self, beat: BeatLength) {
        self.unit.kernel.beat = beat;
    }

    fn current_beat(&self) -> Option<BeatLength> {
        Some(self.unit.kernel.beat)
    }
}

pub(crate) struct StretchKernel {
    lines: [DelayLine; 2],
    beat: BeatLength,
    slowdown: f32,
    cycle_index: i64,
    since_trigger: f32,
    grain_phase: f32,
    grain_delays: [f32; 2],
    grain_samples: f32,
}

impl StretchKernel {
    fn new(config: &EffectConfig) -> Self {
        let capacity = config.max_delay_samples();
        Self {
            lines: [DelayLine::new(capacity), DelayLine::new(capacity)],
            beat: BeatLength::ONE,
            slowdown: 1.0,
            cycle_index: i64::MIN,
            since_trigger: 0.0,
            grain_phase: 0.0,
            grain_delays: [MIN_DELAY_SAMPLES; 2],
            grain_samples: 1.0,
        }
    }

    /// How far behind the live music the stretched sound is now: it has fallen behind by the
    /// share of the time it did not play.
    fn lag(&self) -> f32 {
        let lag = self.since_trigger * (1.0 - 1.0 / self.slowdown);
        let upper = (self.lines[0].max_delay() - self.grain_samples).max(MIN_DELAY_SAMPLES);
        lag.clamp(MIN_DELAY_SAMPLES, upper)
    }
}

impl Kernel for StretchKernel {
    const TRAILS: bool = false;
    const LISTENS_WHEN_OFF: bool = true;
    const MIX_LAW: MixLaw = MixLaw::Crossfade;

    fn begin_block(&mut self, timing: &BlockTiming) {
        self.grain_samples = GRAIN_SECONDS * timing.sample_rate;
    }

    #[inline]
    fn tick(&mut self, input: Frame, beat_position: f64) -> Frame {
        let cycle = (beat_position / self.beat.beats()).floor() as i64;
        if cycle != self.cycle_index {
            self.cycle_index = cycle;
            self.since_trigger = 0.0;
        }
        self.since_trigger += 1.0;
        let previous_phase = self.grain_phase;
        self.grain_phase = (self.grain_phase + 1.0 / self.grain_samples).rem_euclid(1.0);
        let lag = self.lag();
        if self.grain_phase < previous_phase {
            self.grain_delays[0] = lag;
        }
        let second_phase = (self.grain_phase + 0.5).rem_euclid(1.0);
        if second_phase < (previous_phase + 0.5).rem_euclid(1.0) {
            self.grain_delays[1] = lag;
        }
        let heads = [
            (self.grain_phase, self.grain_delays[0]),
            (second_phase, self.grain_delays[1]),
        ];
        let mut output = [0.0; 2];
        for (line, (channel_input, channel_output)) in
            self.lines.iter_mut().zip(input.iter().zip(&mut output))
        {
            for (phase, delay) in heads {
                let gain = (PI * phase).sin();
                *channel_output += line.read_linear(delay) * gain * gain;
            }
            line.write(*channel_input);
        }
        output
    }

    fn reset(&mut self) {
        for line in &mut self.lines {
            line.clear();
        }
        self.cycle_index = i64::MIN;
        self.since_trigger = 0.0;
        self.grain_phase = 0.0;
        self.grain_delays = [MIN_DELAY_SAMPLES; 2];
    }
}

#[cfg(test)]
mod tests {
    use super::StretchKernel;
    use crate::config::EffectConfig;
    use crate::context::BlockTiming;
    use crate::unit::Kernel;

    #[test]
    fn lag_stays_clamped_when_the_line_is_shorter_than_the_grain() {
        let mut kernel = StretchKernel::new(&EffectConfig {
            max_sample_rate: 48_000.0,
            max_delay_seconds: 0.0,
        });
        let timing = BlockTiming {
            sample_rate: 48_000.0,
            beat_position: 0.0,
            beats_per_sample: 120.0 / 60.0 / 48_000.0,
        };
        kernel.begin_block(&timing);
        kernel.since_trigger = 1_000.0;
        kernel.slowdown = 4.0;
        let lag = kernel.lag();
        assert!(lag >= super::MIN_DELAY_SAMPLES);
    }
}
