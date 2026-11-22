use crate::beat::BeatLength;
use crate::context::BlockTiming;
use crate::dsp::smoothing::Smoothed;
use crate::effect::EffectKind;
use crate::unit::{Controls, Frame, Kernel, MixLaw, Unit, engaged, implement_effect, knob};

const ATTACK_SECONDS: f32 = 0.000_8;
const RELEASE_SECONDS: f32 = 0.003;
const PATTERN_STEPS: i64 = 16;
const MIN_LENGTH: f32 = 0.05;
const CONTROL_SMOOTHING_SECONDS: f32 = 0.01;
const DEFAULT_LENGTH: f32 = 0.5;
const DEFAULT_DEPTH: f32 = 1.0;
const FULL_CUT_DEPTH: f32 = 1.0 / 3.0;
const WIDEST_DUTY: f32 = 0.75;
const NARROWEST_DUTY: f32 = 1.0 / 3.0;

/// A trance gate that chops the music in time with the beat grid.
///
/// Each step lasts the beat length (1/8 beat by default) and starts exactly on the grid. The
/// gate opens for the first part of each step, set by the length, with edges just soft enough
/// not to click. A sixteen-step pattern can mute chosen steps.
///
/// The depth knob is the balance and the duty of the cut, as on a DJ mixer: the closed steps
/// fall to silence over the first third of its travel, and the open part of each step shrinks
/// from three quarters to a third across its whole travel, so the chop tightens as it turns.
pub struct Gate {
    unit: Unit<GateKernel>,
}

impl Gate {
    /// Builds a gate on 1/8 beat steps, open for half of each step, closing fully, switched off.
    pub fn new() -> Self {
        Self {
            unit: Unit::new(GateKernel::new(), 1.0),
        }
    }

    /// Sets the part of each step the gate stays open, from 0.05 to 1.
    pub fn set_length(&mut self, length: f32) {
        let current = &mut self.unit.kernel.length;
        current.set_target(knob(length, MIN_LENGTH, 1.0, current.target()));
    }

    /// Sets how far the closed steps fall, from 0 (untouched) to 1 (silent).
    pub fn set_gate_depth(&mut self, depth: f32) {
        let current = &mut self.unit.kernel.depth;
        current.set_target(knob(depth, 0.0, 1.0, current.target()));
    }

    /// Sets which of sixteen steps open, lowest bit first. Steps count from the start of the
    /// beat grid, so the pattern lines up with bars.
    pub fn set_pattern(&mut self, pattern: u16) {
        self.unit.kernel.pattern = pattern;
    }
}

impl Default for Gate {
    fn default() -> Self {
        Self::new()
    }
}

implement_effect!(Gate);

impl Controls for Gate {
    fn effect_kind(&self) -> EffectKind {
        EffectKind::Trans
    }

    fn apply_depth(&mut self, depth: f32) {
        self.set_gate_depth(engaged(depth, FULL_CUT_DEPTH));
        self.set_length(WIDEST_DUTY + (NARROWEST_DUTY - WIDEST_DUTY) * depth);
    }

    fn apply_beat(&mut self, beat: BeatLength) {
        self.unit.kernel.beat = beat;
    }

    fn current_beat(&self) -> Option<BeatLength> {
        Some(self.unit.kernel.beat)
    }
}

pub(crate) struct GateKernel {
    beat: BeatLength,
    pattern: u16,
    length: Smoothed,
    depth: Smoothed,
    gain: f32,
    attack: f32,
    release: f32,
    sample_rate: f32,
}

impl GateKernel {
    fn new() -> Self {
        Self {
            beat: BeatLength::EIGHTH,
            pattern: u16::MAX,
            length: Smoothed::new(DEFAULT_LENGTH, CONTROL_SMOOTHING_SECONDS),
            depth: Smoothed::new(DEFAULT_DEPTH, CONTROL_SMOOTHING_SECONDS),
            gain: 1.0,
            attack: 0.0,
            release: 0.0,
            sample_rate: 0.0,
        }
    }
}

impl Kernel for GateKernel {
    const TRAILS: bool = false;
    const MIX_LAW: MixLaw = MixLaw::Crossfade;

    fn begin_block(&mut self, timing: &BlockTiming) {
        if timing.sample_rate != self.sample_rate {
            self.sample_rate = timing.sample_rate;
            self.length.prepare(timing.sample_rate);
            self.depth.prepare(timing.sample_rate);
            self.attack = 1.0 - (-1.0 / (ATTACK_SECONDS * timing.sample_rate)).exp();
            self.release = 1.0 - (-1.0 / (RELEASE_SECONDS * timing.sample_rate)).exp();
        }
    }

    #[inline]
    fn tick(&mut self, input: Frame, beat_position: f64) -> Frame {
        let length = self.length.next();
        let depth = self.depth.next();
        let steps = beat_position / self.beat.beats();
        let step_index = steps.floor() as i64;
        let step_phase = (steps - steps.floor()) as f32;
        let step_bit = step_index.rem_euclid(PATTERN_STEPS) as u32;
        let open = self.pattern & (1 << step_bit) != 0 && step_phase < length;
        let target = if open { 1.0 } else { 1.0 - depth };
        let speed = if target > self.gain {
            self.attack
        } else {
            self.release
        };
        self.gain += (target - self.gain) * speed;
        [input[0] * self.gain, input[1] * self.gain]
    }

    fn reset(&mut self) {
        self.gain = 1.0;
        self.length.snap_to_target();
        self.depth.snap_to_target();
    }
}
