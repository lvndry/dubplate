use crate::beat::BeatLength;
use crate::context::{BlockTiming, ProcessContext};
use crate::dsp::shaping::{sanitize_input, sanitize_output};
use crate::dsp::smoothing::Smoothed;
use crate::effect::EffectKind;
use crate::parameter::ParameterValue;

pub(crate) type Frame = [f32; 2];

const MIX_SMOOTHING_SECONDS: f32 = 0.01;
const SWITCH_SMOOTHING_SECONDS: f32 = 0.004;
const CAPTURE_EDGE_SECONDS: f32 = 0.001;
const QUIET_LEVEL: f32 = 1e-5;

/// How the mix knob balances the untouched signal against the effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MixLaw {
    /// A straight crossfade, for effects whose output is still the same sound (filters,
    /// modulation, gate, crusher).
    Crossfade,
    /// Both at full level in the middle; past it the dry signal fades out. Echo and reverb
    /// add to the music up to half way and replace it beyond, as the LEVEL/DEPTH knob of a DJ
    /// mixer balances the original and the effect sound. The effect comes in along an audio
    /// taper, so the first quarter stays gentle: 12 dB under the music a quarter of the way,
    /// at full level half way. The master limiter keeps the sum from clipping.
    Blend,
    /// The music always plays at full level and the effect is laid over it at the mix: for
    /// effects that add a sound of their own, such as a tone or noise.
    Layer,
}

impl MixLaw {
    #[inline]
    fn gains(self, mix: f32) -> (f32, f32) {
        match self {
            Self::Crossfade => (1.0 - mix, mix),
            Self::Blend => {
                let rise = (2.0 * mix).min(1.0);
                ((2.0 - 2.0 * mix).min(1.0), rise * rise)
            }
            Self::Layer => (1.0, mix),
        }
    }
}

/// The signal processing of one effect, without the switching and mixing every effect shares.
pub(crate) trait Kernel: Send {
    /// Whether the effect keeps ringing after it is switched off.
    const TRAILS: bool;
    /// Whether the effect needs to hear the music while switched off.
    const LISTENS_WHEN_OFF: bool = false;
    /// How the mix knob blends the effect in.
    const MIX_LAW: MixLaw;

    fn begin_block(&mut self, timing: &BlockTiming);

    /// The fraction of the dry signal this kernel keeps audible independently of the mix.
    #[inline]
    fn dry_passthrough(&self) -> f32 {
        0.0
    }

    /// Processes one frame and returns the effect's output, before mixing.
    fn tick(&mut self, input: Frame, beat_position: f64) -> Frame;

    /// Hears a frame while idle, without contributing its output to the mix.
    #[inline]
    fn listen(&mut self, input: Frame, beat_position: f64) {
        self.tick(input, beat_position);
    }

    /// Clears every internal state.
    fn reset(&mut self);

    /// Moves every smoothed control straight to its target without touching the audio the
    /// effect holds. Constructors call it, so a new effect starts at its settings without
    /// writing through buffers that are already silent.
    fn settle(&mut self) {}

    /// Called each time the effect is switched on.
    fn switched_on(&mut self) {}

    /// Called each time the effect is switched off, before any trail fades out.
    fn switched_off(&mut self) {}

    /// Lets the caller control when the effect stops taking new input.
    fn set_external_capture(&mut self, _capture: bool) {}

    /// How long the output must stay silent before a switched-off trail is over.
    fn tail_seconds(&self) -> f32 {
        0.0
    }
}

/// The switching, mixing and safety shared by every effect.
pub(crate) struct Unit<K> {
    pub(crate) kernel: K,
    enabled: bool,
    capture: Option<CaptureEnvelope>,
    mix: Smoothed,
    presence: Smoothed,
    trail_cut: bool,
    idle: bool,
    quiet_samples: usize,
    sample_rate: f32,
}

struct CaptureEnvelope {
    elapsed: usize,
    remaining: usize,
    start_gain: f32,
    gain: f32,
}

impl CaptureEnvelope {
    fn next(&mut self, edge_frames: f32) -> f32 {
        if self.remaining == 0 {
            return 0.0;
        }
        let attack = ((self.elapsed + 1) as f32 / edge_frames).min(1.0);
        let release = (self.remaining as f32 / edge_frames).min(1.0);
        self.gain = (self.start_gain + (1.0 - self.start_gain) * attack).min(release);
        self.elapsed += 1;
        self.remaining -= 1;
        self.gain
    }
}

impl<K: Kernel> Unit<K> {
    pub(crate) fn new(kernel: K, mix: f32) -> Self {
        Self {
            kernel,
            enabled: false,
            capture: None,
            mix: Smoothed::new(mix.clamp(0.0, 1.0), MIX_SMOOTHING_SECONDS),
            presence: Smoothed::new(0.0, SWITCH_SMOOTHING_SECONDS),
            trail_cut: false,
            idle: true,
            quiet_samples: 0,
            sample_rate: 0.0,
        }
    }

    pub(crate) fn set_mix(&mut self, mix: f32) {
        if mix.is_finite() {
            self.mix.set_target(mix.clamp(0.0, 1.0));
        }
    }

    pub(crate) fn mix(&self) -> f32 {
        self.mix.target()
    }

    pub(crate) fn set_enabled(&mut self, enabled: bool) {
        let resume_hold = enabled && self.capture.is_some();
        if enabled == self.enabled && !resume_hold {
            return;
        }
        self.enabled = enabled;
        self.presence.set_target(if enabled { 1.0 } else { 0.0 });
        if enabled {
            if resume_hold {
                self.capture = None;
                self.kernel.set_external_capture(false);
            }
            self.trail_cut = false;
            if self.idle {
                self.mix.snap_to_target();
                if !K::TRAILS && !K::LISTENS_WHEN_OFF {
                    self.kernel.reset();
                }
            }
            self.idle = false;
            self.quiet_samples = 0;
            self.kernel.switched_on();
        } else {
            self.kernel.switched_off();
        }
    }

    pub(crate) fn start_capture(&mut self, frames: usize) {
        if !self.enabled {
            self.set_enabled(true);
        }
        if K::TRAILS {
            // When re-tapped within the window, continue the input fade from its current
            // gain. Feedback is independent of this envelope and keeps ringing.
            let start_gain = self.capture.as_ref().map_or(0.0, |capture| capture.gain);
            self.capture = Some(CaptureEnvelope {
                elapsed: 0,
                remaining: frames,
                start_gain,
                gain: start_gain,
            });
            self.kernel.set_external_capture(true);
        }
    }

    /// Switches off and fades the trail out with the switch, leaving the music dry. The trail
    /// keeps running silently until it has died away, so the effect goes idle as usual.
    pub(crate) fn switch_off_dry(&mut self) {
        self.set_enabled(false);
        self.trail_cut = true;
    }

    pub(crate) fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub(crate) fn is_active(&self) -> bool {
        !self.idle
    }

    pub(crate) fn needs_input_when_off(&self) -> bool {
        K::LISTENS_WHEN_OFF
    }

    /// Moves the mix and every smoothed control of the effect straight to its target, for a
    /// newly built effect.
    pub(crate) fn settle(&mut self) {
        self.kernel.settle();
        self.mix.snap_to_target();
    }

    pub(crate) fn reset(&mut self) {
        self.kernel.reset();
        self.mix.snap_to_target();
        self.presence.snap_to_target();
        self.quiet_samples = 0;
        self.idle = !self.enabled;
    }

    pub(crate) fn process(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        context: &ProcessContext,
    ) {
        let timing = BlockTiming::from_context(context);
        if timing.sample_rate != self.sample_rate {
            self.sample_rate = timing.sample_rate;
            self.mix.prepare(timing.sample_rate);
            self.presence.prepare(timing.sample_rate);
        }
        self.kernel.begin_block(&timing);

        let frames = left.len().min(right.len());
        if self.idle {
            self.pass_through(&mut left[..frames], &mut right[..frames], &timing);
            return;
        }

        let tail_samples = (self.kernel.tail_seconds() * timing.sample_rate) as usize;
        let capture_edge_frames = (CAPTURE_EDGE_SECONDS * timing.sample_rate).max(1.0);
        let mut poisoned = false;
        let mut beat_position = timing.beat_position;
        for (left_sample, right_sample) in left[..frames].iter_mut().zip(&mut right[..frames]) {
            let dry = [sanitize_input(*left_sample), sanitize_input(*right_sample)];
            let presence = self.presence.next();
            let (dry_mix, wet_mix) = K::MIX_LAW.gains(self.mix.next());
            let dry_mix = dry_mix + (1.0 - dry_mix) * self.kernel.dry_passthrough();
            let dry_gain = presence * dry_mix + (1.0 - presence);
            let (dry_gain, send_gain, wet_gain, heard_gain) =
                if let Some(capture) = &mut self.capture {
                    let send_gain = if self.enabled {
                        capture.next(capture_edge_frames)
                    } else {
                        0.0
                    };
                    let heard_gain = if self.trail_cut {
                        presence * wet_mix
                    } else {
                        wet_mix
                    };
                    (1.0, send_gain, wet_mix, heard_gain)
                } else if K::TRAILS && self.trail_cut {
                    (dry_gain, presence, wet_mix, presence * wet_mix)
                } else if K::TRAILS {
                    (dry_gain, presence, wet_mix, wet_mix)
                } else {
                    (dry_gain, 1.0, presence * wet_mix, presence * wet_mix)
                };

            let mut wet = self
                .kernel
                .tick([dry[0] * send_gain, dry[1] * send_gain], beat_position);
            if !(wet[0].is_finite() && wet[1].is_finite()) {
                wet = [0.0, 0.0];
                poisoned = true;
            }

            *left_sample = sanitize_output(dry[0] * dry_gain + wet[0] * heard_gain);
            *right_sample = sanitize_output(dry[1] * dry_gain + wet[1] * heard_gain);
            beat_position += timing.beats_per_sample;

            if !self.enabled && self.presence.is_settled() {
                let quiet = (wet[0] * wet_gain).abs() < QUIET_LEVEL
                    && (wet[1] * wet_gain).abs() < QUIET_LEVEL;
                self.quiet_samples = if quiet { self.quiet_samples + 1 } else { 0 };
            }
        }

        if poisoned {
            self.kernel.reset();
        }
        if !self.enabled && self.presence.is_settled() && self.quiet_samples > tail_samples {
            self.idle = true;
        }
    }

    fn pass_through(&mut self, left: &mut [f32], right: &mut [f32], timing: &BlockTiming) {
        let mut beat_position = timing.beat_position;
        for (left_sample, right_sample) in left.iter_mut().zip(right.iter_mut()) {
            *left_sample = sanitize_input(*left_sample);
            *right_sample = sanitize_input(*right_sample);
            if K::LISTENS_WHEN_OFF {
                self.kernel
                    .listen([*left_sample, *right_sample], beat_position);
                beat_position += timing.beats_per_sample;
            }
        }
    }
}

/// The controls each effect maps onto the shared [`Effect`] interface.
pub(crate) trait Controls {
    fn effect_kind(&self) -> EffectKind;

    fn apply_depth(&mut self, depth: f32);

    fn apply_beat(&mut self, _beat: BeatLength) {}

    fn current_beat(&self) -> Option<BeatLength> {
        None
    }

    /// Sets the effect's parameter. Effects whose parameter is a beat length take it through
    /// [`Controls::apply_beat`]; the others override this.
    fn apply_parameter(&mut self, value: ParameterValue) {
        if let ParameterValue::Beats(beat) = value {
            self.apply_beat(beat);
        }
    }

    fn current_parameter(&self) -> Option<ParameterValue> {
        self.current_beat().map(ParameterValue::Beats)
    }
}

/// Implements [`Effect`] for a type with a `unit` field and [`Controls`].
macro_rules! implement_effect {
    (impl<$generic:ident: $bound:path> $effect:ty) => {
        $crate::unit::implement_effect!(@body [$generic: $bound] $effect);
    };
    ($effect:ty) => {
        $crate::unit::implement_effect!(@body [] $effect);
    };
    (@body [$($generic:ident: $bound:path)?] $effect:ty) => {
        impl$(<$generic: $bound>)? $crate::effect::Effect for $effect {
            fn kind(&self) -> $crate::effect::EffectKind {
                $crate::unit::Controls::effect_kind(self)
            }

            fn process(
                &mut self,
                left: &mut [f32],
                right: &mut [f32],
                context: &$crate::context::ProcessContext,
            ) {
                self.unit.process(left, right, context);
            }

            fn set_enabled(&mut self, enabled: bool) {
                self.unit.set_enabled(enabled);
            }

            fn start_capture(&mut self, frames: usize) {
                self.unit.start_capture(frames);
            }

            fn switch_off_dry(&mut self) {
                self.unit.switch_off_dry();
            }

            fn is_enabled(&self) -> bool {
                self.unit.is_enabled()
            }

            fn is_active(&self) -> bool {
                self.unit.is_active()
            }

            fn needs_input_when_off(&self) -> bool {
                self.unit.needs_input_when_off()
            }

            fn set_mix(&mut self, mix: f32) {
                self.unit.set_mix(mix);
            }

            fn mix(&self) -> f32 {
                self.unit.mix()
            }

            fn set_depth(&mut self, depth: f32) {
                if depth.is_finite() {
                    $crate::unit::Controls::apply_depth(self, depth.clamp(0.0, 1.0));
                }
            }

            fn set_parameter(&mut self, value: $crate::parameter::ParameterValue) {
                $crate::unit::Controls::apply_parameter(self, value);
            }

            fn parameter(&self) -> Option<$crate::parameter::ParameterValue> {
                $crate::unit::Controls::current_parameter(self)
            }

            fn reset(&mut self) {
                self.unit.reset();
            }
        }
    };
}

pub(crate) use implement_effect;

/// The mix of an effect whose depth knob stresses it more the further it turns: the effect
/// fades in over the first `full_at` of the knob's travel and is fully in from there, leaving
/// the rest of the travel to make it deeper.
pub(crate) fn engaged(depth: f32, full_at: f32) -> f32 {
    (depth / full_at).clamp(0.0, 1.0)
}

/// Clamps a knob value from a caller, turning NaN into `fallback`.
pub(crate) fn knob(value: f32, minimum: f32, maximum: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(minimum, maximum)
    } else {
        fallback
    }
}
