//! Measures how closely a track's separated stems, all at full level, reproduce the track on a
//! deck, and whether they line up with it, by rendering both offline. Plays nothing aloud.
//!
//! ```text
//! cargo run --profile fast -p dubplate-stems --example stem_residual -- <track> <stems-folder> [--from SECONDS] [--seconds N]
//! ```

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use dubplate_engine::{DeckId, DecodeStatus, Track, decode_file, offline_engine};
use dubplate_stems::StemFiles;

const OUTPUT_RATE: u32 = 48_000;
const BLOCK: usize = 256;
/// Keeps the loudest masters under the limiter, so both renders stay linear.
const TRIM: f32 = 0.5;
/// How far either way the stems are tried against the track, in output frames.
const LAGS: i64 = 400;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args().skip(1);
    let track_path = PathBuf::from(arguments.next().ok_or("give the track")?);
    let stems_folder = PathBuf::from(arguments.next().ok_or("give the stems folder")?);
    let mut from_seconds = 60.0;
    let mut seconds = 30.0;
    while let Some(argument) = arguments.next() {
        let value: f64 = arguments.next().ok_or("an option needs a value")?.parse()?;
        match argument.as_str() {
            "--from" => from_seconds = value,
            "--seconds" => seconds = value,
            other => return Err(format!("unknown option {other}").into()),
        }
    }
    let stems = StemFiles::in_folder(stems_folder)
        .ok_or("the folder does not hold the four stems")?;
    let loading = Instant::now();
    let stem_tracks = stems.load()?;
    println!(
        "loaded the stems in {:.2} s",
        loading.elapsed().as_secs_f64()
    );

    let plain = render(&decoded(&track_path)?, from_seconds, seconds)?;
    let separated_track = decoded(&track_path)?;
    separated_track.attach_stems(stem_tracks)?;
    let separated = render(&separated_track, from_seconds, seconds)?;

    let (best_lag, best_residual) = (-LAGS..=LAGS)
        .map(|lag| (lag, residual_decibels(&plain, &separated, lag)))
        .min_by(|first, second| first.1.total_cmp(&second.1))
        .ok_or("no lag")?;
    println!(
        "residual at full level: {:.1} dB under the track",
        -residual_decibels(&plain, &separated, 0)
    );
    println!(
        "best alignment: {best_lag} frames at {OUTPUT_RATE} Hz ({:.1} dB under the track)",
        -best_residual
    );
    Ok(())
}

fn decoded(path: &PathBuf) -> Result<Track, Box<dyn std::error::Error>> {
    let track = decode_file(path)?;
    let deadline = Instant::now() + Duration::from_secs(60);
    while track.status() == DecodeStatus::Decoding {
        if Instant::now() > deadline {
            return Err("decoding took too long".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    if let DecodeStatus::Failed(reason) = track.status() {
        return Err(reason.into());
    }
    Ok(track)
}

/// The deck's left and right output, `seconds` long from `from_seconds` into the track.
fn render(
    track: &Track,
    from_seconds: f64,
    seconds: f64,
) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    let (mut controller, mut renderer) = offline_engine(OUTPUT_RATE);
    controller.load_track(DeckId::A, track.clone())?;
    controller.set_crossfader(0.0)?;
    controller.set_gain(DeckId::A, TRIM)?;
    controller.seek(DeckId::A, from_seconds)?;
    controller.play(DeckId::A)?;
    let frames = (seconds * f64::from(OUTPUT_RATE)) as usize;
    let mut output = vec![0.0; frames * 2];
    for block in output.chunks_mut(BLOCK * 2) {
        renderer.render(block, 2);
    }
    Ok(output)
}

/// How loud `separated` delayed by `lag` frames differs from `plain`, against `plain`, in dB,
/// over the frames both cover after the first second.
fn residual_decibels(plain: &[f32], separated: &[f32], lag: i64) -> f64 {
    let frames = plain.len() / 2;
    let start = OUTPUT_RATE as usize;
    let mut difference = 0.0;
    let mut reference = 0.0;
    for frame in start..frames.saturating_sub(LAGS as usize) {
        let other = (frame as i64 + lag) as usize;
        for channel in 0..2 {
            let expected = f64::from(plain[frame * 2 + channel]);
            let actual = f64::from(separated[other * 2 + channel]);
            difference += (expected - actual).powi(2);
            reference += expected.powi(2);
        }
    }
    10.0 * (difference / reference.max(f64::MIN_POSITIVE)).log10()
}
