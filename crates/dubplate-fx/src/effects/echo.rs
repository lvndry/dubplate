use crate::beat::BeatLength;
use crate::config::EffectConfig;
use crate::context::BlockTiming;
use crate::dsp::delay_line::{DelayLine, MIN_DELAY_SAMPLES};
use crate::dsp::delay_time::DelayTime;
use crate::dsp::shaping::flush_denormal;
use crate::dsp::smoothing::Smoothed;
use crate::effect::EffectKind;
use crate::effects::tape::{
    DEFAULT_DEPTH, RepeatTone, RepeatToneGains, TrailLevel, feedback_for_depth, mix_for_depth,
    read_taps,
};
use crate::unit::{Controls, Frame, Kernel, MixLaw, Unit, implement_effect, knob};

const FEEDBACK_SMOOTHING_SECONDS: f32 = 0.02;
const TRAIL_MARGIN_SECONDS: f32 = 0.05;

/// A tempo-synced stereo echo with feedback.
///
/// Each repeat lands on the beat length at the current tempo, and follows tempo changes live.
/// Repeats darken and saturate a little on every pass, like a tape echo. Switched off, the echo
/// stops taking in new audio and lets its repeats ring out.
///
/// The depth knob raises the echo from silence to its full level, which sits under the music,
/// and the feedback from 0.35 to 0.85. Whatever the feedback, the trail sounds about as loud:
/// more feedback makes it last longer, not louder. The repeats never build up past the music
/// that feeds them and are held under the room the music leaves below full scale.
pub struct Echo {
    unit: Unit<EchoKernel>,
}

impl Echo {
    /// Builds an echo at 1/2 beat with the depth knob at 0.6 (feedback 0.65, mix 0.3), switched
    /// off. This allocates.
    pub fn new(config: &EffectConfig) -> Self {
        Self {
            unit: Unit::new(EchoKernel::new(config), mix_for_depth(DEFAULT_DEPTH)),
        }
    }

    /// Sets how much of each repeat comes back, from 0 (a single repeat) to 1 (repeats that
    /// hold at the level of the music while it plays, and fade slowly once it stops).
    pub fn set_feedback(&mut self, feedback: f32) {
        let kernel = &mut self.unit.kernel;
        kernel
            .feedback
            .set_target(knob(feedback, 0.0, 1.0, kernel.feedback.target()));
    }

    /// The feedback the echo is moving to.
    pub fn feedback(&self) -> f32 {
        self.unit.kernel.feedback.target()
    }
}

implement_effect!(Echo);

impl Controls for Echo {
    fn effect_kind(&self) -> EffectKind {
        EffectKind::Echo
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

pub(crate) struct EchoKernel {
    lines: [DelayLine; 2],
    tones: [RepeatTone; 2],
    tone_gains: RepeatToneGains,
    trail_level: TrailLevel,
    time: DelayTime,
    beat: BeatLength,
    feedback: Smoothed,
    sample_rate: f32,
}

impl EchoKernel {
    fn new(config: &EffectConfig) -> Self {
        let capacity = config.max_delay_samples();
        Self {
            lines: [DelayLine::new(capacity), DelayLine::new(capacity)],
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

impl Kernel for EchoKernel {
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
        let longest = self.lines[0].max_delay();
        self.time.set_target(
            samples.clamp(MIN_DELAY_SAMPLES, longest),
            timing.sample_rate,
        );
        self.trail_level.set_loop_samples(self.time.longest());
    }

    #[inline]
    fn tick(&mut self, input: Frame, _beat_position: f64) -> Frame {
        let taps = self.time.next();
        let delayed = [
            read_taps(&self.lines[0], &taps),
            read_taps(&self.lines[1], &taps),
        ];
        let gains = self.trail_level.next(
            self.feedback.next(),
            input[0].abs().max(input[1].abs()),
            delayed[0].abs().max(delayed[1].abs()),
        );
        for (((line, tone), channel_input), channel_delayed) in self
            .lines
            .iter_mut()
            .zip(&mut self.tones)
            .zip(input)
            .zip(delayed)
        {
            let repeat = tone.shape(channel_delayed, &self.tone_gains) * gains.feedback;
            line.write(flush_denormal(channel_input + repeat));
        }
        delayed.map(|channel_delayed| channel_delayed * gains.output)
    }

    fn reset(&mut self) {
        for line in &mut self.lines {
            line.clear();
        }
        for tone in &mut self.tones {
            tone.reset();
        }
        self.trail_level.reset();
        self.feedback.snap_to_target();
    }

    fn tail_seconds(&self) -> f32 {
        self.time.longest() / self.sample_rate.max(1.0) + TRAIL_MARGIN_SECONDS
    }
}
