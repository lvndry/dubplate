#![allow(dead_code)]

use std::path::Path;
use std::time::{Duration, Instant};

use dubplate_engine::{Controller, DecodeStatus, Renderer, Track};

pub const SAMPLE_RATE: u32 = 48_000;
pub const DECLICK_FRAMES: usize = 240;

/// Frames the channel EQ delays every deck by, as the engine reports it.
pub fn mixer_latency_frames(controller: &Controller) -> usize {
    let stats = controller.stats();
    (stats.mixer_latency_ms * f64::from(stats.sample_rate) / 1_000.0).round() as usize
}

/// Writes a 16-bit PCM WAV file with interleaved samples from -1.0 to 1.0.
pub fn write_wav(path: &Path, samples: &[f32], channel_count: u16, sample_rate: u32) {
    let bytes_per_sample = 2_u16;
    let data_length =
        u32::try_from(samples.len()).expect("WAV fits in 4 GB") * u32::from(bytes_per_sample);
    let block_align = channel_count * bytes_per_sample;
    let mut bytes = Vec::with_capacity(44 + data_length as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_length).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&channel_count.to_le_bytes());
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&(sample_rate * u32::from(block_align)).to_le_bytes());
    bytes.extend_from_slice(&block_align.to_le_bytes());
    bytes.extend_from_slice(&(bytes_per_sample * 8).to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_length.to_le_bytes());
    for sample in samples {
        let quantized = (sample.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        bytes.extend_from_slice(&quantized.to_le_bytes());
    }
    std::fs::write(path, bytes).expect("write WAV file");
}

/// A stereo track holding the same value on every frame.
pub fn constant_track(left: f32, right: f32, seconds: f64, sample_rate: u32) -> Track {
    let frame_count = (seconds * f64::from(sample_rate)) as usize;
    let samples: Vec<f32> = (0..frame_count).flat_map(|_| [left, right]).collect();
    Track::from_interleaved_stereo(&samples, sample_rate).expect("valid test track")
}

/// A stereo track whose samples rise in a straight line by `slope` per frame.
pub fn ramp_track(slope: f32, frame_count: usize, sample_rate: u32) -> Track {
    let samples: Vec<f32> = (0..frame_count)
        .flat_map(|index| {
            let value = index as f32 * slope;
            [value, value]
        })
        .collect();
    Track::from_interleaved_stereo(&samples, sample_rate).expect("valid test track")
}

/// Renders `frame_count` stereo frames in device-sized blocks and returns them interleaved.
pub fn render_frames(renderer: &mut Renderer, frame_count: usize, block_frames: usize) -> Vec<f32> {
    let mut output = vec![0.0; frame_count * 2];
    for block in output.chunks_mut(block_frames * 2) {
        renderer.render(block, 2);
    }
    output
}

pub fn left_channel(interleaved: &[f32]) -> Vec<f32> {
    interleaved.iter().step_by(2).copied().collect()
}

pub fn right_channel(interleaved: &[f32]) -> Vec<f32> {
    interleaved.iter().skip(1).step_by(2).copied().collect()
}

/// Largest change between two neighbouring samples: a click shows up as a large step.
pub fn largest_step(samples: &[f32]) -> f32 {
    samples
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).abs())
        .fold(0.0, f32::max)
}

pub fn wait_until_decoded(track: &Track) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while track.status() == DecodeStatus::Decoding {
        assert!(Instant::now() < deadline, "decoding did not finish");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(track.status(), DecodeStatus::Complete);
}

pub fn assert_close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "expected {expected}, got {actual}"
    );
}

/// A stereo track holding a sine wave on each side, either of which can be silent.
pub fn sine_track(
    frequency: f64,
    left_amplitude: f32,
    right_amplitude: f32,
    seconds: f64,
    sample_rate: u32,
) -> Track {
    let frame_count = (seconds * f64::from(sample_rate)) as usize;
    let samples: Vec<f32> = (0..frame_count)
        .flat_map(|index| {
            let phase = std::f64::consts::TAU * frequency * index as f64 / f64::from(sample_rate);
            let value = phase.sin() as f32;
            [value * left_amplitude, value * right_amplitude]
        })
        .collect();
    Track::from_interleaved_stereo(&samples, sample_rate).expect("valid test track")
}

/// A four-to-the-floor loop at `tempo_bpm`: a kick on every beat, a hi-hat on every off-beat, a
/// shaker on the sixteenths and a held chord, peaking around -3 dBFS. Busy enough that a filter,
/// a gate or an echo all change it audibly.
pub fn club_loop_track(tempo_bpm: f64, seconds: f64, sample_rate: u32) -> Track {
    let frames = (seconds * f64::from(sample_rate)) as usize;
    let beat_seconds = 60.0 / tempo_bpm;
    let mut noise_state: u64 = 0x2545_f491_4f6c_dd1d;
    let mut kick_phase = 0.0;
    let samples: Vec<f32> = (0..frames)
        .flat_map(|frame| {
            let seconds = frame as f64 / f64::from(sample_rate);
            let beats = seconds / beat_seconds;
            let since_beat = beats.fract() * beat_seconds;
            let kick_hz = 50.0 + 90.0 * (-since_beat / 0.03).exp();
            kick_phase += kick_hz / f64::from(sample_rate);
            let kick = (std::f64::consts::TAU * kick_phase).sin() * (-since_beat / 0.2).exp() * 0.5;
            noise_state ^= noise_state << 13;
            noise_state ^= noise_state >> 7;
            noise_state ^= noise_state << 17;
            let noise = (noise_state >> 40) as f64 / (1_u64 << 24) as f64 * 2.0 - 1.0;
            let since_off_beat = (beats + 0.5).fract() * beat_seconds;
            let hat = noise * 0.2 * (-since_off_beat / 0.03).exp();
            let since_sixteenth = (beats * 4.0).fract() * beat_seconds / 4.0;
            let shaker = noise * 0.06 * (-since_sixteenth / 0.02).exp();
            let chord: f64 = [220.0, 261.63, 329.63]
                .iter()
                .map(|hertz| (std::f64::consts::TAU * hertz * seconds).sin())
                .sum::<f64>()
                * 0.07;
            let value = (kick + hat + shaker + chord) as f32;
            [value, value]
        })
        .collect();
    Track::from_interleaved_stereo(&samples, sample_rate).expect("valid test track")
}
