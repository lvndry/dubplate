use crate::beat::BeatLength;
use crate::config::EffectConfig;
use crate::context::BlockTiming;
use crate::dsp::delay_line::{DelayLine, MIN_DELAY_SAMPLES};
use crate::dsp::delay_time::DelayTime;
use crate::dsp::shaping::flush_denormal;
use crate::dsp::smoothing::Smoothed;
use crate::effect::EffectKind;
use crate::effects::delays::tape::{
    DEFAULT_DEPTH, RepeatTone, RepeatToneGains, TrailLevel, feedback_for_depth, mix_for_depth,
    read_taps,
};
use crate::unit::{Controls, Frame, Kernel, MixLaw, Unit, implement_effect, knob};

const FEEDBACK_SMOOTHING_SECONDS: f32 = 0.02;
const TRAIL_MARGIN_SECONDS: f32 = 0.05;

/// A tempo-synced echo whose repeats bounce from left to right.
///
/// The first repeat lands on the left one beat length after the sound, the next on the right
/// one beat length later, and so on, each a little quieter. Like [`crate::Echo`], it follows
/// the tempo live, ages its repeats like tape and rings out when switched off.
///
/// The depth knob balances the music and the echo, as the LEVEL/DEPTH knob of a DJ mixer does:
/// at a quarter the first repeat plays 12 dB under the music, in the middle both play at full
/// level, and fully right only the echo plays. It also raises the feedback from 0.5 to 0.8, so
/// the trail grows longer as the knob turns. The repeats never build up past the music that
/// feeds them.
pub struct PingPong {
    unit: Unit<PingPongKernel>,
}

impl PingPong {
    /// Builds a ping-pong delay at 1/2 beat with the depth knob at 0.5 (feedback 0.65, mix 0.5),
    /// switched off. This allocates.
    pub fn new(config: &EffectConfig) -> Self {
        Self {
            unit: Unit::new(PingPongKernel::new(config), mix_for_depth(DEFAULT_DEPTH)),
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
        self.unit.set_mix(mix_for_depth(depth));
        self.set_feedback(feedback_for_depth(depth));
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
    tones: [RepeatTone; 2],
    tone_gains: RepeatToneGains,
    trail_level: TrailLevel,
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
            tones: [RepeatTone::default(); 2],
            tone_gains: RepeatToneGains::default(),
            trail_level: TrailLevel::new(),
            time: DelayTime::new(),
            beat: BeatLength::HALF,
            feedback: Smoothed::new(
                feedback_for_depth(DEFAULT_DEPTH),
                FEEDBACK_SMOOTHING_SECONDS,
            ),
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
            self.trail_level.prepare(timing.sample_rate);
        }
        let samples = timing.beats_to_samples(self.beat.beats()) as f32;
        let longest = self.left_line.max_delay();
        self.time.set_target(
            samples.clamp(MIN_DELAY_SAMPLES, longest),
            timing.sample_rate,
        );
        self.trail_level.set_loop_samples(2.0 * self.time.longest());
    }

    #[inline]
    fn tick(&mut self, input: Frame, _beat_position: f64) -> Frame {
        let taps = self.time.next();
        let delayed_left = read_taps(&self.left_line, &taps);
        let delayed_right = read_taps(&self.right_line, &taps);
        let centre = 0.5 * (input[0] + input[1]);
        let gains = self.trail_level.next(
            self.feedback.next(),
            centre.abs(),
            delayed_left.abs().max(delayed_right.abs()),
        );
        let [left_tone, right_tone] = &mut self.tones;
        let returning = left_tone.shape(delayed_right, &self.tone_gains) * gains.feedback;
        let crossing = right_tone.shape(delayed_left, &self.tone_gains) * gains.feedback;
        self.left_line.write(flush_denormal(centre + returning));
        self.right_line.write(flush_denormal(crossing));
        [delayed_left * gains.output, delayed_right * gains.output]
    }

    fn reset(&mut self) {
        self.left_line.clear();
        self.right_line.clear();
        for tone in &mut self.tones {
            tone.reset();
        }
        self.trail_level.reset();
        self.feedback.snap_to_target();
    }

    fn tail_seconds(&self) -> f32 {
        2.0 * self.time.longest() / self.sample_rate.max(1.0) + TRAIL_MARGIN_SECONDS
    }
}
