use crate::config::EffectConfig;
use crate::context::BlockTiming;
use crate::dsp::pitch_shift::PitchShifter;
use crate::dsp::smoothing::Smoothed;
use crate::effect::EffectKind;
use crate::parameter::ParameterValue;
use crate::unit::{Controls, Frame, Kernel, MixLaw, Unit, engaged, implement_effect};

const WINDOW_SECONDS: f32 = 0.05;
const PITCH_SMOOTHING_SECONDS: f32 = 0.05;
const FULL_MIX_DEPTH: f32 = 0.05;
const DEFAULT_RANGE_SEMITONES: i8 = 12;
const MOST_SEMITONES: i8 = 12;

/// Shifts the music's pitch without changing its tempo, like the Pitch beat effect of a DJ
/// mixer.
///
/// The parameter is the pitch the knob reaches fully right, from an octave down to an octave
/// up (+12 semitones by default); the depth knob glides the pitch there from the original,
/// so a turn of the knob is a riser and fully left gives the original back.
pub(crate) struct Pitch {
    unit: Unit<PitchKernel>,
}

impl Pitch {
    pub(crate) fn new(config: &EffectConfig) -> Self {
        Self {
            unit: Unit::new(PitchKernel::new(config), 0.0),
        }
    }
}

implement_effect!(Pitch);

impl Controls for Pitch {
    fn effect_kind(&self) -> EffectKind {
        EffectKind::Pitch
    }

    fn apply_depth(&mut self, depth: f32) {
        self.unit.set_mix(engaged(depth, FULL_MIX_DEPTH));
        let kernel = &mut self.unit.kernel;
        kernel.depth = depth;
        kernel.retarget();
    }

    fn apply_parameter(&mut self, value: ParameterValue) {
        if let ParameterValue::Semitones(semitones) = value {
            let kernel = &mut self.unit.kernel;
            kernel.range_semitones = semitones.clamp(-MOST_SEMITONES, MOST_SEMITONES);
            kernel.retarget();
        }
    }

    fn current_parameter(&self) -> Option<ParameterValue> {
        Some(ParameterValue::Semitones(self.unit.kernel.range_semitones))
    }
}

pub(crate) struct PitchKernel {
    shifter: PitchShifter,
    range_semitones: i8,
    depth: f32,
    semitones: Smoothed,
}

impl PitchKernel {
    fn new(config: &EffectConfig) -> Self {
        Self {
            shifter: PitchShifter::new(config.samples_for(WINDOW_SECONDS)),
            range_semitones: DEFAULT_RANGE_SEMITONES,
            depth: 0.0,
            semitones: Smoothed::new(0.0, PITCH_SMOOTHING_SECONDS),
        }
    }

    fn retarget(&mut self) {
        self.semitones
            .set_target(f32::from(self.range_semitones) * self.depth);
    }
}

impl Kernel for PitchKernel {
    const TRAILS: bool = false;
    const MIX_LAW: MixLaw = MixLaw::Crossfade;

    fn begin_block(&mut self, timing: &BlockTiming) {
        self.semitones.prepare(timing.sample_rate);
        self.shifter
            .set_window_samples(WINDOW_SECONDS * timing.sample_rate);
    }

    #[inline]
    fn tick(&mut self, input: Frame, _beat_position: f64) -> Frame {
        let ratio = (self.semitones.next() / 12.0).exp2();
        self.shifter.tick(input, ratio)
    }

    fn reset(&mut self) {
        self.shifter.clear();
        self.semitones.snap_to_target();
    }
}
