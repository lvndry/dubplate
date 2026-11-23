use std::f32::consts::FRAC_1_SQRT_2;

use crate::context::BlockTiming;
use crate::dsp::coefficients::CachedCoefficients;
use crate::dsp::filters::{Svf, SvfCoefficients, SvfOutput};
use crate::dsp::smoothing::Smoothed;
use crate::effect::EffectKind;
use crate::parameter::ParameterValue;
use crate::unit::{Controls, Frame, Kernel, MixLaw, Unit, engaged, implement_effect, knob};

const MIN_CUTOFF_HZ: f32 = 20.0;
const MAX_CUTOFF_HZ: f32 = 20_000.0;
const CUTOFF_SMOOTHING_SECONDS: f32 = 0.015;
const RESONANCE_SMOOTHING_SECONDS: f32 = 0.02;
const MODE_SMOOTHING_SECONDS: f32 = 0.01;
const RESONANCE_OCTAVES: f32 = 4.0;
const FIRST_STAGE_QUALITY: f32 = 0.541_196_1;
const SECOND_STAGE_QUALITY: f32 = 1.306_563;
const DEFAULT_CUTOFF_HZ: f32 = 1_000.0;
const DEFAULT_RESONANCE_PERCENT: i16 = 30;
const LOW_PASS_SWEEP_CURVE: f32 = 0.7;
const BAND_PASS_FULL_MIX_DEPTH: f32 = 0.5;

/// What a [`Filter`] lets through.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FilterMode {
    /// Lows pass, highs are cut.
    LowPass,
    /// Highs pass, lows are cut.
    HighPass,
    /// A band around the cutoff passes, everything else is cut.
    BandPass,
}

/// How steeply a [`Filter`] or [`crate::SweepFilter`] cuts beyond its cutoff.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FilterSlope {
    /// 12 dB per octave: soft and open, like a classic two-pole filter.
    TwelveDecibels,
    /// 24 dB per octave: tight and punchy, like the filter on a club mixer.
    TwentyFourDecibels,
}

/// A resonant low-pass, high-pass or band-pass filter.
///
/// Built from topology-preserving state-variable filters, so the cutoff can sweep at any speed
/// without clicks, and a high resonance sings without blowing up. Changing the mode crossfades
/// between the two shapes.
///
/// The parameter is the resonance, from 0% (flat) to 100% (a strong, singing peak). The depth
/// knob sweeps the cutoff: a low-pass closes from 20 kHz down to 40 Hz, a high-pass opens from
/// 20 Hz up to 12 kHz, a band-pass moves its band from 100 Hz to 8 kHz. A low-pass moves fastest at the start of the knob, reaching 1.9 kHz a
/// quarter of the way; a high-pass is at 100 Hz there, thinning the bass. A band-pass, which
/// leaves only its band, blends in over the first half of the knob.
pub struct Filter {
    unit: Unit<FilterKernel>,
}

impl Filter {
    /// Builds a filter at 1 kHz, 12 dB per octave, with 30% resonance, switched off.
    pub fn new(mode: FilterMode) -> Self {
        let mut kernel = FilterKernel::new();
        kernel.set_mode(mode);
        kernel.snap();
        Self {
            unit: Unit::new(kernel, 1.0),
        }
    }

    /// Sets what the filter lets through, crossfading from the previous mode.
    pub fn set_mode(&mut self, mode: FilterMode) {
        self.unit.kernel.set_mode(mode);
    }

    /// What the filter lets through.
    pub fn mode(&self) -> FilterMode {
        self.unit.kernel.mode
    }

    /// Sets the cutoff, or the centre of the band, from 20 Hz to 20 kHz.
    pub fn set_cutoff_hz(&mut self, hertz: f32) {
        let cutoff = &mut self.unit.kernel.cutoff_octaves;
        let hertz = knob(hertz, MIN_CUTOFF_HZ, MAX_CUTOFF_HZ, cutoff.target().exp2());
        cutoff.set_target(hertz.log2());
    }

    /// The cutoff the filter is moving to, in hertz.
    pub fn cutoff_hz(&self) -> f32 {
        self.unit.kernel.cutoff_octaves.target().exp2()
    }

    /// Sets the resonance, from 0 (flat) to 1 (a strong, singing peak at the cutoff).
    pub fn set_resonance(&mut self, resonance: f32) {
        let kernel = &mut self.unit.kernel;
        let resonance = knob(resonance, 0.0, 1.0, kernel.resonance.target());
        kernel.resonance.set_target(resonance);
        kernel.resonance_percent = (resonance * 100.0).round() as i16;
    }

    /// Sets how steeply the filter cuts.
    pub fn set_slope(&mut self, slope: FilterSlope) {
        self.unit.kernel.slope = slope;
    }
}

implement_effect!(Filter);

impl Controls for Filter {
    fn effect_kind(&self) -> EffectKind {
        match self.unit.kernel.mode {
            FilterMode::LowPass => EffectKind::LowPass,
            FilterMode::HighPass => EffectKind::HighPass,
            FilterMode::BandPass => EffectKind::BandPass,
        }
    }

    fn apply_depth(&mut self, depth: f32) {
        let (open_hz, closed_hz) = match self.unit.kernel.mode {
            FilterMode::LowPass => (20_000.0_f32, 40.0_f32),
            FilterMode::HighPass => (20.0, 12_000.0),
            FilterMode::BandPass => (100.0, 8_000.0),
        };
        let travel = match self.unit.kernel.mode {
            FilterMode::LowPass => depth.powf(LOW_PASS_SWEEP_CURVE),
            FilterMode::HighPass | FilterMode::BandPass => depth,
        };
        let octaves = open_hz.log2() + (closed_hz.log2() - open_hz.log2()) * travel;
        self.set_cutoff_hz(octaves.exp2());
        let mix = match self.unit.kernel.mode {
            FilterMode::LowPass | FilterMode::HighPass => 1.0,
            FilterMode::BandPass => engaged(depth, BAND_PASS_FULL_MIX_DEPTH),
        };
        self.unit.set_mix(mix);
    }

    fn apply_parameter(&mut self, value: ParameterValue) {
        if let ParameterValue::Percent(percent) = value {
            self.set_resonance(f32::from(percent.clamp(0, 100)) / 100.0);
        }
    }

    fn current_parameter(&self) -> Option<ParameterValue> {
        Some(ParameterValue::Percent(self.unit.kernel.resonance_percent))
    }
}

/// Weights that blend the three outputs of a state-variable filter.
pub(crate) struct ModeWeights {
    low: Smoothed,
    band: Smoothed,
    high: Smoothed,
}

impl ModeWeights {
    pub(crate) fn new() -> Self {
        Self {
            low: Smoothed::new(1.0, MODE_SMOOTHING_SECONDS),
            band: Smoothed::new(0.0, MODE_SMOOTHING_SECONDS),
            high: Smoothed::new(0.0, MODE_SMOOTHING_SECONDS),
        }
    }

    fn prepare(&mut self, sample_rate: f32) {
        self.low.prepare(sample_rate);
        self.band.prepare(sample_rate);
        self.high.prepare(sample_rate);
    }

    fn set(&mut self, mode: FilterMode) {
        self.low
            .set_target(f32::from(u8::from(mode == FilterMode::LowPass)));
        self.band
            .set_target(f32::from(u8::from(mode == FilterMode::BandPass)));
        self.high
            .set_target(f32::from(u8::from(mode == FilterMode::HighPass)));
    }

    fn snap(&mut self) {
        self.low.snap_to_target();
        self.band.snap_to_target();
        self.high.snap_to_target();
    }

    #[inline]
    fn next(&mut self) -> [f32; 3] {
        [self.low.next(), self.band.next(), self.high.next()]
    }
}

#[inline]
fn blend(output: SvfOutput, weights: [f32; 3]) -> f32 {
    output.low * weights[0] + output.band * weights[1] + output.high * weights[2]
}

/// The quality factors of the two cascaded stages for a slope and a resonance from 0 to 1.
pub(crate) fn stage_qualities(slope: FilterSlope, resonance: f32) -> (f32, f32) {
    let boost = (resonance * RESONANCE_OCTAVES).exp2();
    match slope {
        FilterSlope::TwelveDecibels => (FRAC_1_SQRT_2 * boost, FRAC_1_SQRT_2),
        FilterSlope::TwentyFourDecibels => (
            FIRST_STAGE_QUALITY,
            SECOND_STAGE_QUALITY * (resonance * 3.2).exp2(),
        ),
    }
}

/// Two state-variable filters per channel, run as one stage or as a cascade.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct FilterStages {
    first: [Svf; 2],
    second: [Svf; 2],
}

impl FilterStages {
    pub(crate) fn reset(&mut self) {
        for filter in self.first.iter_mut().chain(self.second.iter_mut()) {
            filter.reset();
        }
    }

    /// Runs one channel through the stages, choosing each stage's output with `weights`.
    #[inline]
    pub(crate) fn tick(
        &mut self,
        channel: usize,
        input: f32,
        slope: FilterSlope,
        coefficients: &(SvfCoefficients, SvfCoefficients),
        weights: [f32; 3],
    ) -> f32 {
        let first = blend(self.first[channel].tick(input, &coefficients.0), weights);
        match slope {
            FilterSlope::TwelveDecibels => first,
            FilterSlope::TwentyFourDecibels => {
                blend(self.second[channel].tick(first, &coefficients.1), weights)
            }
        }
    }
}

pub(crate) struct FilterKernel {
    resonance_percent: i16,
    mode: FilterMode,
    slope: FilterSlope,
    weights: ModeWeights,
    cutoff_octaves: Smoothed,
    resonance: Smoothed,
    stages: FilterStages,
    coefficients: CachedCoefficients<FilterSlope, (SvfCoefficients, SvfCoefficients)>,
    sample_rate: f32,
}

impl FilterKernel {
    fn new() -> Self {
        Self {
            mode: FilterMode::LowPass,
            slope: FilterSlope::TwelveDecibels,
            weights: ModeWeights::new(),
            cutoff_octaves: Smoothed::new(DEFAULT_CUTOFF_HZ.log2(), CUTOFF_SMOOTHING_SECONDS),
            resonance_percent: DEFAULT_RESONANCE_PERCENT,
            resonance: Smoothed::new(
                f32::from(DEFAULT_RESONANCE_PERCENT) / 100.0,
                RESONANCE_SMOOTHING_SECONDS,
            ),
            stages: FilterStages::default(),
            coefficients: CachedCoefficients::new(
                FilterSlope::TwelveDecibels,
                (
                    SvfCoefficients::new(DEFAULT_CUTOFF_HZ, FRAC_1_SQRT_2, 48_000.0),
                    SvfCoefficients::new(DEFAULT_CUTOFF_HZ, FRAC_1_SQRT_2, 48_000.0),
                ),
            ),
            sample_rate: 0.0,
        }
    }

    fn set_mode(&mut self, mode: FilterMode) {
        self.mode = mode;
        self.weights.set(mode);
    }

    fn snap(&mut self) {
        self.weights.snap();
        self.cutoff_octaves.snap_to_target();
        self.resonance.snap_to_target();
    }
}

impl Kernel for FilterKernel {
    const TRAILS: bool = false;
    const MIX_LAW: MixLaw = MixLaw::Crossfade;

    fn begin_block(&mut self, timing: &BlockTiming) {
        if timing.sample_rate != self.sample_rate {
            self.sample_rate = timing.sample_rate;
            self.coefficients.invalidate();
            self.weights.prepare(timing.sample_rate);
            self.cutoff_octaves.prepare(timing.sample_rate);
            self.resonance.prepare(timing.sample_rate);
        }
    }

    #[inline]
    fn tick(&mut self, input: Frame, _beat_position: f64) -> Frame {
        let cutoff_octaves = self.cutoff_octaves.next();
        let resonance = self.resonance.next();
        let coefficients =
            self.coefficients
                .current((cutoff_octaves, resonance, self.slope), || {
                    let cutoff = cutoff_octaves.exp2();
                    let (first_quality, second_quality) = stage_qualities(self.slope, resonance);
                    (
                        SvfCoefficients::new(cutoff, first_quality, self.sample_rate),
                        SvfCoefficients::new(cutoff, second_quality, self.sample_rate),
                    )
                });
        let weights = self.weights.next();
        [0, 1].map(|channel| {
            self.stages
                .tick(channel, input[channel], self.slope, coefficients, weights)
        })
    }

    fn reset(&mut self) {
        self.stages.reset();
        self.snap();
    }
}
