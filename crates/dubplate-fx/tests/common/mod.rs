//! Offline rendering and measuring helpers shared by the integration tests.

#![allow(dead_code)]

use std::f64::consts::TAU;

use dubplate_fx::{Effect, ProcessContext};

pub const SAMPLE_RATE: f32 = 48_000.0;
pub const BLOCK_FRAMES: usize = 256;

/// Stereo audio as two planar channels.
#[derive(Clone, Debug)]
pub struct Stereo {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
}

impl Stereo {
    pub fn silence(frames: usize) -> Self {
        Self {
            left: vec![0.0; frames],
            right: vec![0.0; frames],
        }
    }

    pub fn mono(samples: Vec<f32>) -> Self {
        Self {
            right: samples.clone(),
            left: samples,
        }
    }

    pub fn frames(&self) -> usize {
        self.left.len()
    }
}

/// Plays music through an effect block by block, keeping the beat position in step.
pub struct Deck {
    pub tempo_bpm: f64,
    pub beat_position: f64,
}

impl Deck {
    pub fn new(tempo_bpm: f64) -> Self {
        Self {
            tempo_bpm,
            beat_position: 0.0,
        }
    }

    /// Processes `audio` in place in blocks of [`BLOCK_FRAMES`].
    pub fn play(&mut self, effect: &mut dyn Effect, audio: &mut Stereo) {
        self.play_range(effect, audio, 0, audio.frames());
    }

    /// Processes frames `start..end` of `audio` in place.
    pub fn play_range(
        &mut self,
        effect: &mut dyn Effect,
        audio: &mut Stereo,
        start: usize,
        end: usize,
    ) {
        let mut block_start = start;
        while block_start < end {
            let block_end = (block_start + BLOCK_FRAMES).min(end);
            let context = ProcessContext::new(SAMPLE_RATE, self.tempo_bpm, self.beat_position);
            effect.process(
                &mut audio.left[block_start..block_end],
                &mut audio.right[block_start..block_end],
                &context,
            );
            let frames = (block_end - block_start) as f64;
            self.beat_position += frames * self.tempo_bpm / 60.0 / f64::from(SAMPLE_RATE);
            block_start = block_end;
        }
    }
}

pub fn seconds_to_frames(seconds: f32) -> usize {
    (seconds * SAMPLE_RATE).round() as usize
}

pub fn sine(frequency: f32, amplitude: f32, frames: usize) -> Vec<f32> {
    let cycles_per_frame = f64::from(frequency) / f64::from(SAMPLE_RATE);
    (0..frames)
        .map(|frame_index| {
            let cycles = (frame_index as f64 * cycles_per_frame).fract();
            amplitude * (TAU * cycles).sin() as f32
        })
        .collect()
}

/// Deterministic white noise from -amplitude to amplitude.
pub fn noise(amplitude: f32, frames: usize, seed: u64) -> Vec<f32> {
    let mut state = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
    (0..frames)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let unit = (state >> 40) as f32 / (1_u64 << 24) as f32;
            amplitude * (2.0 * unit - 1.0)
        })
        .collect()
}

pub fn rms(samples: &[f32]) -> f32 {
    let energy: f64 = samples
        .iter()
        .map(|sample| f64::from(*sample).powi(2))
        .sum();
    (energy / samples.len() as f64).sqrt() as f32
}

pub fn decibels(ratio: f32) -> f32 {
    20.0 * ratio.log10()
}

pub fn peak(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0, |loudest, sample| loudest.max(sample.abs()))
}

/// The index of the loudest sample.
pub fn loudest_index(samples: &[f32]) -> usize {
    samples
        .iter()
        .enumerate()
        .fold(
            (0, 0.0_f32),
            |(best_index, best), (sample_index, sample)| {
                if sample.abs() > best {
                    (sample_index, sample.abs())
                } else {
                    (best_index, best)
                }
            },
        )
        .0
}

/// The largest jump in slope between neighbouring samples. A click shows up here as a spike
/// far above what smooth music produces.
pub fn largest_kink(samples: &[f32]) -> f32 {
    samples
        .windows(3)
        .map(|window| (window[2] - 2.0 * window[1] + window[0]).abs())
        .fold(0.0, f32::max)
}
