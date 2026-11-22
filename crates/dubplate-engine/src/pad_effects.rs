use dubplate_fx::{Effect, EffectKind, ParameterValue, create};

use crate::effects::{effect_config, effect_context};
use crate::processor::{ProcessContext, StereoBlock};

const KIND_COUNT: usize = EffectKind::ALL.len();

/// A beat effect played from a pad while it is held, like rekordbox's Pad FX.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PadEffect {
    /// The effect.
    pub kind: EffectKind,
    /// Its parameter: the echo time, sweep period, gate step or stop time in beats, or a
    /// reverb's size. `None`, or a value in another unit than the effect's, leaves it as it is.
    pub parameter: Option<ParameterValue>,
    /// Its level and depth, from 0 to 1.
    pub depth: f32,
    /// What letting go of the pad does to the effect's trail.
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
    held: Option<(EffectKind, PadRelease)>,
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
        if let Some((previous, release)) = self.held.filter(|(previous, _)| *previous != pad.kind) {
            self.release_kind(previous, release);
        }
        if let Some(effect) = self.effect(pad.kind) {
            effect.set_depth(pad.depth);
            if let Some(value) = pad.parameter {
                effect.set_parameter(value);
            }
            effect.set_enabled(true);
        }
        self.held = Some((pad.kind, pad.release));
    }

    /// Lets go of the held effect.
    pub(crate) fn release(&mut self) {
        if let Some((held, release)) = self.held.take() {
            self.release_kind(held, release);
        }
    }

    fn release_kind(&mut self, kind: EffectKind, release: PadRelease) {
        if let Some(effect) = self.effect(kind) {
            match release {
                PadRelease::RingOut => effect.set_enabled(false),
                PadRelease::Dry => effect.switch_off_dry(),
            }
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
