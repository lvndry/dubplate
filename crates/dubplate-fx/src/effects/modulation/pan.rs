use std::f32::consts::FRAC_PI_2;

use crate::beat::BeatLength;
use crate::context::BlockTiming;
use crate::dsp::smoothing::Smoothed;
use crate::effect::EffectKind;
use crate::unit::{Controls, Frame, Kernel, MixLaw, Unit, engaged, implement_effect};

const THROW_SECONDS: f32 = 0.004;
const WIDTH_SMOOTHING_SECONDS: f32 = 0.02;
const FULL_MIX_DEPTH: f32 = 0.25;
const NARROWEST_WIDTH: f32 = 0.5;
const DEFAULT_DEPTH: f32 = 0.5;

/// An auto pan that throws the music to the left and to the right in turn, one beat length on
/// each side, in time with the beat grid: the first beat length of the bar plays on the left.
///
/// Both channels are summed and placed with an equal-power law, so the music keeps its
/// loudness wherever it sits, and each throw takes a few milliseconds, sharp without clicking.
///
/// The depth knob balances the original and the panned sound over its first quarter, then
/// widens the throw from half way to hard left and right.
pub struct Pan {
    unit: Unit<PanKernel>,
}

impl Pan {
    /// Builds a pan on 1/2 beat, with the depth knob half way, switched off.
    pub fn new() -> Self {
        let mut pan = Self {
            unit: Unit::new(PanKernel::new(), 1.0),
        };
        pan.apply_depth(DEFAULT_DEPTH);
        pan.unit.kernel.width.snap_to_target();
        pan
    }
}

impl Default for Pan {
    fn default() -> Self {
        Self::new()
    }
}

implement_effect!(Pan);

impl Controls for Pan {
    fn effect_kind(&self) -> EffectKind {
        EffectKind::Pan
    }

    fn apply_depth(&mut self, depth: f32) {
        self.unit.set_mix(engaged(depth, FULL_MIX_DEPTH));
        let widening = ((depth - FULL_MIX_DEPTH) / (1.0 - FULL_MIX_DEPTH)).clamp(0.0, 1.0);
        self.unit
            .kernel
            .width
            .set_target(NARROWEST_WIDTH + (1.0 - NARROWEST_WIDTH) * widening);
    }

    fn apply_beat(&mut self, beat: BeatLength) {
        self.unit.kernel.beat = beat;
    }

    fn current_beat(&self) -> Option<BeatLength> {
        Some(self.unit.kernel.beat)
    }
}

pub(crate) struct PanKernel {
    beat: BeatLength,
    width: Smoothed,
    side: f32,
    throw_step: f32,
}

impl PanKernel {
    fn new() -> Self {
        Self {
            beat: BeatLength::HALF,
            width: Smoothed::new(1.0, WIDTH_SMOOTHING_SECONDS),
            side: -1.0,
            throw_step: 1.0,
        }
    }
}

impl Kernel for PanKernel {
    const TRAILS: bool = false;
    const MIX_LAW: MixLaw = MixLaw::Crossfade;

    fn begin_block(&mut self, timing: &BlockTiming) {
        self.width.prepare(timing.sample_rate);
        self.throw_step = 2.0 / (THROW_SECONDS * timing.sample_rate);
    }

    #[inline]
    fn tick(&mut self, input: Frame, beat_position: f64) -> Frame {
        let step = (beat_position / self.beat.beats()).floor() as i64;
        let wanted = if step.rem_euclid(2) == 0 { -1.0 } else { 1.0 };
        self.side = if wanted > self.side {
            (self.side + self.throw_step).min(wanted)
        } else {
            (self.side - self.throw_step).max(wanted)
        };
        let position = self.side * self.width.next();
        let angle = (position + 1.0) * 0.5 * FRAC_PI_2;
        let centre = (input[0] + input[1]) * std::f32::consts::FRAC_1_SQRT_2;
        [centre * angle.cos(), centre * angle.sin()]
    }

    fn reset(&mut self) {
        self.side = -1.0;
        self.width.snap_to_target();
    }
}
