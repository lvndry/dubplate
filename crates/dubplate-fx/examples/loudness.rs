//! Measures how loud and how present every effect is on your own tracks, at several positions
//! of the LEVEL/DEPTH knob.
//!
//! For each track it plays a section dry, then through each effect switched on for that section
//! and off for a few seconds after, and prints:
//!
//! - `added`: the loudness of what the effect changed (the output minus the dry track), against
//!   the dry track, in LU. Around -6 LU the effect is as present as the music; 0 LU means the
//!   change is as loud as the track itself.
//! - `rise`: how much louder the output is than the dry track, in LU.
//! - `peak`: the loudest sample of the effect's output, in dBFS, before the master limiter.
//! - `limited`: the loudest sample after Dubplate's master limiter, in dBFS.
//! - `trail`: the loudness of the first two seconds after the effect is switched off, against
//!   the dry track over the same seconds, in LU.
//!
//! Loudness is K-weighted and gated as in ITU-R BS.1770.
//!
//! ```text
//! cargo run --release -p dubplate-fx --example loudness -- \
//!     122:"$HOME/Music/Afro House/Track.m4a" 113:"$HOME/Music/Amapiano/Other.m4a" \
//!     --start 60 --seconds 16
//! ```

use std::f64::consts::PI;
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use dubplate_engine::{DecodeStatus, decode_file};
use dubplate_fx::{Effect, EffectConfig, EffectKind, Limiter, ProcessContext, create};

const BLOCK_FRAMES: usize = 256;
const DEPTHS: [f32; 4] = [0.25, 0.5, 0.75, 1.0];
const TRAIL_SECONDS: f64 = 2.0;
const DECODE_POLL_INTERVAL: Duration = Duration::from_millis(10);
const SILENCE_LUFS: f64 = -120.0;

struct Track {
    name: String,
    tempo_bpm: f64,
    sample_rate: u32,
    left: Vec<f32>,
    right: Vec<f32>,
}

struct Section {
    start: usize,
    on_frames: usize,
    trail_frames: usize,
}

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run(arguments: Vec<String>) -> Result<(), String> {
    let mut start_seconds = 60.0;
    let mut on_seconds = 16.0;
    let mut kinds: Vec<EffectKind> = EffectKind::ALL.to_vec();
    let mut tracks = Vec::new();
    let mut remaining = arguments.into_iter();
    while let Some(argument) = remaining.next() {
        match argument.as_str() {
            "--start" => start_seconds = number(remaining.next())?,
            "--seconds" => on_seconds = number(remaining.next())?,
            "--effects" => {
                let list = remaining.next().ok_or("--effects needs a list")?;
                kinds = list
                    .split(',')
                    .map(|name| name.parse().map_err(|error| format!("{error}")))
                    .collect::<Result<_, _>>()?;
            }
            _ => {
                let (tempo, path) = argument
                    .split_once(':')
                    .ok_or_else(|| format!("`{argument}` is not <bpm>:<track>"))?;
                let tempo_bpm = tempo
                    .parse()
                    .map_err(|_| format!("`{tempo}` is not a tempo"))?;
                tracks.push(read_track(Path::new(path), tempo_bpm)?);
            }
        }
    }
    if tracks.is_empty() {
        return Err(
            "usage: loudness <bpm>:<track>... [--start s] [--seconds s] [--effects a,b]".into(),
        );
    }

    println!(
        "{:<12} {:>5} {:>8} {:>7} {:>7} {:>8} {:>7}",
        "effect", "depth", "added", "rise", "peak", "limited", "trail"
    );
    for kind in kinds {
        for depth in DEPTHS {
            let mut totals = [0.0; 5];
            for track in &tracks {
                let section = Section {
                    start: (start_seconds * f64::from(track.sample_rate)) as usize,
                    on_frames: (on_seconds * f64::from(track.sample_rate)) as usize,
                    trail_frames: (TRAIL_SECONDS * f64::from(track.sample_rate)) as usize,
                };
                let measured = measure(track, &section, kind, depth)?;
                for (total, value) in totals.iter_mut().zip(measured) {
                    *total += value / tracks.len() as f64;
                }
            }
            println!(
                "{:<12} {:>5.2} {:>8.1} {:>7.1} {:>7.1} {:>8.1} {:>7.1}",
                kind.name(),
                depth,
                totals[0],
                totals[1],
                totals[2],
                totals[3],
                totals[4]
            );
        }
    }
    for track in &tracks {
        println!("track: {} at {} BPM", track.name, track.tempo_bpm);
    }
    Ok(())
}

fn number(value: Option<String>) -> Result<f64, String> {
    let value = value.ok_or("an option needs a value")?;
    value
        .parse()
        .map_err(|_| format!("`{value}` is not a number"))
}

fn read_track(path: &Path, tempo_bpm: f64) -> Result<Track, String> {
    let decoding = decode_file(path).map_err(|error| format!("{}: {error}", path.display()))?;
    loop {
        match decoding.status() {
            DecodeStatus::Decoding => std::thread::sleep(DECODE_POLL_INTERVAL),
            DecodeStatus::Complete => break,
            DecodeStatus::Failed(message) => return Err(format!("{}: {message}", path.display())),
        }
    }
    let frames = decoding.decoded_frames();
    let (left, right) = (0..frames)
        .map_while(|index| decoding.frame(index))
        .map(|[left, right]| (left, right))
        .unzip();
    Ok(Track {
        name: path
            .file_stem()
            .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned()),
        tempo_bpm,
        sample_rate: decoding.sample_rate(),
        left,
        right,
    })
}

/// Plays the section through the effect and returns added, rise, peak, limited and trail.
fn measure(
    track: &Track,
    section: &Section,
    kind: EffectKind,
    depth: f32,
) -> Result<[f64; 5], String> {
    let end = section.start + section.on_frames + section.trail_frames;
    if end > track.left.len() {
        return Err(format!("{} is shorter than the section", track.name));
    }
    let dry_left = &track.left[section.start..end];
    let dry_right = &track.right[section.start..end];
    let mut left = dry_left.to_vec();
    let mut right = dry_right.to_vec();
    let sample_rate = track.sample_rate as f32;
    let config = EffectConfig {
        max_sample_rate: sample_rate,
        ..EffectConfig::default()
    };
    let mut effect = create(kind, &config);
    effect.set_depth(depth);
    play(effect.as_mut(), track, section, &mut left, &mut right);
    let effect_peak = decibels(peak(&left).max(peak(&right)));
    let mut limited_left = left.clone();
    let mut limited_right = right.clone();
    let mut limiter = Limiter::new(sample_rate);
    for (left_block, right_block) in limited_left
        .chunks_mut(BLOCK_FRAMES)
        .zip(limited_right.chunks_mut(BLOCK_FRAMES))
    {
        limiter.process(left_block, right_block);
    }
    let limited = decibels(peak(&limited_left).max(peak(&limited_right)));

    let on = 0..section.on_frames;
    let trail = section.on_frames..end - section.start;
    let loudness = |left: &[f32], right: &[f32]| integrated_loudness(left, right, sample_rate);
    let dry_on = loudness(&dry_left[on.clone()], &dry_right[on.clone()]);
    let wet_on = loudness(&left[on.clone()], &right[on.clone()]);
    let added_left: Vec<f32> = difference(&left, dry_left);
    let added_right: Vec<f32> = difference(&right, dry_right);
    let added = loudness(&added_left[on.clone()], &added_right[on]);
    let dry_trail = loudness(&dry_left[trail.clone()], &dry_right[trail.clone()]);
    let wet_trail = loudness(&added_left[trail.clone()], &added_right[trail]);
    Ok([
        added - dry_on,
        wet_on - dry_on,
        effect_peak,
        limited,
        wet_trail - dry_trail,
    ])
}

fn play(
    effect: &mut dyn Effect,
    track: &Track,
    section: &Section,
    left: &mut [f32],
    right: &mut [f32],
) {
    let sample_rate = f64::from(track.sample_rate);
    let mut block_start = 0;
    while block_start < left.len() {
        let block_end = (block_start + BLOCK_FRAMES).min(left.len());
        effect.set_enabled(block_start < section.on_frames);
        let seconds = (section.start + block_start) as f64 / sample_rate;
        let context = ProcessContext::new(
            track.sample_rate as f32,
            track.tempo_bpm,
            seconds * track.tempo_bpm / 60.0,
        );
        effect.process(
            &mut left[block_start..block_end],
            &mut right[block_start..block_end],
            &context,
        );
        block_start = block_end;
    }
}

fn difference(wet: &[f32], dry: &[f32]) -> Vec<f32> {
    wet.iter()
        .zip(dry)
        .map(|(wet_sample, dry_sample)| wet_sample - dry_sample)
        .collect()
}

fn peak(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0, |loudest, sample| loudest.max(sample.abs()))
}

fn decibels(level: f32) -> f64 {
    20.0 * f64::from(level.max(1e-9)).log10()
}

/// A second-order filter section in direct form one.
#[derive(Clone, Copy)]
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

    fn tick(&mut self, input: f64) -> f64 {
        let output = self.feedforward[0] * input
            + self.feedforward[1] * self.inputs[0]
            + self.feedforward[2] * self.inputs[1]
            - self.feedback[0] * self.outputs[0]
            - self.feedback[1] * self.outputs[1];
        self.inputs = [input, self.inputs[0]];
        self.outputs = [output, self.outputs[0]];
        output
    }
}

/// The two stages of the K-weighting filter of ITU-R BS.1770 at any sample rate.
fn k_weighting(sample_rate: f64) -> [Biquad; 2] {
    let shelf_frequency = 1_681.974_450_955_533;
    let shelf_gain_decibels = 3.999_843_853_973_347;
    let shelf_quality = 0.707_175_236_955_419_6;
    let warped = (PI * shelf_frequency / sample_rate).tan();
    let high_gain = 10.0_f64.powf(shelf_gain_decibels / 20.0);
    let band_gain = high_gain.powf(0.499_666_774_154_541_6);
    let norm = 1.0 + warped / shelf_quality + warped * warped;
    let shelf = Biquad::new(
        [
            (high_gain + band_gain * warped / shelf_quality + warped * warped) / norm,
            2.0 * (warped * warped - high_gain) / norm,
            (high_gain - band_gain * warped / shelf_quality + warped * warped) / norm,
        ],
        [
            2.0 * (warped * warped - 1.0) / norm,
            (1.0 - warped / shelf_quality + warped * warped) / norm,
        ],
    );
    let cut_frequency = 38.135_470_876_024_44;
    let cut_quality = 0.500_327_037_323_877_3;
    let warped = (PI * cut_frequency / sample_rate).tan();
    let norm = 1.0 + warped / cut_quality + warped * warped;
    let high_pass = Biquad::new(
        [1.0, -2.0, 1.0],
        [
            2.0 * (warped * warped - 1.0) / norm,
            (1.0 - warped / cut_quality + warped * warped) / norm,
        ],
    );
    [shelf, high_pass]
}

/// The gated integrated loudness of a stereo signal in LUFS.
fn integrated_loudness(left: &[f32], right: &[f32], sample_rate: f32) -> f64 {
    let sample_rate = f64::from(sample_rate);
    let block = (0.4 * sample_rate) as usize;
    let hop = block / 4;
    let mut energy = vec![0.0; left.len()];
    for channel in [left, right] {
        let mut filters = k_weighting(sample_rate);
        for (sum, sample) in energy.iter_mut().zip(channel) {
            let weighted = filters
                .iter_mut()
                .fold(f64::from(*sample), |signal, filter| filter.tick(signal));
            *sum += weighted * weighted;
        }
    }
    if energy.len() < block {
        return SILENCE_LUFS;
    }
    let blocks: Vec<f64> = (0..=(energy.len() - block) / hop)
        .map(|index| energy[index * hop..index * hop + block].iter().sum::<f64>() / block as f64)
        .collect();
    let lufs = |power: f64| -0.691 + 10.0 * power.max(1e-30).log10();
    let gated_mean = |threshold: f64| {
        let kept: Vec<f64> = blocks
            .iter()
            .copied()
            .filter(|power| lufs(*power) > threshold)
            .collect();
        (!kept.is_empty()).then(|| kept.iter().sum::<f64>() / kept.len() as f64)
    };
    let Some(absolute) = gated_mean(-70.0) else {
        return SILENCE_LUFS;
    };
    gated_mean(lufs(absolute) - 10.0).map_or(SILENCE_LUFS, lufs)
}
