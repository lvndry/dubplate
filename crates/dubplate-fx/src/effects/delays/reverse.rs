use crate::beat::BeatLength;
use crate::config::EffectConfig;
use crate::context::BlockTiming;
use crate::dsp::pitch_shift::PitchShifter;
use crate::effect::EffectKind;
use crate::unit::{Controls, Frame, Kernel, MixLaw, Unit, implement_effect};

const BACKWARDS: f32 = -1.0;
const TRAIL_MARGIN_SECONDS: f32 = 0.05;
const DEFAULT_DEPTH: f32 = 0.5;

/// A reverse delay: every beat length of music comes back played backwards, one beat length
/// later, each reversed slice faded in and out so the seams never click.
///
/// The depth knob balances the music and the reversed sound, as on a DJ mixer.
pub(crate) struct ReverseDelay {
    unit: Unit<ReverseKernel>,
}

impl ReverseDelay {
    pub(crate) fn new(config: &EffectConfig) -> Self {
        Self {
            unit: Unit::new(ReverseKernel::new(config), DEFAULT_DEPTH),
        }
    }
}

implement_effect!(ReverseDelay);

impl Controls for ReverseDelay {
    fn effect_kind(&self) -> EffectKind {
        EffectKind::RevDelay
    }

    fn apply_depth(&mut self, depth: f32) {
        self.unit.set_mix(depth);
    }

    fn apply_beat(&mut self, beat: BeatLength) {
        self.unit.kernel.beat = beat;
    }

    fn current_beat(&self) -> Option<BeatLength> {
        Some(self.unit.kernel.beat)
    }
}

/// Plays the music backwards through rotating heads: run at -1 times normal speed, the pitch
/// shifter's heads each read one window's worth of music in reverse.
pub(crate) struct ReverseKernel {
    heads: PitchShifter,
    beat: BeatLength,
    window_samples: f32,
    sample_rate: f32,
}

impl ReverseKernel {
    fn new(config: &EffectConfig) -> Self {
        Self {
            heads: PitchShifter::new(config.max_delay_samples()),
            beat: BeatLength::ONE,
            window_samples: 1.0,
            sample_rate: 48_000.0,
        }
    }
}

impl Kernel for ReverseKernel {
    const TRAILS: bool = true;
    const MIX_LAW: MixLaw = MixLaw::Blend;

    fn begin_block(&mut self, timing: &BlockTiming) {
        self.sample_rate = timing.sample_rate;
        self.window_samples = 2.0 * timing.beats_to_samples(self.beat.beats()) as f32;
        self.heads.set_window_samples(self.window_samples);
    }

    #[inline]
    fn tick(&mut self, input: Frame, _beat_position: f64) -> Frame {
        self.heads.tick(input, BACKWARDS)
    }

    fn reset(&mut self) {
        self.heads.clear();
    }

    fn tail_seconds(&self) -> f32 {
        self.window_samples / self.sample_rate + TRAIL_MARGIN_SECONDS
    }
}
