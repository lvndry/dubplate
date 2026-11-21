//! Renders a WAV file through one effect so it can be heard.
//!
//! ```text
//! cargo run --release -p dubplate-fx --example render -- input.wav echo output.wav
//! cargo run --release -p dubplate-fx --example render -- input.wav sweep output.wav \
//!     --depth 0 --depth-to 1
//! cargo run --release -p dubplate-fx --example render -- input.wav backspin output.wav \
//!     --bpm 126 --on 20 --off 23
//! ```
//!
//! Options:
//!
//! - `--bpm <tempo>`: the tempo of the track, 120 by default
//! - `--first-beat <seconds>`: where the first beat of the grid falls, 0 by default
//! - `--beat <length>`: the beat length, such as `1/2`, `3/4` or `4`
//! - `--depth <0..1>`: the performance knob, 0.6 by default
//! - `--depth-to <0..1>`: turns the knob from `--depth` to this value while the effect is on
//! - `--mix <0..1>`: the dry and wet balance, overriding what the depth knob set
//! - `--on <seconds>`: when to switch the effect on, 0 by default
//! - `--off <seconds>`: when to switch it off; echo, ping-pong and reverb ring out after it

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use dubplate_fx::{BeatLength, EffectConfig, EffectKind, ProcessContext, create};
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};

const BLOCK_FRAMES: usize = 512;

struct Options {
    input: PathBuf,
    kind: EffectKind,
    output: PathBuf,
    tempo_bpm: f64,
    first_beat_seconds: f64,
    beat: Option<BeatLength>,
    depth: f32,
    depth_to: Option<f32>,
    mix: Option<f32>,
    on_seconds: f64,
    off_seconds: Option<f64>,
}

fn usage() -> String {
    let kinds: Vec<&str> = EffectKind::ALL.iter().map(|kind| kind.name()).collect();
    format!(
        "usage: render <input.wav> <effect> <output.wav> [--bpm 120] [--first-beat 0] \
         [--beat 1/2] [--depth 0.6] [--depth-to 1] [--mix 0.5] [--on 0] [--off 8]\n\
         effects: {}",
        kinds.join(", ")
    )
}

fn parse_options() -> Result<Options, Box<dyn Error>> {
    let mut arguments = std::env::args().skip(1);
    let mut positional = Vec::new();
    let mut options = Options {
        input: PathBuf::new(),
        kind: EffectKind::Echo,
        output: PathBuf::new(),
        tempo_bpm: 120.0,
        first_beat_seconds: 0.0,
        beat: None,
        depth: 0.6,
        depth_to: None,
        mix: None,
        on_seconds: 0.0,
        off_seconds: None,
    };
    while let Some(argument) = arguments.next() {
        if !argument.starts_with("--") {
            positional.push(argument);
            continue;
        }
        let value = arguments
            .next()
            .ok_or_else(|| format!("{argument} needs a value\n{}", usage()))?;
        match argument.as_str() {
            "--bpm" => options.tempo_bpm = value.parse()?,
            "--first-beat" => options.first_beat_seconds = value.parse()?,
            "--beat" => options.beat = Some(value.parse()?),
            "--depth" => options.depth = value.parse()?,
            "--depth-to" => options.depth_to = Some(value.parse()?),
            "--mix" => options.mix = Some(value.parse()?),
            "--on" => options.on_seconds = value.parse()?,
            "--off" => options.off_seconds = Some(value.parse()?),
            _ => return Err(format!("unknown option {argument}\n{}", usage()).into()),
        }
    }
    let [input, kind, output] = <[String; 3]>::try_from(positional).map_err(|_| usage())?;
    options.input = input.into();
    options.kind = kind.parse()?;
    options.output = output.into();
    Ok(options)
}

/// Planar stereo audio between -1 and 1.
struct Track {
    sample_rate: u32,
    left: Vec<f32>,
    right: Vec<f32>,
}

/// Reads a WAV file of any common format as stereo.
fn read_stereo(path: &Path) -> Result<Track, Box<dyn Error>> {
    let mut reader = WavReader::open(path)?;
    let spec = reader.spec();
    let channels = usize::from(spec.channels.max(1));
    let samples: Vec<f32> = match spec.sample_format {
        SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        SampleFormat::Int => {
            let full_scale = (1_i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|sample| sample.map(|value| value as f32 / full_scale))
                .collect::<Result<_, _>>()?
        }
    };
    let frames = samples.len() / channels;
    let mut left = Vec::with_capacity(frames);
    let mut right = Vec::with_capacity(frames);
    for frame in samples.chunks_exact(channels) {
        left.push(frame[0]);
        right.push(frame[channels.min(2) - 1]);
    }
    Ok(Track {
        sample_rate: spec.sample_rate,
        left,
        right,
    })
}

fn main() -> ExitCode {
    match render() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn render() -> Result<(), Box<dyn Error>> {
    let options = parse_options()?;
    let Track {
        sample_rate,
        mut left,
        mut right,
    } = read_stereo(&options.input)?;
    let config = EffectConfig {
        max_sample_rate: sample_rate as f32,
        ..EffectConfig::default()
    };
    let mut effect = create(options.kind, &config);
    if let Some(beat) = options.beat {
        effect.set_beat(beat);
    }
    effect.set_depth(options.depth);
    if let Some(mix) = options.mix {
        effect.set_mix(mix);
    }

    let seconds_per_frame = 1.0 / f64::from(sample_rate);
    let on_frame = (options.on_seconds / seconds_per_frame) as usize;
    let off_frame = options
        .off_seconds
        .map_or(usize::MAX, |seconds| (seconds / seconds_per_frame) as usize);
    let total_frames = left.len();
    let mut block_start = 0;
    while block_start < total_frames {
        let block_end = (block_start + BLOCK_FRAMES).min(total_frames);
        let switched_on = (on_frame..off_frame).contains(&block_start);
        if switched_on != effect.is_enabled() {
            effect.set_enabled(switched_on);
        }
        if let (Some(depth_to), true) = (options.depth_to, switched_on) {
            let span = off_frame.min(total_frames).saturating_sub(on_frame).max(1);
            let progress = (block_start - on_frame) as f32 / span as f32;
            effect.set_depth(options.depth + (depth_to - options.depth) * progress);
            if let Some(mix) = options.mix {
                effect.set_mix(mix);
            }
        }
        let seconds = block_start as f64 * seconds_per_frame;
        let beat_position = (seconds - options.first_beat_seconds) * options.tempo_bpm / 60.0;
        let context = ProcessContext::new(sample_rate as f32, options.tempo_bpm, beat_position);
        effect.process(
            &mut left[block_start..block_end],
            &mut right[block_start..block_end],
            &context,
        );
        block_start = block_end;
    }

    let spec = WavSpec {
        channels: 2,
        sample_rate,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };
    let mut writer = WavWriter::create(&options.output, spec)?;
    for (left_sample, right_sample) in left.iter().zip(&right) {
        writer.write_sample(*left_sample)?;
        writer.write_sample(*right_sample)?;
    }
    writer.finalize()?;
    println!(
        "rendered {} through {} into {}",
        options.input.display(),
        options.kind,
        options.output.display()
    );
    Ok(())
}
