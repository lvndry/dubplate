use crate::beat::BeatLength;
use crate::config::EffectConfig;
use crate::context::BlockTiming;
use crate::effect::EffectKind;
use crate::effects::release::turntable::{Motion, Turntable};
use crate::unit::{Controls, Frame, Kernel, MixLaw, Unit, engaged, implement_effect};

const FULL_MIX_DEPTH: f32 = 0.25;

/// A turntable stopping: the music slows down and drops in pitch until it stops.
///
/// Switching the brake on starts the stop, which takes the beat length (1 beat by default,
/// from 1/4 to 32 beats) at the current tempo. The music stays stopped until the brake is
/// switched off, which brings the live music straight back. Like a real cartridge, the sound
/// gets quieter as the record slows.
///
/// The beat length sets the stop time. The depth knob blends the stop in over its first quarter,
/// so from there on the music stops completely.
pub struct Brake {
    unit: Unit<BrakeKernel>,
}

impl Brake {
    /// Builds a 1-beat brake, switched off. This allocates.
    pub fn new(config: &EffectConfig) -> Self {
        Self {
            unit: Unit::new(BrakeKernel::new(config), 1.0),
        }
    }
}

implement_effect!(Brake);

impl Controls for Brake {
    fn effect_kind(&self) -> EffectKind {
        EffectKind::Brake
    }

    fn apply_depth(&mut self, depth: f32) {
        self.unit.set_mix(engaged(depth, FULL_MIX_DEPTH));
    }

    fn apply_beat(&mut self, beat: BeatLength) {
        self.unit.kernel.beat = beat.clamped(BeatLength::QUARTER, BeatLength::THIRTY_TWO);
    }

    fn current_beat(&self) -> Option<BeatLength> {
        Some(self.unit.kernel.beat)
    }
}

pub(crate) struct BrakeKernel {
    turntable: Turntable,
    motion: Motion,
    beat: BeatLength,
    duration_samples: f64,
}

impl BrakeKernel {
    fn new(config: &EffectConfig) -> Self {
        Self {
            turntable: Turntable::new(config),
            motion: Motion::Waiting,
            beat: BeatLength::ONE,
            duration_samples: 1.0,
        }
    }
}

impl Kernel for BrakeKernel {
    const TRAILS: bool = false;
    const LISTENS_WHEN_OFF: bool = true;
    const MIX_LAW: MixLaw = MixLaw::Crossfade;

    fn begin_block(&mut self, timing: &BlockTiming) {
        let longest = 2.0 * self.turntable.capacity();
        self.duration_samples = timing
            .beats_to_samples(self.beat.beats())
            .clamp(1.0, longest);
    }

    #[inline]
    fn tick(&mut self, input: Frame, _beat_position: f64) -> Frame {
        if self.motion == Motion::Starting {
            self.turntable.drop_stylus();
            self.motion = Motion::Moving {
                elapsed: 0.0,
                duration: self.duration_samples,
            };
        }
        match self.motion {
            Motion::Moving { elapsed, duration } => {
                let speed = (1.0 - elapsed / duration).max(0.0);
                let gain = speed.sqrt() as f32;
                let played = self.turntable.play(input, speed);
                self.motion = if elapsed + 1.0 >= duration {
                    Motion::Stopped
                } else {
                    Motion::Moving {
                        elapsed: elapsed + 1.0,
                        duration,
                    }
                };
                played.map(|sample| sample * gain)
            }
            Motion::Stopped => {
                self.turntable.record(input);
                [0.0; 2]
            }
            Motion::Waiting | Motion::Starting => {
                self.turntable.record(input);
                input
            }
        }
    }

    fn reset(&mut self) {
        self.turntable.clear();
        self.motion = Motion::Waiting;
    }

    fn switched_on(&mut self) {
        self.motion = Motion::Starting;
    }
}
