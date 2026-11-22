//! The echo family: one tempo-synced delay loop, voiced differently for each effect.
//!
//! Every echo shares the loop, its smooth time changes, its level control and the way the
//! depth knob balances the music and the echo. What differs is the voicing: what happens to
//! the music on its way into the loop, what happens to each repeat on its way round, how long
//! the trail lasts and what else the depth knob moves.

pub(crate) mod echo;
pub(crate) mod multi_tap;
pub(crate) mod ping_pong;
pub(crate) mod reverse;
pub(crate) mod tape;
pub(crate) mod voicings;

use crate::beat::BeatLength;
use crate::config::EffectConfig;
use crate::context::BlockTiming;
use crate::dsp::delay_line::{DelayLine, MIN_DELAY_SAMPLES};
use crate::dsp::delay_time::DelayTime;
use crate::dsp::shaping::flush_denormal;
use crate::dsp::smoothing::Smoothed;
use crate::effect::EffectKind;
use crate::effects::delays::tape::{DEFAULT_DEPTH, TrailLevel, read_taps};
use crate::unit::{Controls, Frame, Kernel, MixLaw, Unit, implement_effect};

const FEEDBACK_SMOOTHING_SECONDS: f32 = 0.02;
const TRAIL_MARGIN_SECONDS: f32 = 0.05;

/// What makes one echo of the family sound the way it does.
pub(crate) trait Voicing: Send + 'static {
    /// How the depth knob's mix blends the echo in: by default as a balance of the music and
    /// the echo, both at full level in the middle.
    const MIX_LAW: MixLaw = MixLaw::Blend;

    /// Whether a new delay time glides there however far it is, bending the trail's pitch,
    /// instead of crossfading.
    const GLIDES: bool = false;

    /// Which effect this voicing makes.
    fn kind(&self) -> EffectKind;

    /// The feedback the depth knob sets, from 0 to 1.
    fn feedback_for_depth(&self, depth: f32) -> f32;

    /// The mix the depth knob sets: by default the knob is the balance of the music and the
    /// echo, as on a DJ mixer.
    fn mix_for_depth(&self, depth: f32) -> f32 {
        depth
    }

    /// Prepares for a sample rate.
    fn prepare(&mut self, sample_rate: f32);

    /// Follows the depth knob, for voicings it moves.
    fn set_depth(&mut self, _depth: f32) {}

    /// Shapes the music on its way into the loop.
    #[inline]
    fn send(&mut self, input: Frame) -> Frame {
        input
    }

    /// Shapes a repeat on its way back round the loop.
    fn shape(&mut self, repeat: Frame) -> Frame;

    /// Learns how long the loop is, in samples, at the start of each block.
    fn set_loop_samples(&mut self, _samples: f32) {}

    /// Called each time the effect is switched on.
    fn switched_on(&mut self) {}

    /// How frozen the loop is, from 0 to 1: fully frozen, it takes in no new music and holds
    /// what it has forever. Read once per sample, after [`Voicing::send`].
    fn frozen(&self) -> f32 {
        0.0
    }

    /// Clears every internal state.
    fn reset(&mut self);
}

/// The loop every echo of the family runs, with its voicing.
pub(crate) struct EchoLoop<V> {
    pub(crate) voicing: V,
    lines: [DelayLine; 2],
    trail_level: TrailLevel,
    time: DelayTime,
    pub(crate) beat: BeatLength,
    pub(crate) feedback: Smoothed,
    sample_rate: f32,
}

impl<V: Voicing> EchoLoop<V> {
    pub(crate) fn new(config: &EffectConfig, voicing: V, beat: BeatLength) -> Self {
        let capacity = config.max_delay_samples();
        let feedback = voicing.feedback_for_depth(DEFAULT_DEPTH);
        Self {
            voicing,
            lines: [DelayLine::new(capacity), DelayLine::new(capacity)],
            trail_level: TrailLevel::new(),
            time: if V::GLIDES {
                DelayTime::bending()
            } else {
                DelayTime::new()
            },
            beat,
            feedback: Smoothed::new(feedback, FEEDBACK_SMOOTHING_SECONDS),
            sample_rate: 0.0,
        }
    }
}

impl<V: Voicing> Kernel for EchoLoop<V> {
    const TRAILS: bool = true;
    const MIX_LAW: MixLaw = V::MIX_LAW;

    fn begin_block(&mut self, timing: &BlockTiming) {
        if timing.sample_rate != self.sample_rate {
            self.sample_rate = timing.sample_rate;
            self.feedback.prepare(timing.sample_rate);
            self.trail_level.prepare(timing.sample_rate);
            self.voicing.prepare(timing.sample_rate);
        }
        let samples = timing.beats_to_samples(self.beat.beats()) as f32;
        let longest = self.lines[0].max_delay();
        self.time.set_target(
            samples.clamp(MIN_DELAY_SAMPLES, longest),
            timing.sample_rate,
        );
        self.trail_level.set_loop_samples(self.time.longest());
        self.voicing.set_loop_samples(self.time.longest());
    }

    #[inline]
    fn tick(&mut self, input: Frame, _beat_position: f64) -> Frame {
        let taps = self.time.next();
        let delayed = [
            read_taps(&self.lines[0], &taps),
            read_taps(&self.lines[1], &taps),
        ];
        let sent = self.voicing.send(input);
        let mut gains = self.trail_level.next(
            self.feedback.next(),
            sent[0].abs().max(sent[1].abs()),
            delayed[0].abs().max(delayed[1].abs()),
        );
        let frozen = self.voicing.frozen();
        gains.feedback += (1.0 - gains.feedback) * frozen;
        let repeats = self.voicing.shape(delayed);
        for ((line, channel_sent), repeat) in self.lines.iter_mut().zip(sent).zip(repeats) {
            line.write(flush_denormal(channel_sent + repeat * gains.feedback));
        }
        delayed.map(|channel_delayed| channel_delayed * gains.output)
    }

    fn reset(&mut self) {
        for line in &mut self.lines {
            line.clear();
        }
        self.voicing.reset();
        self.trail_level.reset();
        self.feedback.snap_to_target();
    }

    fn settle(&mut self) {
        self.feedback.snap_to_target();
    }

    fn switched_on(&mut self) {
        self.voicing.switched_on();
    }

    fn tail_seconds(&self) -> f32 {
        self.time.longest() / self.sample_rate.max(1.0) + TRAIL_MARGIN_SECONDS
    }
}

/// An echo of the family, as the effect units see it.
pub(crate) struct EchoEffect<V> {
    unit: Unit<EchoLoop<V>>,
}

impl<V: Voicing> EchoEffect<V> {
    pub(crate) fn new(config: &EffectConfig, voicing: V, beat: BeatLength) -> Self {
        let mut effect = Self {
            unit: Unit::new(EchoLoop::new(config, voicing, beat), DEFAULT_DEPTH),
        };
        effect.apply_depth(DEFAULT_DEPTH);
        effect.unit.settle();
        effect
    }
}

implement_effect!(impl<V: Voicing> EchoEffect<V>);

impl<V: Voicing> Controls for EchoEffect<V> {
    fn effect_kind(&self) -> EffectKind {
        self.unit.kernel.voicing.kind()
    }

    fn apply_depth(&mut self, depth: f32) {
        let kernel = &mut self.unit.kernel;
        kernel
            .feedback
            .set_target(kernel.voicing.feedback_for_depth(depth));
        kernel.voicing.set_depth(depth);
        let mix = kernel.voicing.mix_for_depth(depth);
        self.unit.set_mix(mix);
    }

    fn apply_beat(&mut self, beat: BeatLength) {
        self.unit.kernel.beat = beat;
    }

    fn current_beat(&self) -> Option<BeatLength> {
        Some(self.unit.kernel.beat)
    }
}
