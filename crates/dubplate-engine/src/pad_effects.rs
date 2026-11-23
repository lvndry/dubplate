use dubplate_fx::{
    BeatLength, Effect, EffectKind, FALLBACK_TEMPO_BPM, MAX_TEMPO_BPM, MIN_TEMPO_BPM,
    ParameterValue, create,
};

use crate::effects::{effect_config, effect_context};
use crate::processor::{ProcessContext, StereoBlock};

const KIND_COUNT: usize = EffectKind::ALL.len();

/// A beat effect played from a pad.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PadEffect {
    /// The effect.
    pub kind: EffectKind,
    /// Its parameter: the echo time, sweep period, gate step or stop time in beats, or a
    /// reverb's size. `None`, or a value in another unit than the effect's, leaves it as it is.
    pub parameter: Option<ParameterValue>,
    /// Its level and depth, from 0 to 1.
    pub depth: f32,
    /// What letting go of a held pad does to the effect's trail. Taps always ring out.
    pub release: PadRelease,
}

/// What letting go of a pad does to its effect's echoes and reverb tail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PadRelease {
    /// The trail rings out on its own, as the beat effects do.
    #[default]
    RingOut,
    /// The trail fades with the effect and the music is dry again at once.
    Dry,
}

/// The audio-thread side of a deck's pad effects: one of every effect, built up front, after
/// the channel's EQ and filter and before its fader. Releasing a pad switches its effect off,
/// so echoes and reverbs ring out, or stop dry when the pad says so, and brakes give the live
/// track back, which kept playing underneath.
pub(crate) struct PadEffects {
    bank: [Box<dyn Effect>; KIND_COUNT],
    active: Option<(EffectKind, Activation)>,
}

#[derive(Clone, Copy)]
enum Activation {
    Held(PadRelease),
    Tap(Capture),
}

#[derive(Clone, Copy)]
enum Capture {
    Pending(BeatLength),
    Remaining(usize),
}

impl PadEffects {
    /// Builds one of every effect for `sample_rate`. This allocates, so it runs before the
    /// renderer reaches the audio thread.
    pub(crate) fn new(sample_rate: u32) -> Self {
        let config = effect_config(sample_rate);
        Self {
            bank: std::array::from_fn(|index| create(EffectKind::ALL[index], &config)),
            active: None,
        }
    }

    fn effect(&mut self, kind: EffectKind) -> Option<&mut Box<dyn Effect>> {
        self.bank.iter_mut().find(|effect| effect.kind() == kind)
    }

    /// Plays `pad`, letting go of any other held effect first.
    pub(crate) fn hold(&mut self, pad: PadEffect) {
        self.activate(pad, Activation::Held(pad.release));
    }

    /// Captures a beat length of the live audio and then lets the effect ring out.
    pub(crate) fn tap(&mut self, pad: PadEffect, capture: BeatLength) {
        self.activate(pad, Activation::Tap(Capture::Pending(capture)));
    }

    fn activate(&mut self, pad: PadEffect, activation: Activation) {
        if self
            .active
            .is_some_and(|(previous, _)| previous != pad.kind)
        {
            self.release();
        }
        if let Some(effect) = self.effect(pad.kind) {
            effect.set_depth(pad.depth);
            if let Some(value) = pad.parameter {
                effect.set_parameter(value);
            }
            if let Activation::Held(_) = activation {
                effect.set_enabled(true);
            }
        }
        self.active = Some((pad.kind, activation));
    }

    /// Releases the active pad. A tap's trail keeps ringing if its capture ends early.
    pub(crate) fn release(&mut self) {
        if let Some((kind, activation)) = self.active.take()
            && let Some(effect) = self.effect(kind)
        {
            match activation {
                Activation::Held(PadRelease::Dry) => effect.switch_off_dry(),
                Activation::Held(PadRelease::RingOut) | Activation::Tap(_) => {
                    effect.set_enabled(false);
                }
            }
        }
    }

    /// Runs the active pad, every effect still ringing out, and the effects that must hear the
    /// music before they are pressed.
    pub(crate) fn process(&mut self, block: &mut StereoBlock, context: &ProcessContext) {
        let mut fx_context = effect_context(context);
        let (left, right) = block.channels_mut();
        let Some((kind, Activation::Tap(capture))) = self.active.filter(|_| !left.is_empty())
        else {
            self.process_channels(left, right, &fx_context);
            return;
        };

        let bpm = context.beat_clock.map_or(FALLBACK_TEMPO_BPM, |clock| {
            clock.bpm.clamp(MIN_TEMPO_BPM, MAX_TEMPO_BPM)
        });
        let sample_rate = f64::from(context.sample_rate);
        let remaining = match capture {
            Capture::Pending(length) => {
                let frames = (length.seconds(bpm) * sample_rate).round().max(1.0) as usize;
                if let Some(effect) = self.effect(kind) {
                    effect.start_capture(frames);
                }
                frames
            }
            Capture::Remaining(frames) => frames,
        };
        let captured = remaining.min(left.len());
        let (captured_left, live_left) = left.split_at_mut(captured);
        let (captured_right, live_right) = right.split_at_mut(captured);
        self.process_channels(captured_left, captured_right, &fx_context);

        if captured == remaining {
            self.release();
            if !live_left.is_empty() {
                fx_context.beat_position += captured as f64 * bpm / (60.0 * sample_rate);
                self.process_channels(live_left, live_right, &fx_context);
            }
        } else {
            self.active = Some((
                kind,
                Activation::Tap(Capture::Remaining(remaining - captured)),
            ));
        }
    }

    fn process_channels(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        context: &dubplate_fx::ProcessContext,
    ) {
        for effect in &mut self.bank {
            if effect.is_enabled() || effect.is_active() || effect.needs_input_when_off() {
                effect.process(left, right, context);
            }
        }
    }
}
