//! The fader echo: a deck whose channel fader or crossfader side closes fast echoes the last of
//! its music while it goes, the afro house echo out, and the echoes ring on after the fader is
//! shut. The echo listens after the fader, so it only hears what is still let through.

use dubplate_fx::{BeatLength, Effect, EffectKind, create};

use crate::effects::{effect_config, effect_context};
use crate::processor::{ProcessContext, StereoBlock};

/// A fader closing below this gain starts the echo; a fader moved near the top never does.
const ARMED_BELOW: f32 = 0.8;
/// The smallest drop in gain from one block to the next that counts as closing, so a fader
/// left where it is never echoes.
const CLOSING_STEP: f32 = 1e-3;
/// A fader at or below this gain is shut: its echo rings on however long it stays there.
const SHUT_BELOW: f32 = 0.02;
/// A fader that stops closing above [`SHUT_BELOW`] for this long lets its echo go, so a fader
/// pulled half way and left there does not echo the music on and on.
const PAUSE_SECONDS: f32 = 0.25;
/// The echo's time.
const ECHO_BEAT: BeatLength = BeatLength::HALF;
/// The echo's depth: repeats as loud as the music on top of it, fading over about two beats.
const ECHO_DEPTH: f32 = 0.6;

/// A deck's fader echo, built up front with the deck.
pub(crate) struct FaderEcho {
    echo: Box<dyn Effect>,
    last_gain: f32,
    paused_seconds: f32,
}

impl FaderEcho {
    /// Builds the echo for `sample_rate`. This allocates, so it runs before the renderer
    /// reaches the audio thread.
    pub(crate) fn new(sample_rate: u32) -> Self {
        let mut echo = create(EffectKind::Echo, &effect_config(sample_rate));
        echo.set_beat(ECHO_BEAT);
        echo.set_depth(ECHO_DEPTH);
        Self {
            echo,
            last_gain: 1.0,
            paused_seconds: 0.0,
        }
    }

    /// Follows the deck's fader `gain` for this block and runs the echo over its block, after
    /// the fader. The echo starts while the fader closes below [`ARMED_BELOW`] and stops taking
    /// in music the moment the fader opens again, or once it has rested above shut for
    /// [`PAUSE_SECONDS`]; with `enabled` off it only rings out.
    pub(crate) fn process(
        &mut self,
        enabled: bool,
        gain: f32,
        block: &mut StereoBlock,
        context: &ProcessContext,
    ) {
        let closing = gain < self.last_gain - CLOSING_STEP;
        let opening = gain > self.last_gain + CLOSING_STEP;
        self.last_gain = gain;
        let block_seconds = block.frame_count() as f32 / context.sample_rate.max(1) as f32;
        self.paused_seconds = if closing || gain <= SHUT_BELOW {
            0.0
        } else {
            self.paused_seconds + block_seconds
        };
        if enabled && closing && gain < ARMED_BELOW {
            self.echo.set_enabled(true);
        } else if opening || !enabled || self.paused_seconds > PAUSE_SECONDS {
            self.echo.set_enabled(false);
        }
        if self.echo.is_enabled() || self.echo.is_active() {
            let (left, right) = block.channels_mut();
            self.echo.process(left, right, &effect_context(context));
        }
    }
}
