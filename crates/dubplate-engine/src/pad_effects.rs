use dubplate_fx::{BeatLength, Effect, EffectKind, create};

use crate::effects::{effect_config, effect_context};
use crate::processor::{ProcessContext, StereoBlock};

const KIND_COUNT: usize = EffectKind::ALL.len();

/// A beat effect played from a pad while it is held, like rekordbox's Pad FX.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PadEffect {
    /// The effect.
    pub kind: EffectKind,
    /// Its beat length: the echo time, the sweep period, the gate step or the stop time.
    pub beat: BeatLength,
    /// Its level and depth, from 0 to 1.
    pub depth: f32,
}

/// The audio-thread side of a deck's pad effects: one of every effect, built up front, after
/// the channel's EQ and filter and before its fader. Releasing a pad switches its effect off,
/// so echoes and reverbs ring out and brakes give the live track back, which kept playing
/// underneath.
pub(crate) struct PadEffects {
    bank: [Box<dyn Effect>; KIND_COUNT],
    held: Option<EffectKind>,
}

impl PadEffects {
    /// Builds one of every effect for `sample_rate`. This allocates, so it runs before the
    /// renderer reaches the audio thread.
    pub(crate) fn new(sample_rate: u32) -> Self {
        let config = effect_config(sample_rate);
        Self {
            bank: std::array::from_fn(|index| create(EffectKind::ALL[index], &config)),
            held: None,
        }
    }

    fn effect(&mut self, kind: EffectKind) -> Option<&mut Box<dyn Effect>> {
        self.bank.iter_mut().find(|effect| effect.kind() == kind)
    }

    /// Plays `pad`, letting go of any other held effect first.
    pub(crate) fn hold(&mut self, pad: PadEffect) {
        if let Some(previous) = self.held.filter(|previous| *previous != pad.kind) {
            self.release_kind(previous);
        }
        if let Some(effect) = self.effect(pad.kind) {
            effect.set_depth(pad.depth);
            effect.set_beat(pad.beat);
            effect.set_enabled(true);
        }
        self.held = Some(pad.kind);
    }

    /// Lets go of the held effect.
    pub(crate) fn release(&mut self) {
        if let Some(held) = self.held.take() {
            self.release_kind(held);
        }
    }

    fn release_kind(&mut self, kind: EffectKind) {
        if let Some(effect) = self.effect(kind) {
            effect.set_enabled(false);
        }
    }

    /// Runs the held effect, every effect still ringing out, and the effects that must hear the
    /// music before they are pressed.
    pub(crate) fn process(&mut self, block: &mut StereoBlock, context: &ProcessContext) {
        let effect_context = effect_context(context);
        let (left, right) = block.channels_mut();
        for effect in &mut self.bank {
            if effect.is_enabled() || effect.is_active() || effect.needs_input_when_off() {
                effect.process(left, right, &effect_context);
            }
        }
    }
}
