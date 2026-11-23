use crate::config::EffectConfig;
use crate::context::BlockTiming;
use crate::dsp::delay_line::{DelayLine, MIN_DELAY_SAMPLES};
use crate::dsp::shaping::{flush_denormal, saturate};
use crate::dsp::shepard::{EndlessGlide, SHEPARD_VOICES};
use crate::dsp::smoothing::Smoothed;
use crate::effect::EffectKind;
use crate::parameter::{ParameterValue, SignedBeats};
use crate::unit::{Controls, Frame, Kernel, MixLaw, Unit, engaged, implement_effect};

const SHORTEST_DELAY_SECONDS: f32 = 0.000_25;
const LONGEST_DELAY_SECONDS: f32 = 0.008;
const FULL_MIX_DEPTH: f32 = 0.5;
const LOWEST_FEEDBACK: f32 = 0.3;
const HIGHEST_FEEDBACK: f32 = 0.85;
const FEEDBACK_SMOOTHING_SECONDS: f32 = 0.02;

/// A jet flanger that rises or falls forever: the Enigma Jet of a DJ mixer.
///
/// Several flanger voices sweep through the same delay one after another, each fading in at
/// the long end and out at the short end, so the jet seems to climb without end. One beat
/// length carries the sweep through one voice; a negative beat length falls instead.
///
/// The depth knob blends the jet in over its first half and raises its feedback across its
/// whole travel.
pub struct EnigmaJet {
    unit: Unit<EnigmaJetKernel>,
}

impl EnigmaJet {
    /// Builds a rising jet over 4 beats, switched off. This allocates.
    pub fn new(config: &EffectConfig) -> Self {
        Self {
            unit: Unit::new(EnigmaJetKernel::new(config), 1.0),
        }
    }
}

implement_effect!(EnigmaJet);

impl Controls for EnigmaJet {
    fn effect_kind(&self) -> EffectKind {
        EffectKind::EnigmaJet
    }

    fn apply_depth(&mut self, depth: f32) {
        self.unit.set_mix(engaged(depth, FULL_MIX_DEPTH));
        self.unit
            .kernel
            .feedback
            .set_target(LOWEST_FEEDBACK + (HIGHEST_FEEDBACK - LOWEST_FEEDBACK) * depth);
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

pub(crate) struct EnigmaJetKernel {
    lines: [DelayLine; 2],
    glide: EndlessGlide,
    cycle: SignedBeats,
    feedback: Smoothed,
    beats_per_sample: f64,
    shortest_delay: f32,
    delay_ratio: f32,
}

impl EnigmaJetKernel {
    fn new(config: &EffectConfig) -> Self {
        let capacity = config.samples_for(LONGEST_DELAY_SECONDS) + 4;
        Self {
            lines: [DelayLine::new(capacity), DelayLine::new(capacity)],
            glide: EndlessGlide::default(),
            cycle: SignedBeats::rising(crate::beat::BeatLength::FOUR),
            feedback: Smoothed::new(LOWEST_FEEDBACK, FEEDBACK_SMOOTHING_SECONDS),
            beats_per_sample: 0.0,
            shortest_delay: MIN_DELAY_SAMPLES,
            delay_ratio: 1.0,
        }
    }
}

impl Kernel for EnigmaJetKernel {
    const TRAILS: bool = false;
    const MIX_LAW: MixLaw = MixLaw::Crossfade;

    fn begin_block(&mut self, timing: &BlockTiming) {
        self.feedback.prepare(timing.sample_rate);
        self.beats_per_sample = timing.beats_per_sample;
        let longest = (LONGEST_DELAY_SECONDS * timing.sample_rate).min(self.lines[0].max_delay());
        self.shortest_delay = (SHORTEST_DELAY_SECONDS * timing.sample_rate).max(MIN_DELAY_SAMPLES);
        self.delay_ratio = longest / self.shortest_delay;
    }

    #[inline]
    fn tick(&mut self, input: Frame, _beat_position: f64) -> Frame {
        let voices_per_beat = 1.0 / self.cycle.length.beats();
        let direction = if self.cycle.falling { -1.0 } else { 1.0 };
        self.glide
            .advance(direction * voices_per_beat * self.beats_per_sample);
        let voices = self.glide.voices();
        let feedback = self.feedback.next();
        let normalise = 2.0 / SHEPARD_VOICES as f32;
        let mut wet = [0.0; 2];
        for (line, (channel_input, channel_wet)) in
            self.lines.iter_mut().zip(input.iter().zip(&mut wet))
        {
            let mut swept = 0.0;
            for voice in voices {
                let delay = self.shortest_delay * self.delay_ratio.powf(1.0 - voice.height);
                swept += line.read_linear(delay) * voice.level;
            }
            swept *= normalise;
            line.write(flush_denormal(channel_input + feedback * saturate(swept)));
            *channel_wet = (channel_input + swept) / (1.0 + feedback);
        }
        wet
    }

    fn reset(&mut self) {
        for line in &mut self.lines {
            line.clear();
        }
        self.glide.reset();
        self.feedback.snap_to_target();
    }
}
