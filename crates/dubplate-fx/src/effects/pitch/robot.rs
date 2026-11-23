use std::f32::consts::TAU;

use crate::config::EffectConfig;
use crate::context::BlockTiming;
use crate::dsp::delay_line::DelayLine;
use crate::dsp::shaping::{flush_denormal, saturate};
use crate::dsp::smoothing::Smoothed;
use crate::effect::EffectKind;
use crate::parameter::ParameterValue;
use crate::unit::{Controls, Frame, Kernel, MixLaw, Unit, engaged, implement_effect};

const CENTRE_HZ: f32 = 150.0;
const OCTAVES_AT_FULL_TONE: f32 = 1.5;
const LOWEST_HZ: f32 = 40.0;
const RESONANCE: f32 = 0.85;
const RING_SHARE: f32 = 0.5;
const PITCH_SMOOTHING_SECONDS: f32 = 0.03;
const FULL_MIX_DEPTH: f32 = 0.5;
const DEFAULT_TONE_PERCENT: i16 = 0;
const MAX_TONE_PERCENT: i16 = 100;

/// Turns the music into a robot's voice: a metallic resonance tuned to one pitch, roughened by
/// ring modulation at the same pitch.
///
/// The tone sets the pitch: 0% sits at 150 Hz, -100% an octave and a half lower and 100% an
/// octave and a half higher. The depth knob balances the original and the robot over its
/// first half and is all robot from there.
pub struct Robot {
    unit: Unit<RobotKernel>,
}

impl Robot {
    /// Builds a robot at a 0% tone, switched off. This allocates.
    pub fn new(config: &EffectConfig) -> Self {
        Self {
            unit: Unit::new(RobotKernel::new(config), 1.0),
        }
    }
}

implement_effect!(Robot);

impl Controls for Robot {
    fn effect_kind(&self) -> EffectKind {
        EffectKind::Robot
    }

    fn apply_depth(&mut self, depth: f32) {
        self.unit.set_mix(engaged(depth, FULL_MIX_DEPTH));
    }

    fn apply_parameter(&mut self, value: ParameterValue) {
        if let ParameterValue::Percent(percent) = value {
            let percent = percent.clamp(-MAX_TONE_PERCENT, MAX_TONE_PERCENT);
            let kernel = &mut self.unit.kernel;
            kernel.tone_percent = percent;
            let octaves = f32::from(percent) / 100.0 * OCTAVES_AT_FULL_TONE;
            kernel.pitch_octaves.set_target(CENTRE_HZ.log2() + octaves);
        }
    }

    fn current_parameter(&self) -> Option<ParameterValue> {
        Some(ParameterValue::Percent(self.unit.kernel.tone_percent))
    }
}

pub(crate) struct RobotKernel {
    lines: [DelayLine; 2],
    tone_percent: i16,
    pitch_octaves: Smoothed,
    ring_phase: f32,
    sample_rate: f32,
}

impl RobotKernel {
    fn new(config: &EffectConfig) -> Self {
        let capacity = config.samples_for(1.0 / LOWEST_HZ) + 4;
        Self {
            lines: [DelayLine::new(capacity), DelayLine::new(capacity)],
            tone_percent: DEFAULT_TONE_PERCENT,
            pitch_octaves: Smoothed::new(CENTRE_HZ.log2(), PITCH_SMOOTHING_SECONDS),
            ring_phase: 0.0,
            sample_rate: 48_000.0,
        }
    }
}

impl Kernel for RobotKernel {
    const TRAILS: bool = false;
    const MIX_LAW: MixLaw = MixLaw::Crossfade;

    fn begin_block(&mut self, timing: &BlockTiming) {
        self.sample_rate = timing.sample_rate;
        self.pitch_octaves.prepare(timing.sample_rate);
    }

    #[inline]
    fn tick(&mut self, input: Frame, _beat_position: f64) -> Frame {
        let pitch = self.pitch_octaves.next().exp2();
        let period = self.sample_rate / pitch;
        self.ring_phase = (self.ring_phase + pitch / self.sample_rate).fract();
        let ring = (TAU * self.ring_phase).sin();
        let mut output = [0.0; 2];
        for (line, (channel_input, channel_output)) in
            self.lines.iter_mut().zip(input.iter().zip(&mut output))
        {
            let resonance = line.read_linear(period);
            let combed = channel_input + RESONANCE * saturate(resonance);
            line.write(flush_denormal(combed));
            let voiced = combed * (1.0 - RESONANCE);
            *channel_output = voiced * (1.0 - RING_SHARE) + voiced * ring * RING_SHARE * 2.0;
        }
        output
    }

    fn reset(&mut self) {
        for line in &mut self.lines {
            line.clear();
        }
        self.ring_phase = 0.0;
        self.pitch_octaves.snap_to_target();
    }
}
