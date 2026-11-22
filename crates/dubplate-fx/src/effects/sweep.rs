use crate::context::BlockTiming;
use crate::context::ProcessContext;
use crate::dsp::filters::SvfCoefficients;
use crate::dsp::shaping::smoothstep;
use crate::dsp::smoothing::Smoothed;
use crate::effects::filter::{FilterSlope, FilterStages, stage_qualities};
use crate::unit::{Frame, Kernel, MixLaw, Unit, knob};

const CENTRE_DEAD_ZONE: f32 = 0.02;
const ENGAGE_TRAVEL: f32 = 0.06;
const LOW_PASS_OPEN_HZ: f32 = 22_000.0;
const LOW_PASS_CLOSED_HZ: f32 = 60.0;
const HIGH_PASS_OPEN_HZ: f32 = 12.0;
const HIGH_PASS_CLOSED_HZ: f32 = 9_000.0;
const RESONANCE_TRAVEL: f32 = 0.3;
const POSITION_SMOOTHING_SECONDS: f32 = 0.015;
const RESONANCE_SMOOTHING_SECONDS: f32 = 0.02;
const DEFAULT_RESONANCE: f32 = 0.35;

const LOW_WEIGHTS: [f32; 3] = [1.0, 0.0, 0.0];
const HIGH_WEIGHTS: [f32; 3] = [0.0, 0.0, 1.0];

/// The one-knob filter of a DJ mixer.
///
/// At the centre the music passes untouched, bit for bit. Turning left closes a low-pass from
/// the top down to 60 Hz; turning right opens a high-pass from the bottom up to 9 kHz. Both
/// cut at 24 dB per octave by default, and the resonance grows as the knob leaves the centre,
/// so small moves stay clean and big sweeps sing.
///
/// It is a part of the mixer channel, like the [`crate::Isolator`], so it is always on; moving
/// the knob back to the centre is what takes it out.
pub struct SweepFilter {
    unit: Unit<SweepKernel>,
}

impl SweepFilter {
    /// Builds a sweep filter at the centre.
    pub fn new() -> Self {
        let mut unit = Unit::new(SweepKernel::new(), 1.0);
        unit.set_enabled(true);
        unit.reset();
        Self { unit }
    }

    /// Filters one block of planar stereo audio in place. Real-time safe.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32], context: &ProcessContext) {
        self.unit.process(left, right, context);
    }

    /// Clears the filter's state and moves every control straight to its target.
    pub fn reset(&mut self) {
        self.unit.reset();
    }

    /// Sets the knob, from -1 (low-pass fully closed) through 0 (flat) to 1 (high-pass fully
    /// closed).
    pub fn set_position(&mut self, position: f32) {
        let current = &mut self.unit.kernel.position;
        current.set_target(knob(position, -1.0, 1.0, current.target()));
    }

    /// The knob position the filter is moving to.
    pub fn position(&self) -> f32 {
        self.unit.kernel.position.target()
    }

    /// Sets the resonance reached at the ends of the knob, from 0 to 1.
    pub fn set_resonance(&mut self, resonance: f32) {
        let current = &mut self.unit.kernel.resonance;
        current.set_target(knob(resonance, 0.0, 1.0, current.target()));
    }

    /// Sets how steeply the filter cuts.
    pub fn set_slope(&mut self, slope: FilterSlope) {
        self.unit.kernel.slope = slope;
    }
}

impl Default for SweepFilter {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) struct SweepKernel {
    position: Smoothed,
    resonance: Smoothed,
    slope: FilterSlope,
    low_pass: FilterStages,
    high_pass: FilterStages,
    low_coefficients: (SvfCoefficients, SvfCoefficients),
    high_coefficients: (SvfCoefficients, SvfCoefficients),
    coefficient_inputs: (f32, f32, FilterSlope),
    sample_rate: f32,
}

impl SweepKernel {
    fn new() -> Self {
        let placeholder = SvfCoefficients::new(1_000.0, 0.7, 48_000.0);
        Self {
            position: Smoothed::new(0.0, POSITION_SMOOTHING_SECONDS),
            resonance: Smoothed::new(DEFAULT_RESONANCE, RESONANCE_SMOOTHING_SECONDS),
            slope: FilterSlope::TwentyFourDecibels,
            low_pass: FilterStages::default(),
            high_pass: FilterStages::default(),
            low_coefficients: (placeholder, placeholder),
            high_coefficients: (placeholder, placeholder),
            coefficient_inputs: (f32::NAN, f32::NAN, FilterSlope::TwentyFourDecibels),
            sample_rate: 0.0,
        }
    }

    fn coefficients_for(
        &self,
        cutoff_hz: f32,
        resonance: f32,
    ) -> (SvfCoefficients, SvfCoefficients) {
        let (first_quality, second_quality) = stage_qualities(self.slope, resonance);
        (
            SvfCoefficients::new(cutoff_hz, first_quality, self.sample_rate),
            SvfCoefficients::new(cutoff_hz, second_quality, self.sample_rate),
        )
    }
}

/// How far the knob is from the centre, past the dead zone, from 0 to 1.
fn travel(position: f32) -> f32 {
    ((position.abs() - CENTRE_DEAD_ZONE) / (1.0 - CENTRE_DEAD_ZONE)).clamp(0.0, 1.0)
}

fn exponential(from: f32, to: f32, amount: f32) -> f32 {
    from * (to / from).powf(amount)
}

impl Kernel for SweepKernel {
    const TRAILS: bool = false;
    const MIX_LAW: MixLaw = MixLaw::Crossfade;

    fn begin_block(&mut self, timing: &BlockTiming) {
        if timing.sample_rate != self.sample_rate {
            self.sample_rate = timing.sample_rate;
            self.position.prepare(timing.sample_rate);
            self.resonance.prepare(timing.sample_rate);
            self.coefficient_inputs.0 = f32::NAN;
        }
    }

    #[inline]
    fn tick(&mut self, input: Frame, _beat_position: f64) -> Frame {
        let position = self.position.next();
        let resonance = self.resonance.next();
        let amount = travel(position);
        let low_side = position < 0.0;

        let inputs = (position, resonance, self.slope);
        if inputs != self.coefficient_inputs {
            self.coefficient_inputs = inputs;
            let singing = resonance * smoothstep(amount / RESONANCE_TRAVEL);
            let (low_cutoff, low_resonance) = if low_side {
                (
                    exponential(LOW_PASS_OPEN_HZ, LOW_PASS_CLOSED_HZ, amount),
                    singing,
                )
            } else {
                (LOW_PASS_OPEN_HZ, 0.0)
            };
            let (high_cutoff, high_resonance) = if low_side {
                (HIGH_PASS_OPEN_HZ, 0.0)
            } else {
                (
                    exponential(HIGH_PASS_OPEN_HZ, HIGH_PASS_CLOSED_HZ, amount),
                    singing,
                )
            };
            self.low_coefficients = self.coefficients_for(low_cutoff, low_resonance);
            self.high_coefficients = self.coefficients_for(high_cutoff, high_resonance);
        }

        let engage = smoothstep(amount / ENGAGE_TRAVEL);
        [0, 1].map(|channel| {
            let dry = input[channel];
            let low = self.low_pass.tick(
                channel,
                dry,
                self.slope,
                &self.low_coefficients,
                LOW_WEIGHTS,
            );
            let high = self.high_pass.tick(
                channel,
                dry,
                self.slope,
                &self.high_coefficients,
                HIGH_WEIGHTS,
            );
            if engage == 0.0 {
                dry
            } else {
                let filtered = if low_side { low } else { high };
                dry + (filtered - dry) * engage
            }
        })
    }

    fn reset(&mut self) {
        self.low_pass.reset();
        self.high_pass.reset();
        self.position.snap_to_target();
        self.resonance.snap_to_target();
    }
}
