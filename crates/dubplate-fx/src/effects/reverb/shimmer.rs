use crate::config::EffectConfig;
use crate::context::BlockTiming;
use crate::dsp::filters::{OnePole, one_pole_gain};
use crate::dsp::pitch_shift::PitchShifter;
use crate::dsp::shaping::flush_denormal;
use crate::dsp::smoothing::Smoothed;
use crate::effect::EffectKind;
use crate::effects::reverb::{DEFAULT_SIZE_PERCENT, ReverbKernel};
use crate::parameter::ParameterValue;
use crate::unit::{Controls, Frame, Kernel, MixLaw, Unit, implement_effect};

const SHIFT_SEMITONES: f32 = 12.0;
const WINDOW_SECONDS: f32 = 0.06;
const LOWEST_FEEDBACK: f32 = 0.25;
const HIGHEST_FEEDBACK: f32 = 0.6;
const RISING_TONE_HZ: f32 = 400.0;
const FALLING_TONE_HZ: f32 = 2_500.0;
const FEEDBACK_SMOOTHING_SECONDS: f32 = 0.05;
const DEFAULT_DEPTH: f32 = 0.5;

/// A reverb whose tail climbs, or falls, an octave at a time: Reverb Up and Reverb Down.
///
/// The tail is shifted an octave and fed back into the reverb, so each pass of the space comes
/// back higher (or lower) than the last, thinned by a high-pass on the way up and darkened by a
/// low-pass on the way down. The parameter is the size of the space, as for the Reverb; the
/// depth knob balances the music and the reverb and feeds more of the shifted tail back.
pub(crate) struct ShimmerReverb {
    unit: Unit<ShimmerKernel>,
    size_percent: i16,
}

impl ShimmerReverb {
    pub(crate) fn new(config: &EffectConfig, rising: bool) -> Self {
        let mut reverb = Self {
            unit: Unit::new(ShimmerKernel::new(config, rising), DEFAULT_DEPTH),
            size_percent: DEFAULT_SIZE_PERCENT,
        };
        reverb.apply_parameter(ParameterValue::Percent(DEFAULT_SIZE_PERCENT));
        reverb.apply_depth(DEFAULT_DEPTH);
        reverb.unit.reset();
        reverb
    }
}

implement_effect!(ShimmerReverb);

impl Controls for ShimmerReverb {
    fn effect_kind(&self) -> EffectKind {
        if self.unit.kernel.rising {
            EffectKind::ReverbUp
        } else {
            EffectKind::ReverbDown
        }
    }

    fn apply_depth(&mut self, depth: f32) {
        self.unit.set_mix(depth);
        self.unit
            .kernel
            .feedback
            .set_target(LOWEST_FEEDBACK + (HIGHEST_FEEDBACK - LOWEST_FEEDBACK) * depth);
    }

    fn apply_parameter(&mut self, value: ParameterValue) {
        if let ParameterValue::Percent(percent) = value {
            self.size_percent = percent.clamp(0, 100);
            self.unit.kernel.reverb.set_space(self.size_percent);
        }
    }

    fn current_parameter(&self) -> Option<ParameterValue> {
        Some(ParameterValue::Percent(self.size_percent))
    }
}

pub(crate) struct ShimmerKernel {
    reverb: ReverbKernel,
    shifter: PitchShifter,
    tone: [OnePole; 2],
    tone_gain: f32,
    rising: bool,
    feedback: Smoothed,
    returning: Frame,
}

impl ShimmerKernel {
    fn new(config: &EffectConfig, rising: bool) -> Self {
        Self {
            reverb: ReverbKernel::new(config),
            shifter: PitchShifter::new(config.samples_for(WINDOW_SECONDS)),
            tone: [OnePole::default(); 2],
            tone_gain: 0.0,
            rising,
            feedback: Smoothed::new(LOWEST_FEEDBACK, FEEDBACK_SMOOTHING_SECONDS),
            returning: [0.0; 2],
        }
    }
}

impl Kernel for ShimmerKernel {
    const TRAILS: bool = true;
    const MIX_LAW: MixLaw = MixLaw::Blend;

    fn begin_block(&mut self, timing: &BlockTiming) {
        self.reverb.begin_block(timing);
        self.feedback.prepare(timing.sample_rate);
        self.shifter
            .set_window_samples(WINDOW_SECONDS * timing.sample_rate);
        let tone_hz = if self.rising {
            RISING_TONE_HZ
        } else {
            FALLING_TONE_HZ
        };
        self.tone_gain = one_pole_gain(tone_hz, timing.sample_rate);
    }

    #[inline]
    fn tick(&mut self, input: Frame, beat_position: f64) -> Frame {
        let semitones = if self.rising {
            SHIFT_SEMITONES
        } else {
            -SHIFT_SEMITONES
        };
        let shifted = self.shifter.tick(self.returning, (semitones / 12.0).exp2());
        let feedback = self.feedback.next();
        let [left_tone, right_tone] = &mut self.tone;
        let shaped = if self.rising {
            [
                left_tone.high_pass(shifted[0], self.tone_gain),
                right_tone.high_pass(shifted[1], self.tone_gain),
            ]
        } else {
            [
                left_tone.low_pass(shifted[0], self.tone_gain),
                right_tone.low_pass(shifted[1], self.tone_gain),
            ]
        };
        let fed = [
            input[0] + flush_denormal(shaped[0] * feedback),
            input[1] + flush_denormal(shaped[1] * feedback),
        ];
        let wet = self.reverb.tick(fed, beat_position);
        self.returning = wet;
        wet
    }

    fn reset(&mut self) {
        self.reverb.reset();
        self.shifter.clear();
        for tone in &mut self.tone {
            tone.reset();
        }
        self.returning = [0.0; 2];
        self.feedback.snap_to_target();
    }

    fn tail_seconds(&self) -> f32 {
        self.reverb.tail_seconds()
    }
}
