use std::fmt;

use crate::dsp::delay_line::DelayLine;
use crate::dsp::filters::{Svf, SvfCoefficients, integrator_gain};
use crate::dsp::shaping::{flush_denormal, sanitize_input};
use crate::dsp::smoothing::Smoothed;

/// Where the low band hands over to the mid band, in hertz.
pub const LOW_CROSSOVER_HZ: f32 = 250.0;
/// Where the mid band hands over to the high band, in hertz.
pub const HIGH_CROSSOVER_HZ: f32 = 2_500.0;
/// The highest gain a band reaches: +6 dB, as on a club mixer.
pub const MAX_BAND_GAIN: f32 = 1.995_262_3;

const GAIN_SMOOTHING_SECONDS: f32 = 0.006;
const LOWEST_SAMPLE_RATE: f32 = 8_000.0;

/// Frequency scale and quality factor of each two-pole section of a fourth-order Bessel
/// low-pass, normalised to -3 dB at the cutoff.
const BESSEL_SECTIONS: [(f32, f32); 2] = [(1.4192, 0.5219), (1.5912, 0.8055)];

/// One band of an [`Isolator`] or of a mixer channel's EQ.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EqBand {
    /// Below 250 Hz: kick and bass.
    Low,
    /// 250 Hz to 2.5 kHz: vocals, snares, chords.
    Mid,
    /// Above 2.5 kHz: hats, cymbals, air.
    High,
}

impl EqBand {
    /// Every band, from the top of a mixer channel down, as the knobs are laid out.
    pub const ALL: [EqBand; 3] = [EqBand::High, EqBand::Mid, EqBand::Low];

    /// Position of the band in [`EqBand::ALL`].
    pub fn index(self) -> usize {
        match self {
            EqBand::High => 0,
            EqBand::Mid => 1,
            EqBand::Low => 2,
        }
    }
}

impl fmt::Display for EqBand {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            EqBand::High => "high",
            EqBand::Mid => "mid",
            EqBand::Low => "low",
        })
    }
}

/// How a split builds its low side `L` from a Bessel low-pass `H` and a delay `D` matching
/// `H`'s delay at low frequencies. The high side is always what is left, `D^n - L`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Complement {
    /// `L = H (2D - H)`, so the high side is `(D - H)^2`: twice the low-side kill depth of a
    /// plain subtraction, at twice the latency.
    Squared,
    /// `L = H^2 (3D - 2H)`, so the high side is `(D - H)^2 (D + 2H)`: both sides cut twice as
    /// steeply as `H` alone, at three times the latency.
    Cubic,
}

impl Complement {
    fn latency_factor(self) -> usize {
        match self {
            Complement::Squared => 2,
            Complement::Cubic => 3,
        }
    }
}

/// The coefficients of one phase-coherent split.
#[derive(Debug, Clone, Copy)]
struct SplitDesign {
    sections: [SvfCoefficients; 2],
    delay: usize,
    complement: Complement,
}

impl SplitDesign {
    /// A Bessel low-pass near `cutoff_hz`, nudged so its delay at low frequencies is a whole
    /// number of samples, which a plain delay line then matches exactly.
    fn new(cutoff_hz: f32, sample_rate: f32, complement: Complement) -> Self {
        let delay = bessel_delay_samples(cutoff_hz, sample_rate)
            .round()
            .max(1.0);
        let mut lower = cutoff_hz * 0.5;
        let mut upper = cutoff_hz * 2.0;
        for _ in 0..60 {
            let middle = (lower + upper) * 0.5;
            if bessel_delay_samples(middle, sample_rate) > delay {
                lower = middle;
            } else {
                upper = middle;
            }
        }
        let tuned_hz = (lower + upper) * 0.5;
        Self {
            sections: BESSEL_SECTIONS.map(|(scale, quality)| {
                SvfCoefficients::new(tuned_hz * scale, quality, sample_rate)
            }),
            delay: delay as usize,
            complement,
        }
    }

    fn latency(&self) -> usize {
        self.delay * self.complement.latency_factor()
    }
}

/// The delay at DC, in samples, of the trapezoidal fourth-order Bessel low-pass at
/// `cutoff_hz`. Each two-pole section delays DC by `1 / (2 g Q)` samples.
fn bessel_delay_samples(cutoff_hz: f32, sample_rate: f32) -> f32 {
    BESSEL_SECTIONS
        .iter()
        .map(|(scale, quality)| {
            1.0 / (2.0 * integrator_gain(cutoff_hz * scale, sample_rate) * quality)
        })
        .sum()
}

/// One cascade of the two Bessel sections.
#[derive(Debug, Clone, Copy, Default)]
struct BesselLowPass {
    sections: [Svf; 2],
}

impl BesselLowPass {
    #[inline]
    fn tick(&mut self, input: f32, design: &SplitDesign) -> f32 {
        let first = self.sections[0].tick(input, &design.sections[0]).low;
        self.sections[1].tick(first, &design.sections[1]).low
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

/// The running state of one split on one channel.
#[derive(Debug, Clone)]
struct SplitState {
    stages: [BesselLowPass; 3],
    delay_line: DelayLine,
}

impl SplitState {
    fn new(design: &SplitDesign) -> Self {
        Self {
            stages: [BesselLowPass::default(); 3],
            delay_line: DelayLine::new(design.delay),
        }
    }

    /// The split's low side for one sample, delayed by the split's latency.
    #[inline]
    fn low(&mut self, input: f32, design: &SplitDesign) -> f32 {
        let [first_stage, second_stage, third_stage] = &mut self.stages;
        let filtered = first_stage.tick(input, design);
        match design.complement {
            Complement::Squared => {
                let delayed = self.delay_line.delay_whole(input, design.delay);
                second_stage.tick(2.0 * delayed - filtered, design)
            }
            Complement::Cubic => {
                let twice = second_stage.tick(filtered, design);
                let thrice = third_stage.tick(twice, design);
                let delayed = self.delay_line.delay_whole(twice, design.delay);
                3.0 * delayed - 2.0 * thrice
            }
        }
    }

    fn reset(&mut self) {
        for stage in &mut self.stages {
            stage.reset();
        }
        self.delay_line.clear();
    }
}

/// Everything one channel of an [`Isolator`] keeps between samples.
#[derive(Debug, Clone)]
struct ChannelState {
    low_split: SplitState,
    high_split: SplitState,
    dry_delay: DelayLine,
    band_delay: DelayLine,
    low_delay: DelayLine,
}

/// The three-band isolator EQ of a DJ mixer channel.
///
/// At unity the music comes out unchanged, only delayed by [`Isolator::latency_frames`]. A band
/// at zero is killed: more than 65 dB down at 60 Hz for the low, 45 dB at 8 kHz for the high.
/// A band reaches +6 dB at [`MAX_BAND_GAIN`]. Gain changes glide over a few milliseconds, so
/// turning or flicking a knob never clicks.
///
/// The bands come from Bessel low-passes subtracted from a delayed copy of the music. A Bessel
/// filter delays everything below its cutoff by the same time, so the bands stay in phase with
/// each other and with the music: killing the low leaves a kick's attack as sharp as it was,
/// where a Linkwitz-Riley crossover rotates the phase and rounds it off. Each split is used
/// squared or cubed, which deepens the kill well past a plain subtraction.
#[derive(Debug, Clone)]
pub struct Isolator {
    layout: Layout,
    channels: [ChannelState; 2],
    gains: [Smoothed; 3],
}

/// The fixed design of an isolator at one sample rate.
#[derive(Debug, Clone, Copy)]
struct Layout {
    low_design: SplitDesign,
    high_design: SplitDesign,
    latency: usize,
    band_delay: usize,
    low_delay: usize,
}

impl Layout {
    #[inline]
    fn split_sample(&self, state: &mut ChannelState, input: f32, weights: BandWeights) -> f32 {
        let input = sanitize_input(input);
        let low = state.low_split.low(input, &self.low_design);
        let band = state.high_split.low(input, &self.high_design);
        let dry = state.dry_delay.delay_whole(input, self.latency);
        let band = state.band_delay.delay_whole(band, self.band_delay);
        let low = state.low_delay.delay_whole(low, self.low_delay);
        flush_denormal(dry * weights.dry + band * weights.band + low * weights.low)
    }
}

impl Isolator {
    /// Builds an isolator at unity for `sample_rate`. This allocates its delay lines, so it
    /// runs off the audio thread.
    pub fn new(sample_rate: f32) -> Self {
        let sample_rate = if sample_rate.is_finite() {
            sample_rate.max(LOWEST_SAMPLE_RATE)
        } else {
            LOWEST_SAMPLE_RATE
        };
        let low_design = SplitDesign::new(LOW_CROSSOVER_HZ, sample_rate, Complement::Squared);
        let high_design = SplitDesign::new(HIGH_CROSSOVER_HZ, sample_rate, Complement::Cubic);
        let latency = low_design.latency().max(high_design.latency());
        let band_delay = latency - high_design.latency();
        let low_delay = latency - low_design.latency();
        let channel = ChannelState {
            low_split: SplitState::new(&low_design),
            high_split: SplitState::new(&high_design),
            dry_delay: DelayLine::new(latency),
            band_delay: DelayLine::new(band_delay),
            low_delay: DelayLine::new(low_delay),
        };
        let gains = std::array::from_fn(|_| {
            let mut gain = Smoothed::new(1.0, GAIN_SMOOTHING_SECONDS);
            gain.prepare(sample_rate);
            gain
        });
        Self {
            layout: Layout {
                low_design,
                high_design,
                latency,
                band_delay,
                low_delay,
            },
            channels: [channel.clone(), channel],
            gains,
        }
    }

    /// How many frames the isolator delays the music by, whatever the gains.
    pub fn latency_frames(&self) -> usize {
        self.layout.latency
    }

    /// Sets a band's gain, a linear factor from 0 (killed) to [`MAX_BAND_GAIN`].
    pub fn set_gain(&mut self, band: EqBand, gain: f32) {
        if gain.is_finite() {
            self.gains[band.index()].set_target(gain.clamp(0.0, MAX_BAND_GAIN));
        }
    }

    /// The gain a band is moving to.
    pub fn gain(&self, band: EqBand) -> f32 {
        self.gains[band.index()].target()
    }

    /// Moves every gain straight to its target and clears the filters.
    pub fn reset(&mut self) {
        for gain in &mut self.gains {
            gain.snap_to_target();
        }
        for channel in &mut self.channels {
            channel.low_split.reset();
            channel.high_split.reset();
            channel.dry_delay.clear();
            channel.band_delay.clear();
            channel.low_delay.clear();
        }
    }

    /// Processes a block of stereo audio in place. Never allocates.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let layout = self.layout;
        let [high_gain, mid_gain, low_gain] = &mut self.gains;
        let [left_state, right_state] = &mut self.channels;
        for (left_sample, right_sample) in left.iter_mut().zip(right.iter_mut()) {
            let high = high_gain.next();
            let mid = mid_gain.next();
            let low = low_gain.next();
            let weights = BandWeights {
                dry: high,
                band: mid - high,
                low: low - mid,
            };
            *left_sample = layout.split_sample(left_state, *left_sample, weights);
            *right_sample = layout.split_sample(right_state, *right_sample, weights);
        }
    }
}

/// How much of the delayed music, of everything below the high crossover and of everything
/// below the low crossover makes up the output. At unity only the delayed music is left.
#[derive(Debug, Clone, Copy)]
struct BandWeights {
    dry: f32,
    band: f32,
    low: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE: f32 = 48_000.0;

    fn sine_level_db(isolator: &mut Isolator, frequency: f32) -> f32 {
        let frames = 48_000;
        let mut left: Vec<f32> = (0..frames)
            .map(|index| (std::f32::consts::TAU * frequency * index as f32 / SAMPLE_RATE).sin())
            .collect();
        let mut right = left.clone();
        isolator.process(&mut left, &mut right);
        let settled = &left[frames / 2..];
        let peak = settled
            .iter()
            .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
        20.0 * peak.max(1e-12).log10()
    }

    fn isolator_with(low: f32, mid: f32, high: f32) -> Isolator {
        let mut isolator = Isolator::new(SAMPLE_RATE);
        isolator.set_gain(EqBand::Low, low);
        isolator.set_gain(EqBand::Mid, mid);
        isolator.set_gain(EqBand::High, high);
        isolator.reset();
        isolator
    }

    #[test]
    fn unity_is_the_music_delayed_bit_for_bit() {
        let mut isolator = Isolator::new(SAMPLE_RATE);
        let latency = isolator.latency_frames();
        assert!(latency > 0 && latency < 200, "latency {latency}");
        let input: Vec<f32> = (0..4_000)
            .map(|index| ((index * 7_919) % 1_000) as f32 / 500.0 - 1.0)
            .collect();
        let mut left = input.clone();
        let mut right = input.clone();
        isolator.process(&mut left, &mut right);
        assert_eq!(&left[latency..], &input[..input.len() - latency]);
        assert_eq!(left, right);
    }

    #[test]
    fn killing_a_band_removes_it_and_leaves_the_others() {
        let mut low_killed = isolator_with(0.0, 1.0, 1.0);
        assert!(sine_level_db(&mut low_killed, 60.0) < -60.0);
        let mut low_killed = isolator_with(0.0, 1.0, 1.0);
        assert!(sine_level_db(&mut low_killed, 5_000.0).abs() < 0.5);
        let mut mid_killed = isolator_with(1.0, 0.0, 1.0);
        assert!(sine_level_db(&mut mid_killed, 1_000.0) < -25.0);
        let mut high_killed = isolator_with(1.0, 1.0, 0.0);
        assert!(sine_level_db(&mut high_killed, 8_000.0) < -40.0);
        let mut high_killed = isolator_with(1.0, 1.0, 0.0);
        assert!(sine_level_db(&mut high_killed, 80.0).abs() < 0.5);
    }

    #[test]
    fn a_full_boost_adds_six_decibels() {
        let mut boosted = isolator_with(MAX_BAND_GAIN, 1.0, 1.0);
        assert!((sine_level_db(&mut boosted, 50.0) - 6.0).abs() < 0.3);
    }

    #[test]
    fn delays_are_whole_samples_at_common_rates() {
        for sample_rate in [44_100.0, 48_000.0, 96_000.0, 192_000.0] {
            let isolator = Isolator::new(sample_rate);
            let milliseconds = isolator.latency_frames() as f32 / sample_rate * 1_000.0;
            assert!(
                (2.0..3.5).contains(&milliseconds),
                "{sample_rate} Hz: {milliseconds} ms"
            );
        }
    }

    #[test]
    fn a_kill_glides_instead_of_stepping() {
        let mut isolator = Isolator::new(SAMPLE_RATE);
        let mut left = vec![0.5_f32; 2_000];
        let mut right = left.clone();
        isolator.process(&mut left, &mut right);
        isolator.set_gain(EqBand::Low, 0.0);
        let mut left = vec![0.5_f32; 2_000];
        let mut right = left.clone();
        isolator.process(&mut left, &mut right);
        let largest_step = left
            .windows(2)
            .map(|pair| (pair[1] - pair[0]).abs())
            .fold(0.0, f32::max);
        assert!(largest_step < 0.01, "step {largest_step}");
    }
}
