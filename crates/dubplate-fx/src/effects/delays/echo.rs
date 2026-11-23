use crate::beat::BeatLength;
use crate::config::EffectConfig;
use crate::effect::{Effect, EffectKind};
use crate::unit::{Controls, Unit, implement_effect, knob};

use super::tape::DEFAULT_DEPTH;
use super::voicings::Aged;
use super::{EchoEffect, EchoLoop, Voicing};

/// A tempo-synced stereo echo with feedback.
///
/// Each repeat lands on the beat length at the current tempo, and follows tempo changes live.
/// Repeats darken and saturate a little on every pass, like a tape echo. Switched off, the echo
/// stops taking in new audio and lets its repeats ring out.
///
/// The depth knob balances the music and the echo, as the LEVEL/DEPTH knob of a DJ mixer does:
/// at a quarter the first repeat plays 12 dB under the music, in the middle both play at full
/// level, and fully right only the echo plays. It also raises the feedback from 0.5 to 0.8, so
/// the trail grows longer as the knob turns. The repeats never build up past the music that
/// feeds them.
pub struct Echo {
    unit: Unit<EchoLoop<Aged>>,
}

impl Echo {
    /// Builds an echo at 1/2 beat with the depth knob at 0.5 (feedback 0.65, mix 0.5), switched
    /// off. This allocates.
    pub fn new(config: &EffectConfig) -> Self {
        let mut echo = Self {
            unit: Unit::new(
                EchoLoop::new(config, Aged::default(), BeatLength::HALF),
                DEFAULT_DEPTH,
            ),
        };
        echo.apply_depth(DEFAULT_DEPTH);
        echo.unit.settle();
        echo
    }

    /// Builds a half-beat echo that stores the dry music while switched off. For a fader echo,
    /// keep processing it while idle so a fast cut can echo audio that already passed the fader.
    pub fn with_history(config: &EffectConfig) -> impl Effect + use<> {
        EchoEffect::new(config, Aged::<true>::default(), BeatLength::HALF)
    }

    /// Sets how much of each repeat comes back, from 0 (a single repeat) to 1 (repeats that
    /// hold at the level of the music while it plays, and fade slowly once it stops).
    pub fn set_feedback(&mut self, feedback: f32) {
        let current = &mut self.unit.kernel.feedback;
        current.set_target(knob(feedback, 0.0, 1.0, current.target()));
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
        let kernel = &mut self.unit.kernel;
        let feedback = kernel.voicing.feedback_for_depth(depth);
        kernel.feedback.set_target(feedback);
        self.unit.set_mix(depth);
    }

    fn apply_beat(&mut self, beat: BeatLength) {
        self.unit.kernel.beat = beat;
    }

    fn current_beat(&self) -> Option<BeatLength> {
        Some(self.unit.kernel.beat)
    }
}
