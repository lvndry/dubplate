//! Sounds Dubplate makes itself for the sampler, as rekordbox's OSC sampler does, so a DJ has an
//! air horn, a dub siren, a riser and a log drum without a sample pack. Each is computed from
//! oscillators and seeded noise, the same on every launch.

use std::f64::consts::TAU;

use crate::error::EngineError;
use crate::sampler::{SampleMode, SampleSettings};
use crate::track::Track;

/// The tempo the tempo-locked sounds are made at: the riser lasts 8 beats and the siren's loop
/// 2 beats at 120 BPM, and a synced slot stretches them to the master's tempo.
pub const SYNTH_BPM: f64 = 120.0;

/// Where every sound peaks, about -3 dBFS, so a sample on top of a loud mix leaves the master
/// limiter room.
const PEAK: f64 = 0.7;

/// A sound the sampler can make without a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SynthSound {
    /// Three short blasts and a long one, the dancehall and afro house air horn.
    AirHorn,
    /// A dub siren wailing up and down twice a bar, looping seamlessly.
    Siren,
    /// Noise and a tone rising for 8 beats into a drop.
    Riser,
    /// The pitched, woody bass hit of amapiano.
    LogDrum,
}

impl SynthSound {
    /// Every sound, in the order the sampler lists them.
    pub const ALL: [Self; 4] = [Self::AirHorn, Self::Siren, Self::Riser, Self::LogDrum];

    /// The sound's name as the pads show it.
    pub fn name(self) -> &'static str {
        match self {
            Self::AirHorn => "Air horn",
            Self::Siren => "Siren",
            Self::Riser => "Riser",
            Self::LogDrum => "Log drum",
        }
    }

    /// The name settings store it under.
    pub fn key(self) -> &'static str {
        match self {
            Self::AirHorn => "air-horn",
            Self::Siren => "siren",
            Self::Riser => "riser",
            Self::LogDrum => "log-drum",
        }
    }

    /// The sound a settings key names.
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|sound| sound.key() == key)
    }

    /// How a slot plays the sound when it is put there: the siren loops, the riser and the siren
    /// follow the master's tempo, the hits play once at their own pitch.
    pub fn settings(self) -> SampleSettings {
        let tempo_locked = SampleSettings {
            sync: true,
            bpm: Some(SYNTH_BPM),
            ..SampleSettings::default()
        };
        match self {
            Self::AirHorn | Self::LogDrum => SampleSettings::default(),
            Self::Siren => SampleSettings {
                mode: SampleMode::Loop,
                ..tempo_locked
            },
            Self::Riser => tempo_locked,
        }
    }

    /// The sound as a track at `sample_rate`.
    pub fn track(self, sample_rate: u32) -> Result<Track, EngineError> {
        let rate = f64::from(sample_rate);
        let mono = match self {
            Self::AirHorn => air_horn(rate),
            Self::Siren => siren(rate),
            Self::Riser => riser(rate),
            Self::LogDrum => log_drum(rate),
        };
        let loudest = mono
            .iter()
            .fold(0.0_f64, |loudest, sample| loudest.max(sample.abs()));
        let scale = if loudest > 0.0 { PEAK / loudest } else { 0.0 };
        let stereo: Vec<f32> = mono
            .iter()
            .flat_map(|sample| {
                let scaled = (sample * scale) as f32;
                [scaled, scaled]
            })
            .collect();
        Track::from_interleaved_stereo(&stereo, sample_rate)
    }
}

/// Frames in `seconds` at `rate`.
fn frames_in(seconds: f64, rate: f64) -> usize {
    (seconds * rate).round() as usize
}

/// A band-limited sawtooth at `phase` cycles, with every harmonic below `ceiling_hz`.
fn sawtooth(phase: f64, frequency: f64, ceiling_hz: f64) -> f64 {
    let harmonics = ((ceiling_hz / frequency).floor() as usize).max(1);
    (1..=harmonics)
        .map(|harmonic| {
            let order = harmonic as f64;
            (TAU * phase * order).sin() / order
        })
        .sum::<f64>()
}

/// A band-limited square wave at `phase` cycles: odd harmonics below `ceiling_hz`.
fn square(phase: f64, frequency: f64, ceiling_hz: f64) -> f64 {
    let harmonics = ((ceiling_hz / frequency).floor() as usize).max(1);
    (1..=harmonics)
        .step_by(2)
        .map(|harmonic| {
            let order = harmonic as f64;
            (TAU * phase * order).sin() / order
        })
        .sum::<f64>()
}

/// A gain that rises over `attack` seconds and falls over `release` seconds at the edges of a
/// note lasting `length` seconds, `time` seconds in.
fn note_envelope(time: f64, length: f64, attack: f64, release: f64) -> f64 {
    if time < 0.0 || time >= length {
        return 0.0;
    }
    (time / attack).min(1.0) * ((length - time) / release).min(1.0)
}

fn air_horn(rate: f64) -> Vec<f64> {
    const BLASTS: [(f64, f64); 4] = [(0.0, 0.14), (0.2, 0.14), (0.4, 0.14), (0.6, 0.62)];
    const VOICES_HZ: [f64; 3] = [466.16, 554.37, 698.46];
    let ceiling = (rate * 0.45).min(9_000.0);
    let length = frames_in(1.3, rate);
    let mut phases = [0.0_f64; VOICES_HZ.len()];
    (0..length)
        .map(|frame| {
            let time = frame as f64 / rate;
            let gain: f64 = BLASTS
                .iter()
                .map(|(start, blast)| note_envelope(time - start, *blast, 0.004, 0.03))
                .sum();
            let blast_start = BLASTS
                .iter()
                .rev()
                .find(|(start, _)| time >= *start)
                .map_or(0.0, |(start, _)| *start);
            let scoop = 1.0 - 0.06 * (-(time - blast_start) / 0.025).exp();
            let mut sample = 0.0;
            for (phase, voice_hz) in phases.iter_mut().zip(VOICES_HZ) {
                let frequency = voice_hz * scoop;
                sample += sawtooth(*phase, frequency, ceiling);
                *phase = (*phase + frequency / rate).fract();
            }
            (sample * 0.8).tanh() * gain
        })
        .collect()
}

fn siren(rate: f64) -> Vec<f64> {
    let beats = 2.0;
    let length = frames_in(beats * 60.0 / SYNTH_BPM, rate);
    let sweeps = 2.0;
    let ceiling = (rate * 0.45).min(8_000.0);
    let frequency_at = |frame: usize| {
        let position = (frame as f64 / length as f64 * sweeps).fract();
        let triangle = 1.0 - (2.0 * position - 1.0).abs();
        620.0 * 2.0_f64.powf(triangle)
    };
    let cycles: f64 = (0..length).map(|frame| frequency_at(frame) / rate).sum();
    let whole_cycles = cycles.round().max(1.0) / cycles;
    let mut phase = 0.0_f64;
    (0..length)
        .map(|frame| {
            let frequency = frequency_at(frame) * whole_cycles;
            let sample = square(phase, frequency, ceiling);
            phase = (phase + frequency / rate).fract();
            sample
        })
        .collect()
}

fn riser(rate: f64) -> Vec<f64> {
    let seconds = 8.0 * 60.0 / SYNTH_BPM;
    let length = frames_in(seconds, rate);
    let mut noise = NoiseSource::new(0x5eed_d0b9_1a7e);
    let mut band_pass = StateVariableFilter::default();
    let mut tone_phase = 0.0_f64;
    (0..length)
        .map(|frame| {
            let progress = frame as f64 / length as f64;
            let centre = 200.0 * 40.0_f64.powf(progress);
            let filtered = band_pass.band_pass(noise.next_sample(), centre.min(rate * 0.12), rate);
            let tone_hz = 110.0 * 8.0_f64.powf(progress);
            let tone = (TAU * tone_phase).sin();
            tone_phase = (tone_phase + tone_hz / rate).fract();
            let swell = progress * progress;
            let tail = ((length - frame) as f64 / (0.005 * rate)).min(1.0);
            (filtered * 2.5 + tone * 0.35) * swell * tail
        })
        .collect()
}

fn log_drum(rate: f64) -> Vec<f64> {
    const ROOT_HZ: f64 = 98.0;
    let length = frames_in(0.9, rate);
    let mut body_phase = 0.0_f64;
    let mut knock_phase = 0.0_f64;
    (0..length)
        .map(|frame| {
            let time = frame as f64 / rate;
            let glide = 1.0 + 1.2 * (-time / 0.018).exp();
            let body_hz = ROOT_HZ * glide;
            let knock_hz = ROOT_HZ * 2.76 * glide;
            let body = (TAU * body_phase).sin() * (-time / 0.32).exp();
            let knock = (TAU * knock_phase).sin() * (-time / 0.05).exp() * 0.45;
            body_phase = (body_phase + body_hz / rate).fract();
            knock_phase = (knock_phase + knock_hz / rate).fract();
            let attack = (time / 0.002).min(1.0);
            let tail = ((length - frame) as f64 / (0.01 * rate)).min(1.0);
            ((body + knock) * 1.6).tanh() * attack * tail
        })
        .collect()
}

/// Seeded white noise, the same sequence on every run.
struct NoiseSource {
    state: u64,
}

impl NoiseSource {
    fn new(seed: u64) -> Self {
        Self { state: seed.max(1) }
    }

    /// The next value, from -1.0 to 1.0.
    fn next_sample(&mut self) -> f64 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        (self.state >> 11) as f64 / (1_u64 << 52) as f64 - 1.0
    }
}

/// A Chamberlin state variable filter, for the riser's sweeping band.
#[derive(Default)]
struct StateVariableFilter {
    low: f64,
    band: f64,
}

impl StateVariableFilter {
    fn band_pass(&mut self, input: f64, centre_hz: f64, rate: f64) -> f64 {
        const DAMPING: f64 = 0.35;
        let tuning = 2.0 * (std::f64::consts::PI * centre_hz / rate).sin();
        self.low += tuning * self.band;
        let high = input - self.low - DAMPING * self.band;
        self.band += tuning * high;
        self.band
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sound_has_a_settings_key_that_names_it_back() {
        for sound in SynthSound::ALL {
            assert_eq!(SynthSound::from_key(sound.key()), Some(sound));
        }
        assert_eq!(SynthSound::from_key("horn"), None);
    }

    #[test]
    fn the_noise_spans_minus_one_to_one() {
        let mut noise = NoiseSource::new(7);
        let samples: Vec<f64> = (0..10_000).map(|_| noise.next_sample()).collect();
        assert!(samples.iter().all(|sample| (-1.0..1.0).contains(sample)));
        let mean = samples.iter().sum::<f64>() / samples.len() as f64;
        assert!(mean.abs() < 0.05, "mean {mean}");
    }
}
