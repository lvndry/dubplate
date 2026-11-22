use std::f64::consts::TAU;

use crate::beat::BeatLength;
use crate::context::BlockTiming;
use crate::dsp::filters::SvfCoefficients;
use crate::dsp::lfo::SyncedPhase;
use crate::dsp::smoothing::Smoothed;
use crate::effect::EffectKind;
use crate::effects::filters::filter::{FilterSlope, FilterStages, stage_qualities};
use crate::unit::{Controls, Frame, Kernel, MixLaw, Unit, engaged, implement_effect};

const OPEN_HZ: f32 = 16_000.0;
const DEEPEST_HZ: f32 = 150.0;
const SWEEP_CURVE: f32 = 0.7;
const LOWEST_RESONANCE: f32 = 0.2;
const HIGHEST_RESONANCE: f32 = 0.7;
const FULL_MIX_DEPTH: f32 = 1.0 / 3.0;
const CONTROL_INTERVAL: u32 = 16;
const CONTROL_SMOOTHING_SECONDS: f32 = 0.02;
const DEFAULT_DEPTH: f32 = 0.5;
const LOW_PASS_WEIGHTS: [f32; 3] = [1.0, 0.0, 0.0];

/// A low-pass filter whose cutoff sweeps down and back up in time with the beat, like the
/// Filter of a DJ mixer's beat effects.
///
/// One sweep takes the beat length (4 beats by default) and lines up with the bar: the filter
/// is open on the bar and closed half way through it. It cuts at 24 dB per octave.
///
/// The depth knob stresses the effect the further it turns: the filter fades in over the first
/// third of its travel, while across the whole travel the sweep reaches deeper, from 16 kHz down
/// to 150 Hz at the end, and the resonance grows.
pub struct BeatFilter {
    unit: Unit<BeatFilterKernel>,
}

impl BeatFilter {
    /// Builds a beat filter sweeping over 4 beats, with the depth knob half way, switched off.
    pub fn new() -> Self {
        let mut filter = Self {
            unit: Unit::new(
                BeatFilterKernel::new(),
                engaged(DEFAULT_DEPTH, FULL_MIX_DEPTH),
            ),
        };
        filter.apply_depth(DEFAULT_DEPTH);
        filter.unit.kernel.snap();
        filter
    }
}

impl Default for BeatFilter {
    fn default() -> Self {
        Self::new()
    }
}

implement_effect!(BeatFilter);

impl Controls for BeatFilter {
    fn effect_kind(&self) -> EffectKind {
        EffectKind::Filter
    }

    fn apply_depth(&mut self, depth: f32) {
        self.unit.set_mix(engaged(depth, FULL_MIX_DEPTH));
        let kernel = &mut self.unit.kernel;
        let reach = depth.powf(SWEEP_CURVE);
        kernel
            .deepest_octaves
            .set_target(OPEN_HZ.log2() + (DEEPEST_HZ.log2() - OPEN_HZ.log2()) * reach);
        kernel
            .resonance
            .set_target(LOWEST_RESONANCE + (HIGHEST_RESONANCE - LOWEST_RESONANCE) * depth);
    }

    fn apply_beat(&mut self, beat: BeatLength) {
        self.unit.kernel.beat = beat;
    }

    fn current_beat(&self) -> Option<BeatLength> {
        Some(self.unit.kernel.beat)
    }
}

pub(crate) struct BeatFilterKernel {
    stages: FilterStages,
    coefficients: (SvfCoefficients, SvfCoefficients),
    phase: SyncedPhase,
    beat: BeatLength,
    deepest_octaves: Smoothed,
    resonance: Smoothed,
    control_countdown: u32,
    beats_per_sample: f64,
    sample_rate: f32,
}

impl BeatFilterKernel {
    fn new() -> Self {
        let open = SvfCoefficients::new(OPEN_HZ, 0.7, 48_000.0);
        Self {
            stages: FilterStages::default(),
            coefficients: (open, open),
            phase: SyncedPhase::new(),
            beat: BeatLength::FOUR,
            deepest_octaves: Smoothed::new(OPEN_HZ.log2(), CONTROL_SMOOTHING_SECONDS),
            resonance: Smoothed::new(LOWEST_RESONANCE, CONTROL_SMOOTHING_SECONDS),
            control_countdown: 0,
            beats_per_sample: 0.0,
            sample_rate: 48_000.0,
        }
    }

    fn snap(&mut self) {
        self.deepest_octaves.snap_to_target();
        self.resonance.snap_to_target();
    }

    fn update_coefficients(&mut self, phase: f64) {
        let closing = (0.5 - 0.5 * (TAU * phase).cos()) as f32;
        let octaves = OPEN_HZ.log2() + (self.deepest_octaves.current() - OPEN_HZ.log2()) * closing;
        let (first_quality, second_quality) =
            stage_qualities(FilterSlope::TwentyFourDecibels, self.resonance.current());
        let cutoff = octaves.exp2();
        self.coefficients = (
            SvfCoefficients::new(cutoff, first_quality, self.sample_rate),
            SvfCoefficients::new(cutoff, second_quality, self.sample_rate),
        );
    }
}

impl Kernel for BeatFilterKernel {
    const TRAILS: bool = false;
    const MIX_LAW: MixLaw = MixLaw::Crossfade;

    fn begin_block(&mut self, timing: &BlockTiming) {
        if timing.sample_rate != self.sample_rate {
            self.control_countdown = 0;
        }
        self.sample_rate = timing.sample_rate;
        self.beats_per_sample = timing.beats_per_sample;
        self.phase.prepare(timing.sample_rate);
        self.deepest_octaves.prepare(timing.sample_rate);
        self.resonance.prepare(timing.sample_rate);
    }

    #[inline]
    fn tick(&mut self, input: Frame, beat_position: f64) -> Frame {
        let phase = self
            .phase
            .next(beat_position, self.beat.beats(), self.beats_per_sample);
        self.deepest_octaves.next();
        self.resonance.next();
        if self.control_countdown == 0 {
            self.update_coefficients(phase);
            self.control_countdown = CONTROL_INTERVAL;
        }
        self.control_countdown -= 1;
        [0, 1].map(|channel| {
            self.stages.tick(
                channel,
                input[channel],
                FilterSlope::TwentyFourDecibels,
                &self.coefficients,
                LOW_PASS_WEIGHTS,
            )
        })
    }

    fn reset(&mut self) {
        self.stages.reset();
        self.phase.unlock();
        self.snap();
        self.control_countdown = 0;
    }
}
