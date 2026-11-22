//! How loud a track sounds over its whole length, as ITU-R BS.1770-4 measures it: K-weighted,
//! in 400 ms blocks overlapping by three quarters, gated at -70 LUFS and then 10 LU under the
//! loudness of what passed. Auto Gain levels folder tracks with it.

use euphoria_core::AudioBuffer;

/// How far one gating block starts after the previous one, in seconds: a quarter of a block.
const STEP_SECONDS: f64 = 0.1;
/// A gating block, 400 ms, is four steps.
const STEPS_PER_BLOCK: usize = 4;
/// Blocks quieter than this never count.
const ABSOLUTE_GATE_LUFS: f64 = -70.0;
/// Blocks this far under the loudness of the blocks over the absolute gate do not count.
const RELATIVE_GATE_LU: f64 = 10.0;
/// BS.1770's offset from the mean square of the K-weighted signal to LUFS.
const LOUDNESS_OFFSET: f64 = -0.691;
/// A mono mixdown counted on both channels, left and right alike, as a stereo track's
/// loudness counts both: twice the power, 3.01 dB.
const BOTH_CHANNELS_DB: f64 = 3.010_299_956_639_812;

/// A loudness in LUFS.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Lufs(pub f32);

/// The integrated loudness of a track's mono mixdown, counted on both channels, or `None` for
/// a track with no block over the absolute gate, such as silence.
pub fn integrated_loudness(audio: &AudioBuffer) -> Option<Lufs> {
    let sample_rate = f64::from(audio.sample_rate().get());
    let step = (STEP_SECONDS * sample_rate).round() as usize;
    if step == 0 {
        return None;
    }
    let mut weighting = KWeighting::new(sample_rate);
    let step_energies: Vec<f64> = audio
        .samples()
        .chunks_exact(step)
        .map(|chunk| {
            chunk
                .iter()
                .map(|sample| {
                    let weighted = weighting.process(f64::from(*sample));
                    weighted * weighted
                })
                .sum()
        })
        .collect();
    let block_samples = (step * STEPS_PER_BLOCK) as f64;
    let block_powers: Vec<f64> = step_energies
        .windows(STEPS_PER_BLOCK)
        .map(|steps| steps.iter().sum::<f64>() / block_samples)
        .collect();
    let loudness_of = |power: f64| LOUDNESS_OFFSET + 10.0 * power.log10() + BOTH_CHANNELS_DB;
    let mean_power = |powers: &mut dyn Iterator<Item = f64>| {
        let (sum, count) = powers.fold((0.0, 0_usize), |(sum, count), power| {
            (sum + power, count + 1)
        });
        (count > 0).then(|| sum / count as f64)
    };
    let over_absolute = |power: &&f64| loudness_of(**power) > ABSOLUTE_GATE_LUFS;
    let ungated = mean_power(&mut block_powers.iter().filter(over_absolute).copied())?;
    let relative_gate = loudness_of(ungated) - RELATIVE_GATE_LU;
    let gated = mean_power(
        &mut block_powers
            .iter()
            .filter(over_absolute)
            .filter(|power| loudness_of(**power) > relative_gate)
            .copied(),
    )?;
    Some(Lufs(loudness_of(gated) as f32))
}

/// BS.1770's K-weighting: a high shelf for the head, then a high pass, each a biquad designed
/// for the sample rate, as libebur128 designs them.
struct KWeighting {
    shelf: Biquad,
    high_pass: Biquad,
}

impl KWeighting {
    fn new(sample_rate: f64) -> Self {
        let shelf_frequency = 1_681.974_450_955_533;
        let shelf_gain_db = 3.999_843_853_973_347;
        let shelf_q = 0.707_175_236_955_419_6;
        let k = (std::f64::consts::PI * shelf_frequency / sample_rate).tan();
        let high_gain = 10.0_f64.powf(shelf_gain_db / 20.0);
        let band_gain = high_gain.powf(0.499_666_774_154_541_6);
        let denominator = 1.0 + k / shelf_q + k * k;
        let shelf = Biquad::new(
            [
                (high_gain + band_gain * k / shelf_q + k * k) / denominator,
                2.0 * (k * k - high_gain) / denominator,
                (high_gain - band_gain * k / shelf_q + k * k) / denominator,
            ],
            [
                2.0 * (k * k - 1.0) / denominator,
                (1.0 - k / shelf_q + k * k) / denominator,
            ],
        );
        let pass_frequency = 38.135_470_876_024_44;
        let pass_q = 0.500_327_037_323_877_3;
        let k = (std::f64::consts::PI * pass_frequency / sample_rate).tan();
        let denominator = 1.0 + k / pass_q + k * k;
        let high_pass = Biquad::new(
            [1.0, -2.0, 1.0],
            [
                2.0 * (k * k - 1.0) / denominator,
                (1.0 - k / pass_q + k * k) / denominator,
            ],
        );
        Self { shelf, high_pass }
    }

    fn process(&mut self, sample: f64) -> f64 {
        self.high_pass.process(self.shelf.process(sample))
    }
}

/// A biquad in direct form I, `a0` normalised to 1.
struct Biquad {
    feedforward: [f64; 3],
    feedback: [f64; 2],
    inputs: [f64; 2],
    outputs: [f64; 2],
}

impl Biquad {
    fn new(feedforward: [f64; 3], feedback: [f64; 2]) -> Self {
        Self {
            feedforward,
            feedback,
            inputs: [0.0; 2],
            outputs: [0.0; 2],
        }
    }

    fn process(&mut self, input: f64) -> f64 {
        let [b0, b1, b2] = self.feedforward;
        let [a1, a2] = self.feedback;
        let output = b0 * input + b1 * self.inputs[0] + b2 * self.inputs[1]
            - a1 * self.outputs[0]
            - a2 * self.outputs[1];
        self.inputs = [input, self.inputs[0]];
        self.outputs = [output, self.outputs[0]];
        output
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::*;

    const RATE: u32 = 48_000;

    fn sine(frequency: f64, amplitude: f64, seconds: f64) -> Vec<f32> {
        let count = (seconds * f64::from(RATE)) as usize;
        (0..count)
            .map(|index| {
                let time = index as f64 / f64::from(RATE);
                (amplitude * (std::f64::consts::TAU * frequency * time).sin()) as f32
            })
            .collect()
    }

    fn buffer(samples: Vec<f32>) -> AudioBuffer {
        AudioBuffer::new(samples, NonZeroU32::new(RATE).unwrap())
    }

    #[test]
    fn the_k_weighting_matches_the_standard_at_48_khz() {
        let weighting = KWeighting::new(48_000.0);
        let close = |found: f64, expected: f64| (found - expected).abs() < 1e-8;
        let [b0, b1, b2] = weighting.shelf.feedforward;
        let [a1, a2] = weighting.shelf.feedback;
        assert!(close(b0, 1.535_124_859_586_97), "{b0}");
        assert!(close(b1, -2.691_696_189_406_38), "{b1}");
        assert!(close(b2, 1.198_392_810_852_85), "{b2}");
        assert!(close(a1, -1.690_659_293_182_41), "{a1}");
        assert!(close(a2, 0.732_480_774_215_85), "{a2}");
        let [a1, a2] = weighting.high_pass.feedback;
        assert!(close(a1, -1.990_047_454_833_98), "{a1}");
        assert!(close(a2, 0.990_072_250_366_21), "{a2}");
    }

    #[test]
    fn a_full_scale_1_khz_sine_on_both_channels_reads_as_bs_1770_says() {
        // BS.1770's check: a 0 dBFS 997 Hz sine on one channel reads -3.01 LUFS, so on both
        // channels it reads 0.
        let loudness = integrated_loudness(&buffer(sine(997.0, 1.0, 10.0))).unwrap();
        assert!((loudness.0 - 0.0).abs() < 0.05, "{loudness:?}");
    }

    #[test]
    fn twenty_db_quieter_reads_twenty_lu_lower() {
        let loud = integrated_loudness(&buffer(sine(997.0, 0.5, 10.0))).unwrap();
        let quiet = integrated_loudness(&buffer(sine(997.0, 0.05, 10.0))).unwrap();
        assert!((loud.0 - quiet.0 - 20.0).abs() < 0.01, "{loud:?} {quiet:?}");
    }

    #[test]
    fn silence_and_a_quiet_tail_do_not_pull_the_loudness_down() {
        assert_eq!(integrated_loudness(&buffer(vec![0.0; 48_000 * 5])), None);
        let mut with_silence = sine(997.0, 0.5, 10.0);
        with_silence.extend(std::iter::repeat_n(0.0, 48_000 * 20));
        let gated = integrated_loudness(&buffer(with_silence)).unwrap();
        let tone = integrated_loudness(&buffer(sine(997.0, 0.5, 10.0))).unwrap();
        assert!((gated.0 - tone.0).abs() < 0.05, "{gated:?} {tone:?}");
    }

    #[test]
    fn a_track_shorter_than_a_block_has_no_loudness() {
        assert_eq!(integrated_loudness(&buffer(sine(997.0, 0.5, 0.2))), None);
    }
}
