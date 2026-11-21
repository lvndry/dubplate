use crate::beat::BeatLength;
use crate::config::EffectConfig;
use crate::context::BlockTiming;
use crate::dsp::delay_line::{DelayLine, MIN_DELAY_SAMPLES};
use crate::dsp::delay_time::DelayTime;
use crate::dsp::shaping::flush_denormal;
use crate::dsp::smoothing::Smoothed;
use crate::effect::EffectKind;
use crate::effects::tape::{RepeatTone, RepeatToneGains, read_taps};
use crate::unit::{Controls, Frame, Kernel, MixLaw, Unit, implement_effect, knob};

const FEEDBACK_SMOOTHING_SECONDS: f32 = 0.02;
const TRAIL_MARGIN_SECONDS: f32 = 0.05;
const DEFAULT_FEEDBACK: f32 = 0.55;
const DEFAULT_MIX: f32 = 0.5;

/// A tempo-synced echo whose repeats bounce from left to right.
///
/// The first repeat lands on the left one beat length after the sound, the next on the right
/// one beat length later, and so on, each a little quieter. Like [`crate::Echo`], it follows
/// the tempo live, ages its repeats like tape and rings out when switched off.
///
/// The depth knob raises the echo level up to the dry level and the feedback from 0.35 to 0.85.
pub struct PingPong {
    unit: Unit<PingPongKernel>,
}

impl PingPong {
    /// Builds a ping-pong delay at 1/2 beat, feedback 0.55 and mix 0.5, switched off. This
    /// allocates.
    pub fn new(config: &EffectConfig) -> Self {
        Self {
            unit: Unit::new(PingPongKernel::new(config), DEFAULT_MIX),
        }
    }

    /// Sets how much of each bounce comes back, from 0 to 1.
    pub fn set_feedback(&mut self, feedback: f32) {
        let kernel = &mut self.unit.kernel;
        kernel
            .feedback
            .set_target(knob(feedback, 0.0, 1.0, kernel.feedback.target()));
    }

    /// The feedback the delay is moving to.
    pub fn feedback(&self) -> f32 {
        self.unit.kernel.feedback.target()
    }
}

implement_effect!(PingPong);

impl Controls for PingPong {
    fn effect_kind(&self) -> EffectKind {
        EffectKind::PingPong
    }

    fn apply_depth(&mut self, depth: f32) {
        self.unit.set_mix(0.5 * depth);
        self.set_feedback(0.35 + 0.5 * depth);
    }

    fn apply_beat(&mut self, beat: BeatLength) {
        self.unit.kernel.beat = beat;
    }

    fn current_beat(&self) -> Option<BeatLength> {
        Some(self.unit.kernel.beat)
    }
}

pub(crate) struct PingPongKernel {
    left_line: DelayLine,
    right_line: DelayLine,
    tone: RepeatTone,
    tone_gains: RepeatToneGains,
    time: DelayTime,
    beat: BeatLength,
    feedback: Smoothed,
    sample_rate: f32,
}

impl PingPongKernel {
    fn new(config: &EffectConfig) -> Self {
        let capacity = config.max_delay_samples();
        Self {
            left_line: DelayLine::new(capacity),
            right_line: DelayLine::new(capacity),
            tone: RepeatTone::default(),
            tone_gains: RepeatToneGains::default(),
            time: DelayTime::new(),
            beat: BeatLength::HALF,
            feedback: Smoothed::new(DEFAULT_FEEDBACK, FEEDBACK_SMOOTHING_SECONDS),
            sample_rate: 0.0,
        }
    }
}

impl Kernel for PingPongKernel {
    const TRAILS: bool = true;
    const MIX_LAW: MixLaw = MixLaw::Blend;

    fn begin_block(&mut self, timing: &BlockTiming) {
        if timing.sample_rate != self.sample_rate {
            self.sample_rate = timing.sample_rate;
            self.feedback.prepare(timing.sample_rate);
            self.tone_gains = RepeatToneGains::new(timing.sample_rate);
        }
        let samples = timing.beats_to_samples(self.beat.beats()) as f32;
        let longest = self.left_line.max_delay();
        self.time.set_target(
            samples.clamp(MIN_DELAY_SAMPLES, longest),
            timing.sample_rate,
        );
    }

    #[inline]
    fn tick(&mut self, input: Frame, _beat_position: f64) -> Frame {
        let taps = self.time.next();
        let feedback = self.feedback.next();
        let delayed_left = read_taps(&self.left_line, &taps);
        let delayed_right = read_taps(&self.right_line, &taps);
        let centre = 0.5 * (input[0] + input[1]);
        let returning = self.tone.shape(delayed_right, &self.tone_gains) * feedback;
        self.left_line.write(flush_denormal(centre + returning));
        self.right_line
            .write(flush_denormal(delayed_left * feedback));
        [delayed_left, delayed_right]
    }

    fn reset(&mut self) {
        self.left_line.clear();
        self.right_line.clear();
        self.tone.reset();
        self.feedback.snap_to_target();
    }

    fn tail_seconds(&self) -> f32 {
        2.0 * self.time.longest() / self.sample_rate.max(1.0) + TRAIL_MARGIN_SECONDS
    }
}
