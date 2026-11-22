use crate::dsp::shaping::sanitize_input;

/// The level the limiter never lets a sample pass: -0.3 dBFS.
pub const LIMITER_CEILING: f32 = 0.966_051;

const DEFAULT_LOOKAHEAD_SECONDS: f32 = 0.0015;
const DEFAULT_HOLD_SECONDS: f32 = 0.02;
const DEFAULT_RELEASE_SECONDS: f32 = 0.12;
const LOWEST_SAMPLE_RATE: f32 = 8_000.0;

/// How a [`Limiter`] reacts: how far ahead it looks and how fast it lets go.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LimiterTiming {
    /// How far ahead the limiter sees a peak coming, which is also the delay it adds and the
    /// time it takes to turn down for a peak, in seconds.
    pub lookahead_seconds: f32,
    /// How long the gain stays down after a peak before it starts to rise, in seconds. Longer
    /// than half a bass cycle keeps the gain steady through it, so limiting never distorts the
    /// bass, and it adds no delay.
    pub hold_seconds: f32,
    /// How long the gain takes to come most of the way back up after the hold, in seconds.
    pub release_seconds: f32,
}

impl Default for LimiterTiming {
    fn default() -> Self {
        Self {
            lookahead_seconds: DEFAULT_LOOKAHEAD_SECONDS,
            hold_seconds: DEFAULT_HOLD_SECONDS,
            release_seconds: DEFAULT_RELEASE_SECONDS,
        }
    }
}

/// The smallest value over the last `length` pushed, in constant time per push, without
/// allocating after construction.
#[derive(Debug, Clone)]
struct SlidingMinimum {
    values: Vec<f32>,
    indices: Vec<usize>,
    head: usize,
    count: usize,
    length: usize,
    pushed: usize,
}

impl SlidingMinimum {
    fn new(length: usize) -> Self {
        Self {
            values: vec![0.0; length],
            indices: vec![0; length],
            head: 0,
            count: 0,
            length,
            pushed: 0,
        }
    }

    fn slot(&self, offset: usize) -> usize {
        (self.head + offset) % self.length
    }

    fn push(&mut self, value: f32) -> f32 {
        while self.count > 0 {
            let last = self.slot(self.count - 1);
            if self.values[last] >= value {
                self.count -= 1;
            } else {
                break;
            }
        }
        let back = self.slot(self.count);
        self.values[back] = value;
        self.indices[back] = self.pushed;
        self.count += 1;
        while self.indices[self.head] + self.length <= self.pushed {
            self.head = (self.head + 1) % self.length;
            self.count -= 1;
        }
        self.pushed += 1;
        self.values[self.head]
    }

    fn reset(&mut self) {
        self.head = 0;
        self.count = 0;
        self.pushed = 0;
    }
}

/// A running average over the last `length` values, kept exact by summing in `f64`.
#[derive(Debug, Clone)]
struct BoxAverage {
    values: Vec<f32>,
    sum: f64,
    position: usize,
}

impl BoxAverage {
    fn new(length: usize) -> Self {
        Self {
            values: vec![1.0; length],
            sum: length as f64,
            position: 0,
        }
    }

    fn push(&mut self, value: f32) -> f32 {
        self.sum += f64::from(value) - f64::from(self.values[self.position]);
        self.values[self.position] = value;
        self.position = (self.position + 1) % self.values.len();
        (self.sum / self.values.len() as f64) as f32
    }

    fn reset(&mut self) {
        self.values.fill(1.0);
        self.sum = self.values.len() as f64;
        self.position = 0;
    }
}

/// A transparent look-ahead limiter that never lets a sample past [`LIMITER_CEILING`].
///
/// It delays the music by its look-ahead and works out, for every sample, the gain that keeps
/// it under the ceiling. The gain falls over the whole look-ahead before a peak arrives, as the
/// average of the lowest gain needed anywhere in the window, so it reaches the gain the peak
/// needs exactly when the peak plays and never steps; it holds there for a bass cycle and then
/// rises back along the release.
/// Both channels share one gain, so the stereo image never shifts. Music under the ceiling
/// passes untouched, only delayed.
#[derive(Debug, Clone)]
pub struct Limiter {
    minimum: SlidingMinimum,
    average: BoxAverage,
    delay_left: Vec<f32>,
    delay_right: Vec<f32>,
    delay_position: usize,
    release_coefficient: f32,
    released_gain: f32,
    gain_reduction: f32,
}

impl Limiter {
    /// Builds a limiter for `sample_rate` with the default timing: 1.5 ms of look-ahead, a
    /// 20 ms hold and 120 ms of release. This allocates, so it runs off the audio thread.
    pub fn new(sample_rate: f32) -> Self {
        Self::with_timing(sample_rate, LimiterTiming::default())
    }

    /// Builds a limiter for `sample_rate` with its own timing.
    pub fn with_timing(sample_rate: f32, timing: LimiterTiming) -> Self {
        let sample_rate = if sample_rate.is_finite() {
            sample_rate.max(LOWEST_SAMPLE_RATE)
        } else {
            LOWEST_SAMPLE_RATE
        };
        let lookahead = (timing.lookahead_seconds.max(0.0) * sample_rate)
            .round()
            .max(1.0) as usize;
        let hold = (timing.hold_seconds.max(0.0) * sample_rate).round() as usize;
        let release_frames = (timing.release_seconds.max(0.001) * sample_rate).max(1.0);
        Self {
            minimum: SlidingMinimum::new(lookahead + hold + 1),
            average: BoxAverage::new(lookahead + 1),
            delay_left: vec![0.0; lookahead],
            delay_right: vec![0.0; lookahead],
            delay_position: 0,
            release_coefficient: 1.0 - (-1.0 / release_frames).exp(),
            released_gain: 1.0,
            gain_reduction: 1.0,
        }
    }

    /// How many frames the limiter delays the music by.
    pub fn latency_frames(&self) -> usize {
        self.delay_left.len()
    }

    /// The lowest gain applied since the previous call, where 1.0 means the limiter did nothing.
    pub fn take_gain_reduction(&mut self) -> f32 {
        std::mem::replace(&mut self.gain_reduction, 1.0)
    }

    /// Clears the limiter's memory of the music.
    pub fn reset(&mut self) {
        self.minimum.reset();
        self.average.reset();
        self.delay_left.fill(0.0);
        self.delay_right.fill(0.0);
        self.delay_position = 0;
        self.released_gain = 1.0;
        self.gain_reduction = 1.0;
    }

    /// Limits a block of stereo audio in place. Never allocates.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (left_sample, right_sample) in left.iter_mut().zip(right.iter_mut()) {
            let incoming_left = sanitize_input(*left_sample);
            let incoming_right = sanitize_input(*right_sample);
            let peak = incoming_left.abs().max(incoming_right.abs());
            let needed = if peak > LIMITER_CEILING {
                LIMITER_CEILING / peak
            } else {
                1.0
            };
            let lowest_ahead = self.minimum.push(needed);
            self.released_gain = if lowest_ahead < self.released_gain {
                lowest_ahead
            } else {
                self.released_gain + (lowest_ahead - self.released_gain) * self.release_coefficient
            };
            let gain = self.average.push(self.released_gain);
            self.gain_reduction = self.gain_reduction.min(gain);
            let outgoing_left =
                std::mem::replace(&mut self.delay_left[self.delay_position], incoming_left);
            let outgoing_right =
                std::mem::replace(&mut self.delay_right[self.delay_position], incoming_right);
            self.delay_position = (self.delay_position + 1) % self.delay_left.len();
            *left_sample = (outgoing_left * gain).clamp(-LIMITER_CEILING, LIMITER_CEILING);
            *right_sample = (outgoing_right * gain).clamp(-LIMITER_CEILING, LIMITER_CEILING);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE: f32 = 48_000.0;

    fn limited(input: &[f32]) -> (Vec<f32>, usize) {
        let mut limiter = Limiter::new(SAMPLE_RATE);
        let mut left = input.to_vec();
        let mut right = input.to_vec();
        limiter.process(&mut left, &mut right);
        assert_eq!(left, right);
        (left, limiter.latency_frames())
    }

    #[test]
    fn music_under_the_ceiling_passes_untouched_only_delayed() {
        let input: Vec<f32> = (0..9_600)
            .map(|index| ((index * 7_919) % 1_000) as f32 / 1_100.0 - 0.45)
            .collect();
        let (output, latency) = limited(&input);
        assert_eq!(latency, 72);
        assert_eq!(&output[latency..], &input[..input.len() - latency]);
    }

    #[test]
    fn no_sample_ever_passes_the_ceiling() {
        let input: Vec<f32> = (0..48_000)
            .map(|index| {
                let burst = if (index / 3_000) % 2 == 0 { 4.0 } else { 0.3 };
                (index as f32 * 0.05).sin() * burst + if index % 997 == 0 { 9.0 } else { 0.0 }
            })
            .collect();
        let (output, _) = limited(&input);
        assert!(output.iter().all(|sample| sample.abs() <= LIMITER_CEILING));
    }

    #[test]
    fn a_steady_bass_note_over_the_ceiling_is_turned_down_without_distortion() {
        let input: Vec<f32> = (0..96_000)
            .map(|index| (std::f32::consts::TAU * 40.0 * index as f32 / SAMPLE_RATE).sin() * 2.0)
            .collect();
        let (output, latency) = limited(&input);
        let settled = &output[48_000..];
        let source = &input[48_000 - latency..input.len() - latency];
        let scale: f64 = settled
            .iter()
            .zip(source)
            .map(|(out, inp)| f64::from(*out) * f64::from(*inp))
            .sum::<f64>()
            / source
                .iter()
                .map(|inp| f64::from(*inp).powi(2))
                .sum::<f64>();
        let residual: f64 = settled
            .iter()
            .zip(source)
            .map(|(out, inp)| (f64::from(*out) - scale * f64::from(*inp)).powi(2))
            .sum();
        let energy: f64 = settled.iter().map(|out| f64::from(*out).powi(2)).sum();
        assert!(10.0 * (residual / energy).log10() < -100.0);
        assert!((scale * 2.0 - f64::from(LIMITER_CEILING)).abs() < 1e-3);
    }

    #[test]
    fn the_gain_comes_back_after_a_peak() {
        let mut limiter = Limiter::new(SAMPLE_RATE);
        let mut left = vec![0.5_f32; 24_000];
        left[1_000] = 3.0;
        let mut right = left.clone();
        limiter.process(&mut left, &mut right);
        assert!(limiter.take_gain_reduction() < 0.4);
        let tail = &left[23_000..];
        assert!(tail.iter().all(|sample| (sample - 0.5).abs() < 0.015));
        assert_eq!(limiter.take_gain_reduction(), 1.0);
    }
}
