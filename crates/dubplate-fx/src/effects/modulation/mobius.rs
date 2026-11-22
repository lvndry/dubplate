use crate::context::BlockTiming;
use crate::dsp::shepard::{EndlessGlide, SHEPARD_VOICES};
use crate::effect::EffectKind;
use crate::parameter::{ParameterValue, SignedBeats};
use crate::unit::{Controls, Frame, Kernel, MixLaw, Unit, implement_effect};

const LOWEST_HZ: f32 = 55.0;
const TONE_LEVEL: f32 = 0.5;

/// The waveform of a [`Mobius`] tone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Waveform {
    /// A bright sawtooth.
    Saw,
    /// A soft triangle.
    Triangle,
}

/// A tone that rises or falls forever, laid over the music: the Mobius of a DJ mixer.
///
/// Six voices an octave apart glide together, each fading in at the bottom and out at the top,
/// so the ear hears one pitch climbing, or falling, without end. Each beat length the tone
/// moves one octave; a negative beat length falls. It sounds even when the music is silent.
///
/// The depth knob sets how loud the tone plays; the music keeps its own level.
pub struct Mobius {
    unit: Unit<MobiusKernel>,
}

impl Mobius {
    pub(crate) fn new(waveform: Waveform) -> Self {
        Self {
            unit: Unit::new(MobiusKernel::new(waveform), 0.0),
        }
    }

    /// Builds a rising sawtooth Mobius over 4 beats, switched off.
    pub fn saw() -> Self {
        Self::new(Waveform::Saw)
    }

    /// Builds a rising triangle Mobius over 4 beats, switched off.
    pub fn triangle() -> Self {
        Self::new(Waveform::Triangle)
    }
}

implement_effect!(Mobius);

impl Controls for Mobius {
    fn effect_kind(&self) -> EffectKind {
        match self.unit.kernel.waveform {
            Waveform::Saw => EffectKind::MobiusSaw,
            Waveform::Triangle => EffectKind::MobiusTriangle,
        }
    }

    fn apply_depth(&mut self, depth: f32) {
        self.unit.set_mix(depth);
    }

    fn apply_parameter(&mut self, value: ParameterValue) {
        if let ParameterValue::SignedBeats(cycle) = value {
            self.unit.kernel.cycle = cycle;
        }
    }

    fn current_parameter(&self) -> Option<ParameterValue> {
        Some(ParameterValue::SignedBeats(self.unit.kernel.cycle))
    }
}

pub(crate) struct MobiusKernel {
    waveform: Waveform,
    glide: EndlessGlide,
    cycle: SignedBeats,
    phases: [f32; SHEPARD_VOICES],
    beats_per_sample: f64,
    sample_rate: f32,
}

impl MobiusKernel {
    fn new(waveform: Waveform) -> Self {
        Self {
            waveform,
            glide: EndlessGlide::default(),
            cycle: SignedBeats::rising(crate::beat::BeatLength::FOUR),
            phases: [0.0; SHEPARD_VOICES],
            beats_per_sample: 0.0,
            sample_rate: 48_000.0,
        }
    }
}

impl Waveform {
    /// One sample of the waveform at `phase`, from 0 to 1, between -1 and 1. The sawtooth is
    /// smoothed at its jump over one sample step so it does not alias harshly.
    #[inline]
    fn sample(self, phase: f32, step: f32) -> f32 {
        match self {
            Waveform::Saw => 2.0 * phase - 1.0 - poly_blep(phase, step),
            Waveform::Triangle => 1.0 - 4.0 * (phase - 0.5).abs(),
        }
    }
}

/// The polynomial correction that rounds off a sawtooth's jump.
#[inline]
fn poly_blep(phase: f32, step: f32) -> f32 {
    if phase < step {
        let position = phase / step;
        2.0 * position - position * position - 1.0
    } else if phase > 1.0 - step {
        let position = (phase - 1.0) / step;
        position * position + 2.0 * position + 1.0
    } else {
        0.0
    }
}

impl Kernel for MobiusKernel {
    const TRAILS: bool = false;
    const MIX_LAW: MixLaw = MixLaw::Layer;

    fn begin_block(&mut self, timing: &BlockTiming) {
        self.beats_per_sample = timing.beats_per_sample;
        self.sample_rate = timing.sample_rate;
    }

    #[inline]
    fn tick(&mut self, _input: Frame, _beat_position: f64) -> Frame {
        let direction = if self.cycle.falling { -1.0 } else { 1.0 };
        self.glide.advance(
            direction * self.beats_per_sample / self.cycle.length.beats() / SHEPARD_VOICES as f64,
        );
        let mut tone = 0.0;
        for (voice, phase) in self.glide.voices().into_iter().zip(&mut self.phases) {
            let frequency = LOWEST_HZ * (voice.height * SHEPARD_VOICES as f32).exp2();
            let step = frequency / self.sample_rate;
            *phase = (*phase + step).fract();
            tone += self.waveform.sample(*phase, step) * voice.level;
        }
        let level = tone * TONE_LEVEL * 2.0 / SHEPARD_VOICES as f32;
        [level, level]
    }

    fn reset(&mut self) {
        self.glide.reset();
        self.phases = [0.0; SHEPARD_VOICES];
    }
}
