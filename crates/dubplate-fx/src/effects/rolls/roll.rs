//! Rolls: the music from the moment the effect is switched on, recorded and repeated every
//! beat length, forwards or backwards, until it is switched off.

use crate::beat::BeatLength;
use crate::config::EffectConfig;
use crate::context::BlockTiming;
use crate::effect::EffectKind;
use crate::unit::{Controls, Frame, Kernel, MixLaw, Unit, engaged, implement_effect};

const EDGE_SECONDS: f32 = 0.0015;
const FULL_MIX_DEPTH: f32 = 0.5;

/// How a roll plays what it recorded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RollStyle {
    /// Repeats the recording; a new beat length repeats a different part of the same recording.
    Hold,
    /// Records again from the current moment whenever the beat length changes.
    Slip,
    /// Plays the recording backwards on every repeat.
    Reverse,
}

/// A recording of the music from the moment a roll starts, and where the roll is in it.
pub(crate) struct RollKernel {
    recording: [Vec<f32>; 2],
    recorded: usize,
    elapsed: usize,
    style: RollStyle,
    beat: BeatLength,
    loop_samples: usize,
    edge_samples: f32,
}

impl RollKernel {
    pub(crate) fn new(config: &EffectConfig, style: RollStyle) -> Self {
        let capacity = config.max_delay_samples().max(1);
        Self {
            recording: [vec![0.0; capacity], vec![0.0; capacity]],
            recorded: 0,
            elapsed: 0,
            style,
            beat: BeatLength::HALF,
            loop_samples: 1,
            edge_samples: 1.0,
        }
    }

    pub(crate) fn set_beat(&mut self, beat: BeatLength) {
        if beat != self.beat && self.style == RollStyle::Slip {
            self.restart();
        }
        self.beat = beat;
    }

    fn restart(&mut self) {
        self.recorded = 0;
        self.elapsed = 0;
    }

    /// The gain that fades the repeat out and in over a millisecond and a half at each end of
    /// the loop, so the jump back never clicks.
    #[inline]
    fn edge_gain(&self, position: usize) -> f32 {
        let from_start = position as f32;
        let to_end = (self.loop_samples - 1 - position) as f32;
        (from_start.min(to_end) / self.edge_samples).min(1.0)
    }
}

impl Kernel for RollKernel {
    const TRAILS: bool = false;
    const MIX_LAW: MixLaw = MixLaw::Crossfade;

    fn begin_block(&mut self, timing: &BlockTiming) {
        let capacity = self.recording[0].len();
        self.loop_samples = (timing.beats_to_samples(self.beat.beats()).round() as usize)
            .clamp(2, capacity);
        self.edge_samples = (EDGE_SECONDS * timing.sample_rate).max(1.0);
    }

    #[inline]
    fn tick(&mut self, input: Frame, _beat_position: f64) -> Frame {
        let capacity = self.recording[0].len();
        if self.recorded < capacity {
            for (channel, sample) in self.recording.iter_mut().zip(input) {
                channel[self.recorded] = sample;
            }
            self.recorded += 1;
        }
        let first_pass = self.elapsed < self.loop_samples;
        let position = self.elapsed % self.loop_samples;
        self.elapsed += 1;
        if first_pass {
            let to_end = (self.loop_samples - 1 - position) as f32;
            let gain = (to_end / self.edge_samples).min(1.0);
            return input.map(|sample| sample * gain);
        }
        let index = match self.style {
            RollStyle::Hold | RollStyle::Slip => position,
            RollStyle::Reverse => self.loop_samples - 1 - position,
        };
        let gain = self.edge_gain(position);
        [
            self.recording[0][index] * gain,
            self.recording[1][index] * gain,
        ]
    }

    fn reset(&mut self) {
        self.restart();
    }

    fn switched_on(&mut self) {
        self.restart();
    }
}

/// A roll as the effect units see it: Roll, Slip Roll or Rev Roll.
///
/// Switching it on records the music from that moment and repeats it every beat length (1/2
/// beat by default) until it is switched off, which brings the live music straight back. The
/// depth knob balances the original and the roll over its first half; from there on only the
/// roll plays.
pub(crate) struct RollEffect {
    unit: Unit<RollKernel>,
}

impl RollEffect {
    pub(crate) fn new(config: &EffectConfig, style: RollStyle) -> Self {
        Self {
            unit: Unit::new(RollKernel::new(config, style), 1.0),
        }
    }
}

implement_effect!(RollEffect);

impl Controls for RollEffect {
    fn effect_kind(&self) -> EffectKind {
        match self.unit.kernel.style {
            RollStyle::Hold => EffectKind::Roll,
            RollStyle::Slip => EffectKind::SlipRoll,
            RollStyle::Reverse => EffectKind::RevRoll,
        }
    }

    fn apply_depth(&mut self, depth: f32) {
        self.unit.set_mix(engaged(depth, FULL_MIX_DEPTH));
    }

    fn apply_beat(&mut self, beat: BeatLength) {
        self.unit.kernel.set_beat(beat);
    }

    fn current_beat(&self) -> Option<BeatLength> {
        Some(self.unit.kernel.beat)
    }
}
