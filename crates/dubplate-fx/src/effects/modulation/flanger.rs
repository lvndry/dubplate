use std::f64::consts::TAU;

use crate::beat::BeatLength;
use crate::config::EffectConfig;
use crate::context::BlockTiming;
use crate::dsp::delay_line::{DelayLine, MIN_DELAY_SAMPLES};
use crate::dsp::lfo::SyncedPhase;
use crate::dsp::shaping::{flush_denormal, saturate};
use crate::dsp::smoothing::Smoothed;
use crate::effect::EffectKind;
use crate::unit::{Controls, Frame, Kernel, MixLaw, Unit, engaged, implement_effect, knob};

const SHORTEST_DELAY_SECONDS: f32 = 0.000_2;
const LONGEST_DELAY_SECONDS: f32 = 0.006;
const STEREO_OFFSET: f64 = 0.25;
const MAX_FEEDBACK: f32 = 0.95;
const FEEDBACK_SMOOTHING_SECONDS: f32 = 0.02;
const DEFAULT_FEEDBACK: f32 = 0.6;
const LOWEST_DEPTH_FEEDBACK: f32 = 0.4;
const FULL_MIX_DEPTH: f32 = 0.5;

/// A jet flanger whose sweep is locked to the beat grid.
///
/// One sweep down and back takes the beat length (1/16 to 32 beats, 4 by default) and lines up
/// with the bar. The sweep is exponential, so the jet moves evenly through the whole range, and
/// the right channel runs a quarter cycle apart for width that still sums well to mono.
///
/// The depth knob blends the flanger in over its first half and raises its feedback from 0.4
/// to 0.95 across its whole travel.
pub struct Flanger {
    unit: Unit<FlangerKernel>,
}

impl Flanger {
    /// Builds a flanger with a 4-beat sweep and feedback 0.6, switched off. This allocates.
    pub fn new(config: &EffectConfig) -> Self {
        Self {
            unit: Unit::new(FlangerKernel::new(config), 1.0),
        }
    }

    /// Sets the feedback, from -0.95 (hollow) through 0 to 0.95 (a metallic jet).
    pub fn set_feedback(&mut self, feedback: f32) {
        let current = &mut self.unit.kernel.feedback;
        current.set_target(knob(
            feedback,
            -MAX_FEEDBACK,
            MAX_FEEDBACK,
            current.target(),
        ));
    }
}

implement_effect!(Flanger);

impl Controls for Flanger {
    fn effect_kind(&self) -> EffectKind {
        EffectKind::Flanger
    }

    fn apply_depth(&mut self, depth: f32) {
        self.unit.set_mix(engaged(depth, FULL_MIX_DEPTH));
        self.set_feedback(LOWEST_DEPTH_FEEDBACK + (MAX_FEEDBACK - LOWEST_DEPTH_FEEDBACK) * depth);
    }

    fn apply_beat(&mut self, beat: BeatLength) {
        self.unit.kernel.beat = beat;
    }

    fn current_beat(&self) -> Option<BeatLength> {
        Some(self.unit.kernel.beat)
    }
}

pub(crate) struct FlangerKernel {
    lines: [DelayLine; 2],
    phase: SyncedPhase,
    beat: BeatLength,
    feedback: Smoothed,
    beats_per_sample: f64,
    shortest_delay: f32,
    delay_ratio: f32,
}

impl FlangerKernel {
    fn new(config: &EffectConfig) -> Self {
        let capacity = config.samples_for(LONGEST_DELAY_SECONDS) + 4;
        Self {
            lines: [DelayLine::new(capacity), DelayLine::new(capacity)],
            phase: SyncedPhase::new(),
            beat: BeatLength::FOUR,
            feedback: Smoothed::new(DEFAULT_FEEDBACK, FEEDBACK_SMOOTHING_SECONDS),
            beats_per_sample: 0.0,
            shortest_delay: MIN_DELAY_SAMPLES,
            delay_ratio: 1.0,
        }
    }
}

impl Kernel for FlangerKernel {
    const TRAILS: bool = false;
    const MIX_LAW: MixLaw = MixLaw::Crossfade;

    fn begin_block(&mut self, timing: &BlockTiming) {
        self.phase.prepare(timing.sample_rate);
        self.feedback.prepare(timing.sample_rate);
        self.beats_per_sample = timing.beats_per_sample;
        let longest = (LONGEST_DELAY_SECONDS * timing.sample_rate).min(self.lines[0].max_delay());
        self.shortest_delay = (SHORTEST_DELAY_SECONDS * timing.sample_rate).max(MIN_DELAY_SAMPLES);
        self.delay_ratio = longest / self.shortest_delay;
    }

    #[inline]
    fn tick(&mut self, input: Frame, beat_position: f64) -> Frame {
        let phase = self
            .phase
            .next(beat_position, self.beat.beats(), self.beats_per_sample);
        let feedback = self.feedback.next();
        let compensation = 1.0 / (1.0 + feedback.abs());
        let mut wet = [0.0; 2];
        for (channel_index, (line, channel_wet)) in self.lines.iter_mut().zip(&mut wet).enumerate()
        {
            let channel_phase = phase + STEREO_OFFSET * channel_index as f64;
            let sweep = (0.5 - 0.5 * (TAU * channel_phase).cos()) as f32;
            let delay = self.shortest_delay * self.delay_ratio.powf(sweep);
            let delayed = line.read_cubic(delay);
            let dry = input[channel_index];
            line.write(flush_denormal(dry + feedback * saturate(delayed)));
            *channel_wet = (dry + delayed) * compensation;
        }
        wet
    }

    fn reset(&mut self) {
        for line in &mut self.lines {
            line.clear();
        }
        self.phase.unlock();
        self.feedback.snap_to_target();
    }
}
