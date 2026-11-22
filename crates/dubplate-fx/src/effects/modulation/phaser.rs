use std::f64::consts::TAU;

use crate::beat::BeatLength;
use crate::context::BlockTiming;
use crate::dsp::filters::{OnePole, one_pole_gain};
use crate::dsp::lfo::SyncedPhase;
use crate::dsp::shaping::saturate;
use crate::dsp::smoothing::Smoothed;
use crate::effect::EffectKind;
use crate::unit::{Controls, Frame, Kernel, MixLaw, Unit, engaged, implement_effect, knob};

const STAGE_COUNT: usize = 6;
const LOWEST_HZ: f32 = 160.0;
const HIGHEST_HZ: f32 = 4_800.0;
const STEREO_OFFSET: f64 = 0.25;
const MAX_FEEDBACK: f32 = 0.9;
const FEEDBACK_SMOOTHING_SECONDS: f32 = 0.02;
const DEFAULT_FEEDBACK: f32 = 0.5;
const LOWEST_DEPTH_FEEDBACK: f32 = 0.3;
const HIGHEST_DEPTH_FEEDBACK: f32 = 0.85;
const FULL_MIX_DEPTH: f32 = 0.5;

/// A six-stage phaser whose sweep is locked to the beat grid.
///
/// One sweep up and back takes the beat length (1/16 to 32 beats, 2 by default) and lines up
/// with the bar. Its three notches move exponentially between 160 Hz and 4.8 kHz, and the
/// right channel runs a quarter cycle apart.
///
/// The depth knob blends the phaser in over its first half and raises its feedback from 0.3 to
/// 0.85 across its whole travel.
pub struct Phaser {
    unit: Unit<PhaserKernel>,
}

impl Phaser {
    /// Builds a phaser with a 2-beat sweep and feedback 0.5, switched off.
    pub fn new() -> Self {
        Self {
            unit: Unit::new(PhaserKernel::new(), 1.0),
        }
    }

    /// Sets the feedback, from 0 (gentle) to 0.9 (sharp, vocal notches).
    pub fn set_feedback(&mut self, feedback: f32) {
        let current = &mut self.unit.kernel.feedback;
        current.set_target(knob(feedback, 0.0, MAX_FEEDBACK, current.target()));
    }
}

impl Default for Phaser {
    fn default() -> Self {
        Self::new()
    }
}

implement_effect!(Phaser);

impl Controls for Phaser {
    fn effect_kind(&self) -> EffectKind {
        EffectKind::Phaser
    }

    fn apply_depth(&mut self, depth: f32) {
        self.unit.set_mix(engaged(depth, FULL_MIX_DEPTH));
        self.set_feedback(
            LOWEST_DEPTH_FEEDBACK + (HIGHEST_DEPTH_FEEDBACK - LOWEST_DEPTH_FEEDBACK) * depth,
        );
    }

    fn apply_beat(&mut self, beat: BeatLength) {
        self.unit.kernel.beat = beat;
    }

    fn current_beat(&self) -> Option<BeatLength> {
        Some(self.unit.kernel.beat)
    }
}

pub(crate) struct PhaserKernel {
    stages: [[OnePole; STAGE_COUNT]; 2],
    last_output: [f32; 2],
    phase: SyncedPhase,
    beat: BeatLength,
    feedback: Smoothed,
    beats_per_sample: f64,
    sample_rate: f32,
}

impl PhaserKernel {
    fn new() -> Self {
        Self {
            stages: [[OnePole::default(); STAGE_COUNT]; 2],
            last_output: [0.0; 2],
            phase: SyncedPhase::new(),
            beat: BeatLength::TWO,
            feedback: Smoothed::new(DEFAULT_FEEDBACK, FEEDBACK_SMOOTHING_SECONDS),
            beats_per_sample: 0.0,
            sample_rate: 48_000.0,
        }
    }
}

impl Kernel for PhaserKernel {
    const TRAILS: bool = false;
    const MIX_LAW: MixLaw = MixLaw::Crossfade;

    fn begin_block(&mut self, timing: &BlockTiming) {
        self.phase.prepare(timing.sample_rate);
        self.feedback.prepare(timing.sample_rate);
        self.beats_per_sample = timing.beats_per_sample;
        self.sample_rate = timing.sample_rate;
    }

    #[inline]
    fn tick(&mut self, input: Frame, beat_position: f64) -> Frame {
        let phase = self
            .phase
            .next(beat_position, self.beat.beats(), self.beats_per_sample);
        let feedback = self.feedback.next();
        let mut wet = [0.0; 2];
        for (channel_index, channel_wet) in wet.iter_mut().enumerate() {
            let channel_phase = phase + STEREO_OFFSET * channel_index as f64;
            let sweep = (0.5 - 0.5 * (TAU * channel_phase).cos()) as f32;
            let frequency = LOWEST_HZ * (HIGHEST_HZ / LOWEST_HZ).powf(sweep);
            let gain = one_pole_gain(frequency, self.sample_rate);
            let dry = input[channel_index];
            let mut signal = dry + feedback * saturate(self.last_output[channel_index]);
            for stage in &mut self.stages[channel_index] {
                signal = stage.all_pass(signal, gain);
            }
            self.last_output[channel_index] = signal;
            *channel_wet = 0.5 * (dry + signal);
        }
        wet
    }

    fn reset(&mut self) {
        for stage in self.stages.iter_mut().flatten() {
            stage.reset();
        }
        self.last_output = [0.0; 2];
        self.phase.unlock();
        self.feedback.snap_to_target();
    }
}
