use crate::beat::BeatLength;
use crate::config::EffectConfig;
use crate::context::BlockTiming;
use crate::effect::EffectKind;
use crate::effects::release::turntable::Turntable;
use crate::unit::{Controls, Frame, Kernel, MixLaw, Unit, engaged, implement_effect};

const FULL_MIX_DEPTH: f32 = 0.05;

/// A record slowed by hand: the depth knob sets how slow it plays, and the beat length how
/// long it takes to get there.
///
/// Fully left the music plays at its own speed; turning right slows it steadily, in pitch and
/// tempo together, until fully right stops it. The speed glides to each new setting over the
/// beat length (1 beat by default), so a quick turn to the right is a brake and a turn back to
/// the left lets the record run up to speed again. Switched off, the live music comes straight
/// back.
pub struct VinylBrake {
    unit: Unit<VinylBrakeKernel>,
}

impl VinylBrake {
    /// Builds a vinyl brake gliding over 1 beat, switched off. This allocates.
    pub fn new(config: &EffectConfig) -> Self {
        Self {
            unit: Unit::new(VinylBrakeKernel::new(config), 1.0),
        }
    }
}

implement_effect!(VinylBrake);

impl Controls for VinylBrake {
    fn effect_kind(&self) -> EffectKind {
        EffectKind::VinylBrake
    }

    fn apply_depth(&mut self, depth: f32) {
        self.unit.set_mix(engaged(depth, FULL_MIX_DEPTH));
        self.unit.kernel.target_speed = f64::from(1.0 - depth);
    }

    fn apply_beat(&mut self, beat: BeatLength) {
        self.unit.kernel.beat = beat;
    }

    fn current_beat(&self) -> Option<BeatLength> {
        Some(self.unit.kernel.beat)
    }
}

pub(crate) struct VinylBrakeKernel {
    turntable: Turntable,
    beat: BeatLength,
    speed: f64,
    target_speed: f64,
    glide_per_sample: f64,
}

impl VinylBrakeKernel {
    fn new(config: &EffectConfig) -> Self {
        Self {
            turntable: Turntable::new(config),
            beat: BeatLength::ONE,
            speed: 1.0,
            target_speed: 1.0,
            glide_per_sample: 0.0,
        }
    }
}

impl Kernel for VinylBrakeKernel {
    const TRAILS: bool = false;
    const LISTENS_WHEN_OFF: bool = true;
    const MIX_LAW: MixLaw = MixLaw::Crossfade;

    fn begin_block(&mut self, timing: &BlockTiming) {
        self.glide_per_sample = 1.0 / timing.beats_to_samples(self.beat.beats()).max(1.0);
    }

    #[inline]
    fn tick(&mut self, input: Frame, _beat_position: f64) -> Frame {
        let difference = self.target_speed - self.speed;
        self.speed += difference.clamp(-self.glide_per_sample, self.glide_per_sample);
        let gain = self.speed.sqrt() as f32;
        self.turntable
            .play(input, self.speed)
            .map(|sample| sample * gain)
    }

    fn reset(&mut self) {
        self.turntable.clear();
        self.speed = 1.0;
    }

    fn switched_on(&mut self) {
        self.turntable.drop_stylus();
        self.speed = 1.0;
    }
}
