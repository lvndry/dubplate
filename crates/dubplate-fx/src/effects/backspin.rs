use crate::beat::BeatLength;
use crate::config::EffectConfig;
use crate::context::BlockTiming;
use crate::dsp::filters::{Svf, SvfCoefficients};
use crate::dsp::shaping::smoothstep;
use crate::effect::EffectKind;
use crate::effects::turntable::{Motion, Turntable};
use crate::unit::{Controls, Frame, Kernel, MixLaw, Unit, implement_effect};

const SPIN_SPEED: f64 = 3.0;
const WIND_UP_SHARE: f64 = 0.12;
const ANTI_ALIAS_RATIO: f32 = 0.45;

/// A record spun backwards by hand.
///
/// Switching the backspin on throws the record back: within the first eighth of the beat
/// length it reverses to three times normal speed, then slows to a stop by the end of the beat
/// length (2 beats by default, from 1/4 to 32 beats, limited by the stored audio). The music
/// stays stopped until the backspin is switched off, which brings the live music straight back.
///
/// The depth knob sets the spin time, from 1/2 beat to 4 beats.
pub struct Backspin {
    unit: Unit<BackspinKernel>,
}

impl Backspin {
    /// Builds a 2-beat backspin, switched off. This allocates.
    pub fn new(config: &EffectConfig) -> Self {
        Self {
            unit: Unit::new(BackspinKernel::new(config), 1.0),
        }
    }
}

implement_effect!(Backspin);

impl Controls for Backspin {
    fn effect_kind(&self) -> EffectKind {
        EffectKind::Backspin
    }

    fn apply_depth(&mut self, depth: f32) {
        let beats = [
            BeatLength::HALF,
            BeatLength::ONE,
            BeatLength::TWO,
            BeatLength::FOUR,
        ];
        let index = (depth * (beats.len() - 1) as f32).round() as usize;
        self.apply_beat(beats[index.min(beats.len() - 1)]);
    }

    fn apply_beat(&mut self, beat: BeatLength) {
        self.unit.kernel.beat = beat.clamped(BeatLength::QUARTER, BeatLength::THIRTY_TWO);
    }

    fn current_beat(&self) -> Option<BeatLength> {
        Some(self.unit.kernel.beat)
    }
}

/// The speed of the record `progress` of the way through a backspin: a quick throw from
/// forwards to fast backwards, then a long slowdown.
fn spin_speed(progress: f64) -> f64 {
    if progress < WIND_UP_SHARE {
        let throw = f64::from(smoothstep((progress / WIND_UP_SHARE) as f32));
        1.0 - (1.0 + SPIN_SPEED) * throw
    } else {
        let remaining = 1.0 - (progress - WIND_UP_SHARE) / (1.0 - WIND_UP_SHARE);
        -SPIN_SPEED * remaining * remaining
    }
}

pub(crate) struct BackspinKernel {
    turntable: Turntable,
    motion: Motion,
    beat: BeatLength,
    duration_samples: f64,
    anti_alias: [Svf; 2],
    sample_rate: f32,
}

impl BackspinKernel {
    fn new(config: &EffectConfig) -> Self {
        Self {
            turntable: Turntable::new(config),
            motion: Motion::Waiting,
            beat: BeatLength::TWO,
            duration_samples: 1.0,
            anti_alias: [Svf::default(); 2],
            sample_rate: 48_000.0,
        }
    }
}

impl Kernel for BackspinKernel {
    const TRAILS: bool = false;
    const LISTENS_WHEN_OFF: bool = true;
    const MIX_LAW: MixLaw = MixLaw::Crossfade;

    fn begin_block(&mut self, timing: &BlockTiming) {
        self.sample_rate = timing.sample_rate;
        let longest = self.turntable.capacity() / (1.0 + SPIN_SPEED);
        self.duration_samples = timing
            .beats_to_samples(self.beat.beats())
            .clamp(1.0, longest);
    }

    #[inline]
    fn tick(&mut self, input: Frame, _beat_position: f64) -> Frame {
        if self.motion == Motion::Starting {
            self.turntable.drop_stylus();
            for filter in &mut self.anti_alias {
                filter.reset();
            }
            self.motion = Motion::Moving {
                elapsed: 0.0,
                duration: self.duration_samples,
            };
        }
        match self.motion {
            Motion::Moving { elapsed, duration } => {
                let speed = spin_speed(elapsed / duration);
                let played = self.turntable.play(input, speed);
                let magnitude = speed.abs() as f32;
                let cutoff = ANTI_ALIAS_RATIO * self.sample_rate / magnitude.max(1.0);
                let coefficients =
                    SvfCoefficients::new(cutoff, std::f32::consts::FRAC_1_SQRT_2, self.sample_rate);
                let gain = magnitude.min(1.0).sqrt();
                self.motion = if elapsed + 1.0 >= duration {
                    Motion::Stopped
                } else {
                    Motion::Moving {
                        elapsed: elapsed + 1.0,
                        duration,
                    }
                };
                [0, 1].map(|channel| {
                    self.anti_alias[channel]
                        .tick(played[channel], &coefficients)
                        .low
                        * gain
                })
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
        for filter in &mut self.anti_alias {
            filter.reset();
        }
    }

    fn switched_on(&mut self) {
        self.motion = Motion::Starting;
    }
}
