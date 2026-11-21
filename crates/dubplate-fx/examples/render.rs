//! Renders one of your tracks through one effect into a WAV file, so you can hear it.
//!
//! It reads every format Dubplate plays (MP3, M4A with AAC or ALAC, FLAC, WAV, AIFF and Ogg)
//! and writes a 32-bit float WAV at the track's own sample rate.
//!
//! ```text
//! cargo run --release -p dubplate-fx --example render -- \
//!     "$HOME/Music/Afro House/Artist - Title (Extended Mix).m4a" echo echo.wav \
//!     --bpm 124 --beat 1/2 --on 32 --off 40
//! cargo run --release -p dubplate-fx --example render -- track.m4a sweep sweep.wav \
//!     --depth 0 --depth-to 1
//! cargo run --release -p dubplate-fx --example render -- track.mp3 backspin backspin.wav \
//!     --bpm 126 --on 20 --off 23
//! ```
//!
//! Options:
//!
//! - `--bpm <tempo>`: the tempo of the track, 120 by default
//! - `--first-beat <seconds>`: where the first beat of the grid falls, in seconds into the track,
//!   0 by default
//! - `--beat <length>`: the beat length, such as `1/2`, `3/4` or `4`
//! - `--depth <0..1>`: the performance knob, 0.6 by default
//! - `--depth-to <0..1>`: turns the knob from `--depth` to this value while the effect is on
//! - `--mix <0..1>`: the dry and wet balance, overriding what the depth knob set
//! - `--on <seconds>`: when to switch the effect on, in seconds into the track, 0 by default
//! - `--off <seconds>`: when to switch it off, in seconds into the track; echo, ping-pong and
//!   reverb ring out after it

use std::io;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use dubplate_engine::{DecodeStatus, EngineError, decode_file};
use dubplate_fx::{BeatLength, EffectConfig, EffectKind, ProcessContext, create};
use hound::{SampleFormat, WavSpec, WavWriter};

const BLOCK_FRAMES: usize = 512;
const DECODE_POLL_INTERVAL: Duration = Duration::from_millis(10);
const READABLE_FORMATS: &str = "MP3, M4A (AAC or ALAC), FLAC, WAV, AIFF and Ogg";
const KNOB_RANGE: RangeInclusive<f64> = 0.0..=1.0;
const SECONDS_RANGE: RangeInclusive<f64> = 0.0..=f64::MAX;
const TEMPO_RANGE: RangeInclusive<f64> = 20.0..=999.0;
const KNOB_EXPECTED: &str = "a knob position from 0 to 1, such as 0.6";
const SECONDS_EXPECTED: &str = "a time in seconds into the track, such as 32 or 95.5";

#[derive(Debug)]
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

/// What the command line asked for.
#[derive(Debug)]
enum Command {
    Render(Box<Options>),
    Help,
}

fn usage() -> String {
    let kinds: Vec<&str> = EffectKind::ALL.iter().map(|kind| kind.name()).collect();
    format!(
        "usage: render <track> <effect> <output.wav> [options]

  <track>        the track to play through the effect ({READABLE_FORMATS})
  <effect>       one of {effects}
  <output.wav>   where to write the result, as a WAV file at the track's sample rate

options:
  --bpm <tempo>             the track's tempo, 120 by default
  --first-beat <seconds>    where the first beat of the grid falls, in seconds into the track, 0 by default
  --beat <length>           the effect's beat length, such as 1/2, 3/4 or 4
  --depth <0..1>            the performance knob, 0.6 by default
  --depth-to <0..1>         turns the knob from --depth to this value while the effect is on
  --mix <0..1>              the dry and wet balance, overriding what the depth knob set
  --on <seconds>            when to switch the effect on, in seconds into the track, 0 by default
  --off <seconds>           when to switch it off, in seconds into the track; echo, ping-pong and reverb ring out after it

example:
  cargo run --release -p dubplate-fx --example render -- \\
    \"$HOME/Music/Afro House/Artist - Title (Extended Mix).m4a\" echo echo.wav \\
    --bpm 124 --beat 1/2 --on 32 --off 40",
        effects = kinds.join(", ")
    )
}

fn main() -> ExitCode {
    match parse_command(std::env::args().skip(1)) {
        Ok(Command::Help) => {
            println!("{}", usage());
            ExitCode::SUCCESS
        }
        Ok(Command::Render(options)) => match render(&options) {
            Ok(summary) => {
                println!("{summary}");
                ExitCode::SUCCESS
            }
            Err(message) => {
                eprintln!("{message}");
                ExitCode::FAILURE
            }
        },
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn parse_command(arguments: impl IntoIterator<Item = String>) -> Result<Command, String> {
    let mut arguments = arguments.into_iter();
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
        if argument == "--help" || argument == "-h" {
            return Ok(Command::Help);
        }
        if !argument.starts_with("--") {
            positional.push(argument);
            continue;
        }
        let option = argument.as_str();
        let value = arguments
            .next()
            .ok_or_else(|| format!("the option {option} needs a value after it\n\n{}", usage()))?;
        match option {
            "--bpm" => {
                options.tempo_bpm = parse_number(
                    option,
                    &value,
                    TEMPO_RANGE,
                    "a tempo from 20 to 999 BPM, such as 124",
                )?;
            }
            "--first-beat" => {
                options.first_beat_seconds =
                    parse_number(option, &value, SECONDS_RANGE, SECONDS_EXPECTED)?;
            }
            "--beat" => {
                let beat = value.parse().map_err(|_| {
                    invalid_value_message(
                        option,
                        &value,
                        "a beat length from 1/16 to 32, such as 1/2, 3/4 or 4",
                    )
                })?;
                options.beat = Some(beat);
            }
            "--depth" => options.depth = parse_knob(option, &value)?,
            "--depth-to" => options.depth_to = Some(parse_knob(option, &value)?),
            "--mix" => options.mix = Some(parse_knob(option, &value)?),
            "--on" => {
                options.on_seconds = parse_number(option, &value, SECONDS_RANGE, SECONDS_EXPECTED)?;
            }
            "--off" => {
                options.off_seconds = Some(parse_number(
                    option,
                    &value,
                    SECONDS_RANGE,
                    SECONDS_EXPECTED,
                )?);
            }
            _ => return Err(format!("there is no option {option}\n\n{}", usage())),
        }
    }
    let argument_count = positional.len();
    let [input, kind, output] = match <[String; 3]>::try_from(positional) {
        Ok(arguments) => arguments,
        Err(_) if argument_count == 0 => return Err(usage()),
        Err(_) => return Err(argument_count_message(argument_count)),
    };
    if let Some(off_seconds) = options.off_seconds
        && off_seconds <= options.on_seconds
    {
        return Err(format!(
            "--off {off_seconds} must come after --on {}: the effect switches off at --off seconds \
             into the track",
            options.on_seconds
        ));
    }
    options.input = input.into();
    options.kind = kind.parse().map_err(|error| format!("{error}"))?;
    options.output = output.into();
    Ok(Command::Render(Box::new(options)))
}

fn parse_number(
    option: &str,
    value: &str,
    range: RangeInclusive<f64>,
    expected: &str,
) -> Result<f64, String> {
    value
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|number| range.contains(number))
        .ok_or_else(|| invalid_value_message(option, value, expected))
}

fn parse_knob(option: &str, value: &str) -> Result<f32, String> {
    parse_number(option, value, KNOB_RANGE, KNOB_EXPECTED).map(|number| number as f32)
}

fn invalid_value_message(option: &str, value: &str, expected: &str) -> String {
    format!("\"{value}\" is not a valid value for {option}: it needs {expected}")
}

fn argument_count_message(argument_count: usize) -> String {
    let hint = if argument_count > 3 {
        "\nA path with spaces needs quotes around it, such as \"$HOME/Music/Afro House/Track.m4a\"."
    } else {
        ""
    };
    format!(
        "render needs a track, an effect and an output file, but got {argument_count} \
         arguments{hint}\n\n{}",
        usage()
    )
}

/// Explains an I/O error in plain words, without the operating system's error number.
fn describe_io_error(error: &io::Error) -> String {
    match error.kind() {
        io::ErrorKind::NotFound => "no such file or directory".to_owned(),
        io::ErrorKind::PermissionDenied => "permission denied".to_owned(),
        io::ErrorKind::IsADirectory => "it is a folder, not a file".to_owned(),
        io::ErrorKind::ReadOnlyFilesystem => "the disk is read-only".to_owned(),
        io::ErrorKind::StorageFull => "the disk is full".to_owned(),
        _ => {
            let text = error.to_string();
            let without_code = text
                .find(" (os error ")
                .map_or(text.as_str(), |position| &text[..position]);
            let mut characters = without_code.chars();
            characters.next().map_or_else(String::new, |first| {
                first.to_lowercase().chain(characters).collect()
            })
        }
    }
}

fn input_open_message(path: &Path, error: &io::Error) -> String {
    let hint = match error.kind() {
        io::ErrorKind::NotFound => {
            "\nCheck the path. A path with spaces needs quotes around it, or drag the file onto \
             the terminal to paste its full path."
        }
        io::ErrorKind::PermissionDenied => {
            "\nGive your terminal access to that folder (on macOS: System Settings, Privacy & \
             Security, Files and Folders), or copy the track somewhere it can read."
        }
        _ => "",
    };
    format!(
        "could not open the input file \"{}\": {}{hint}",
        path.display(),
        describe_io_error(error)
    )
}

fn input_decode_message(path: &Path, reason: &str) -> String {
    format!(
        "could not read the input file \"{}\" as audio: {reason}\nDubplate reads {READABLE_FORMATS} \
         files.",
        path.display()
    )
}

fn output_write_message(path: &Path, error: &io::Error) -> String {
    let folder = path
        .parent()
        .filter(|folder| !folder.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    match error.kind() {
        io::ErrorKind::NotFound => format!(
            "could not write \"{}\": the folder \"{}\" does not exist\nCreate the folder first, \
             or write the file somewhere else.",
            path.display(),
            folder.display()
        ),
        io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem => format!(
            "could not write \"{}\": {}\nChoose a folder you can write to, such as your Desktop.",
            path.display(),
            describe_io_error(error)
        ),
        _ => format!(
            "could not write \"{}\": {}",
            path.display(),
            describe_io_error(error)
        ),
    }
}

fn output_error_message(path: &Path, error: hound::Error) -> String {
    match error {
        hound::Error::IoError(io_error) => output_write_message(path, &io_error),
        other => format!("could not write \"{}\": {other}", path.display()),
    }
}

fn format_duration(seconds: f64) -> String {
    let whole_seconds = seconds.max(0.0).round() as u64;
    format!("{}:{:02}", whole_seconds / 60, whole_seconds % 60)
}

/// Planar stereo audio between -1 and 1.
struct Track {
    sample_rate: u32,
    left: Vec<f32>,
    right: Vec<f32>,
}

/// Decodes the whole file with the engine's decoder and returns it as stereo.
fn read_stereo(path: &Path) -> Result<Track, String> {
    if path.is_dir() {
        return Err(format!(
            "could not open the input file \"{}\": it is a folder, not a track",
            path.display()
        ));
    }
    let decoding = decode_file(path).map_err(|error| match error {
        EngineError::OpenFile { source, .. } => input_open_message(path, &source),
        EngineError::Decode { message, .. } => input_decode_message(path, &message),
        other => format!(
            "could not read the input file \"{}\": {other}",
            path.display()
        ),
    })?;
    loop {
        match decoding.status() {
            DecodeStatus::Decoding => std::thread::sleep(DECODE_POLL_INTERVAL),
            DecodeStatus::Complete => break,
            DecodeStatus::Failed(message) => return Err(input_decode_message(path, &message)),
        }
    }
    let frame_count = decoding.decoded_frames();
    let mut left = Vec::with_capacity(frame_count);
    let mut right = Vec::with_capacity(frame_count);
    for [left_sample, right_sample] in (0..frame_count).map_while(|index| decoding.frame(index)) {
        left.push(left_sample);
        right.push(right_sample);
    }
    Ok(Track {
        sample_rate: decoding.sample_rate(),
        left,
        right,
    })
}

fn is_same_file(first: &Path, second: &Path) -> bool {
    match (first.canonicalize(), second.canonicalize()) {
        (Ok(first), Ok(second)) => first == second,
        _ => false,
    }
}

fn check_output_path(options: &Options) -> Result<(), String> {
    let is_wav = options
        .output
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("wav"));
    if !is_wav {
        return Err(format!(
            "the output file \"{}\" must end in .wav: the render is written as a WAV file",
            options.output.display()
        ));
    }
    if is_same_file(&options.input, &options.output) {
        return Err(format!(
            "the output file \"{}\" is the input track; choose another name so the track is not \
             overwritten",
            options.output.display()
        ));
    }
    Ok(())
}

/// Renders the track through the effect and returns a line saying what was written.
fn render(options: &Options) -> Result<String, String> {
    check_output_path(options)?;
    let Track {
        sample_rate,
        mut left,
        mut right,
    } = read_stereo(&options.input)?;
    let total_frames = left.len();
    let track_seconds = total_frames as f64 / f64::from(sample_rate);
    if options.on_seconds >= track_seconds {
        return Err(format!(
            "--on {} is after the end of the track, which lasts {} ({track_seconds:.1} seconds)",
            options.on_seconds,
            format_duration(track_seconds)
        ));
    }

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
    let write_error = |error| output_error_message(&options.output, error);
    let mut writer = WavWriter::create(&options.output, spec).map_err(write_error)?;
    for (left_sample, right_sample) in left.iter().zip(&right) {
        writer.write_sample(*left_sample).map_err(write_error)?;
        writer.write_sample(*right_sample).map_err(write_error)?;
    }
    writer.finalize().map_err(write_error)?;
    Ok(format!(
        "rendered \"{}\" through {} into \"{}\" ({} at {sample_rate} Hz)",
        options.input.display(),
        options.kind,
        options.output.display(),
        format_duration(track_seconds)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(text: &[&str]) -> Vec<String> {
        text.iter().map(|argument| (*argument).to_owned()).collect()
    }

    fn render_error(text: &[&str]) -> String {
        match parse_command(arguments(text)) {
            Ok(Command::Render(options)) => render(&options).expect_err("the render should fail"),
            Ok(Command::Help) => panic!("expected a render, got help"),
            Err(message) => message,
        }
    }

    fn write_test_wav(path: &Path, sample_rate: u32, frame_count: usize) {
        let spec = WavSpec {
            channels: 2,
            sample_rate,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut writer = WavWriter::create(path, spec).expect("create the test track");
        for frame in 0..frame_count {
            let phase = std::f32::consts::TAU * 440.0 * frame as f32 / sample_rate as f32;
            let sample = (phase.sin() * 8_000.0) as i16;
            writer.write_sample(sample).expect("write left");
            writer.write_sample(sample).expect("write right");
        }
        writer.finalize().expect("finish the test track");
    }

    #[test]
    fn a_missing_input_names_the_file() {
        let directory = tempfile::tempdir().expect("temporary folder");
        let input = directory.path().join("track.m4a");
        let output = directory.path().join("out.wav");
        let message = render_error(&[
            input.to_str().expect("utf-8 path"),
            "echo",
            output.to_str().expect("utf-8 path"),
        ]);
        let expected = format!(
            "could not open the input file \"{}\": no such file or directory",
            input.display()
        );
        assert!(message.starts_with(&expected), "{message}");
        assert!(!message.contains("os error"), "{message}");
    }

    #[cfg(unix)]
    #[test]
    fn an_unwritable_output_names_the_file() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().expect("temporary folder");
        let input = directory.path().join("track.wav");
        write_test_wav(&input, 44_100, 4_410);
        let locked = directory.path().join("locked");
        std::fs::create_dir(&locked).expect("create the locked folder");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555))
            .expect("lock the folder");
        let output = locked.join("out.wav");
        let message = render_error(&[
            input.to_str().expect("utf-8 path"),
            "echo",
            output.to_str().expect("utf-8 path"),
        ]);
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755))
            .expect("unlock the folder");
        let expected = format!(
            "could not write \"{}\": permission denied",
            output.display()
        );
        assert!(message.starts_with(&expected), "{message}");
    }

    #[test]
    fn an_output_in_a_missing_folder_names_the_folder() {
        let path = Path::new("renders/out.wav");
        let message = output_write_message(path, &io::Error::from(io::ErrorKind::NotFound));
        assert!(
            message.starts_with(
                "could not write \"renders/out.wav\": the folder \"renders\" does not exist"
            ),
            "{message}"
        );
    }

    #[test]
    fn io_errors_read_as_plain_words() {
        assert_eq!(
            describe_io_error(&io::Error::from(io::ErrorKind::NotFound)),
            "no such file or directory"
        );
        assert_eq!(
            describe_io_error(&io::Error::from(io::ErrorKind::PermissionDenied)),
            "permission denied"
        );
        if cfg!(unix) {
            assert_eq!(
                describe_io_error(&io::Error::from_raw_os_error(5)),
                "input/output error"
            );
        }
    }

    #[test]
    fn a_file_that_is_not_audio_lists_the_formats() {
        let directory = tempfile::tempdir().expect("temporary folder");
        let input = directory.path().join("notes.m4a");
        std::fs::write(&input, "not audio").expect("write the file");
        let output = directory.path().join("out.wav");
        let message = render_error(&[
            input.to_str().expect("utf-8 path"),
            "echo",
            output.to_str().expect("utf-8 path"),
        ]);
        assert!(
            message.starts_with(&format!(
                "could not read the input file \"{}\" as audio",
                input.display()
            )),
            "{message}"
        );
        assert!(message.contains(READABLE_FORMATS), "{message}");
    }

    #[test]
    fn a_bad_option_value_names_the_option_and_the_value() {
        let message = render_error(&["track.m4a", "echo", "out.wav", "--bpm", "fast"]);
        assert!(
            message.starts_with("\"fast\" is not a valid value for --bpm"),
            "{message}"
        );
        let message = render_error(&["track.m4a", "echo", "out.wav", "--depth", "2"]);
        assert!(
            message.starts_with("\"2\" is not a valid value for --depth"),
            "{message}"
        );
        let message = render_error(&["track.m4a", "echo", "out.wav", "--beat", "1/64"]);
        assert!(
            message.starts_with("\"1/64\" is not a valid value for --beat"),
            "{message}"
        );
    }

    #[test]
    fn an_unknown_effect_lists_the_effects() {
        let message = render_error(&["track.m4a", "wobble", "out.wav"]);
        assert!(message.contains("wobble"), "{message}");
        for kind in EffectKind::ALL {
            assert!(message.contains(kind.name()), "{message}");
        }
    }

    #[test]
    fn an_off_before_on_is_refused() {
        let message = render_error(&["track.m4a", "echo", "out.wav", "--on", "16", "--off", "8"]);
        assert!(
            message.starts_with("--off 8 must come after --on 16"),
            "{message}"
        );
    }

    #[test]
    fn an_unquoted_path_with_spaces_says_to_quote_it() {
        let message = render_error(&["My", "Track.m4a", "echo", "out.wav"]);
        assert!(message.contains("got 4 arguments"), "{message}");
        assert!(message.contains("quotes"), "{message}");
    }

    #[test]
    fn the_output_must_be_a_wav_file_and_not_the_input() {
        let message = render_error(&["track.m4a", "echo", "out.m4a"]);
        assert!(
            message.starts_with("the output file \"out.m4a\" must end in .wav"),
            "{message}"
        );

        let directory = tempfile::tempdir().expect("temporary folder");
        let input = directory.path().join("track.wav");
        write_test_wav(&input, 44_100, 441);
        let path = input.to_str().expect("utf-8 path");
        let message = render_error(&[path, "echo", path]);
        assert!(message.contains("is the input track"), "{message}");
    }

    #[test]
    fn renders_a_track_at_its_own_sample_rate() {
        let directory = tempfile::tempdir().expect("temporary folder");
        let input = directory.path().join("track.wav");
        let output = directory.path().join("echo.wav");
        let sample_rate = 48_000;
        let frame_count = 24_000;
        write_test_wav(&input, sample_rate, frame_count);
        let command = parse_command(arguments(&[
            input.to_str().expect("utf-8 path"),
            "echo",
            output.to_str().expect("utf-8 path"),
            "--bpm",
            "124",
            "--beat",
            "1/2",
        ]))
        .expect("valid arguments");
        let Command::Render(options) = command else {
            panic!("expected a render");
        };
        let summary = render(&options).expect("the render succeeds");
        assert!(summary.contains("echo"), "{summary}");
        let reader = hound::WavReader::open(&output).expect("open the render");
        assert_eq!(reader.spec().sample_rate, sample_rate);
        assert_eq!(reader.spec().channels, 2);
        assert_eq!(reader.duration() as usize, frame_count);
    }

    #[test]
    fn an_effect_switched_on_after_the_end_is_refused() {
        let directory = tempfile::tempdir().expect("temporary folder");
        let input = directory.path().join("track.wav");
        write_test_wav(&input, 44_100, 44_100);
        let output = directory.path().join("out.wav");
        let message = render_error(&[
            input.to_str().expect("utf-8 path"),
            "echo",
            output.to_str().expect("utf-8 path"),
            "--on",
            "90",
        ]);
        assert!(
            message.starts_with("--on 90 is after the end of the track, which lasts 0:01"),
            "{message}"
        );
    }
}
