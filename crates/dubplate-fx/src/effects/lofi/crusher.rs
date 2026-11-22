use crate::context::BlockTiming;
use crate::dsp::crush::SampleCrush;
use crate::dsp::smoothing::Smoothed;
use crate::effect::EffectKind;
use crate::unit::{Controls, Frame, Kernel, MixLaw, Unit, implement_effect, knob};

const MIN_BITS: f32 = 1.0;
const MAX_BITS: f32 = 16.0;
const MIN_RATE_HZ: f32 = 400.0;
const MAX_RATE_HZ: f32 = 48_000.0;
const CONTROL_SMOOTHING_SECONDS: f32 = 0.02;
const DEFAULT_BITS: f32 = 8.0;
const DEFAULT_RATE_HZ: f32 = 11_025.0;
const CRUSHED_BITS: f32 = 3.0;
const CRUSHED_RATE_HZ: f32 = 1_500.0;

/// A bit crusher with sample-rate reduction, for lo-fi grit.
///
/// It holds each sample for as long as the reduced rate asks and rounds it to fewer levels.
/// Both controls move continuously, so sweeping them is smooth rather than stepped.
///
/// The depth knob crushes from 16 bits at 48 kHz (clean) down to 3 bits at 1.5 kHz, along a
/// curve that does most of its work early: a quarter of the way it is already at 9.5 bits and
/// 8.5 kHz, a clear lo-fi grit.
pub struct Crusher {
    unit: Unit<CrusherKernel>,
}

impl Crusher {
    /// Builds a crusher at 8 bits and 11 kHz, switched off.
    pub fn new() -> Self {
        Self {
            unit: Unit::new(CrusherKernel::new(), 1.0),
        }
    }

    /// Sets the resolution, from 1 to 16 bits. Fractions of a bit are allowed.
    pub fn set_bits(&mut self, bits: f32) {
        let current = &mut self.unit.kernel.bits;
        current.set_target(knob(bits, MIN_BITS, MAX_BITS, current.target()));
    }

    /// Sets the reduced sample rate, from 400 Hz to 48 kHz.
    pub fn set_rate_hz(&mut self, hertz: f32) {
        let current = &mut self.unit.kernel.rate_octaves;
        let hertz = knob(hertz, MIN_RATE_HZ, MAX_RATE_HZ, current.target().exp2());
        current.set_target(hertz.log2());
    }
}

impl Default for Crusher {
    fn default() -> Self {
        Self::new()
    }
}

implement_effect!(Crusher);

impl Controls for Crusher {
    fn effect_kind(&self) -> EffectKind {
        EffectKind::Crusher
    }

    fn apply_depth(&mut self, depth: f32) {
        let amount = depth.sqrt();
        self.set_bits(MAX_BITS + (CRUSHED_BITS - MAX_BITS) * amount);
        let octaves = MAX_RATE_HZ.log2() + (CRUSHED_RATE_HZ.log2() - MAX_RATE_HZ.log2()) * amount;
        self.set_rate_hz(octaves.exp2());
    }
}

pub(crate) struct CrusherKernel {
    bits: Smoothed,
    rate_octaves: Smoothed,
    crush: SampleCrush,
    sample_rate: f32,
}

impl CrusherKernel {
    fn new() -> Self {
        Self {
            bits: Smoothed::new(DEFAULT_BITS, CONTROL_SMOOTHING_SECONDS),
            rate_octaves: Smoothed::new(DEFAULT_RATE_HZ.log2(), CONTROL_SMOOTHING_SECONDS),
            crush: SampleCrush::default(),
            sample_rate: 48_000.0,
        }
    }
}

impl Kernel for CrusherKernel {
    const TRAILS: bool = false;
    const MIX_LAW: MixLaw = MixLaw::Crossfade;

    fn begin_block(&mut self, timing: &BlockTiming) {
        self.sample_rate = timing.sample_rate;
        self.bits.prepare(timing.sample_rate);
        self.rate_octaves.prepare(timing.sample_rate);
    }

    #[inline]
    fn tick(&mut self, input: Frame, _beat_position: f64) -> Frame {
        let levels = (self.bits.next() - 1.0).exp2();
        let rate = self.rate_octaves.next().exp2();
        self.crush.tick(input, levels, rate / self.sample_rate)
    }

    fn reset(&mut self) {
        self.crush = SampleCrush::default();
        self.bits.snap_to_target();
        self.rate_octaves.snap_to_target();
    }
}
