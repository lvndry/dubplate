//! Plays one or two tracks on the default sound card and prints latency and dropouts.
//!
//! ```text
//! cargo run --release -p dubplate-engine --example play -- <file-a> [file-b] [--buffer FRAMES] [--seconds N] [--volume DB]
//! ```
//!
//! In a terminal: space plays or pauses the selected deck, tab selects the other deck, c is
//! the cue button, up and down change the tempo by 1%, left and right move the crossfader,
//! x throws the crossfader to the other side, and q quits.

use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use dubplate_engine::{
    AudioOutput, Controller, DeckId, DecodeStatus, EngineStats, OutputConfig, Track,
    decibels_to_gain, gain_to_decibels,
};

struct Options {
    files: Vec<PathBuf>,
    buffer_frames: Option<u32>,
    seconds: Option<f64>,
    volume_db: f32,
}

fn parse_options() -> Result<Options, String> {
    let mut options = Options {
        files: Vec::new(),
        buffer_frames: None,
        seconds: None,
        volume_db: 0.0,
    };
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        let mut value = |name: &str| {
            arguments
                .next()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match argument.as_str() {
            "--buffer" => {
                options.buffer_frames =
                    Some(value("--buffer")?.parse().map_err(|_| "bad --buffer")?);
            }
            "--seconds" => {
                options.seconds = Some(value("--seconds")?.parse().map_err(|_| "bad --seconds")?);
            }
            "--volume" => {
                options.volume_db = value("--volume")?.parse().map_err(|_| "bad --volume")?;
            }
            _ => options.files.push(PathBuf::from(argument)),
        }
    }
    if options.files.is_empty() || options.files.len() > 2 {
        return Err(
            "usage: play <file-a> [file-b] [--buffer FRAMES] [--seconds N] [--volume DB]"
                .to_owned(),
        );
    }
    Ok(options)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = parse_options()?;
    let (mut controller, output) = AudioOutput::start(&OutputConfig {
        buffer_frames: options.buffer_frames,
    })?;
    println!(
        "Output: {} ({} channels, {} Hz)",
        output.device_name(),
        output.channel_count(),
        controller.stats().sample_rate
    );

    let mut tracks = Vec::new();
    for (deck, path) in DeckId::ALL.into_iter().zip(&options.files) {
        let track = controller.load_file(deck, path)?;
        println!(
            "Deck {deck:?}: {} ({} Hz, {} channels)",
            path.display(),
            track.sample_rate(),
            track.source_channels()
        );
        tracks.push(track);
    }
    for track in &tracks {
        wait_for_first_seconds(track);
    }
    controller.set_master_gain(decibels_to_gain(options.volume_db))?;
    controller.set_crossfader(if tracks.len() == 2 { 0.5 } else { 0.0 })?;
    for deck in DeckId::ALL.into_iter().take(tracks.len()) {
        controller.play(deck)?;
    }

    let interactive = options.seconds.is_none() && std::io::stdin().is_terminal();
    if interactive {
        println!(
            "space play/pause, tab switch deck, c cue, up/down tempo, left/right crossfader, x throw crossfader, q quit"
        );
        crossterm::terminal::enable_raw_mode()?;
        let result = run_interactive(&mut controller, tracks.len());
        crossterm::terminal::disable_raw_mode()?;
        result?;
    } else {
        run_for(&mut controller, options.seconds);
    }
    println!("\r\n{}", summary(&controller.stats()));
    drop(output);
    Ok(())
}

fn wait_for_first_seconds(track: &Track) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while track.status() == DecodeStatus::Decoding
        && track.decoded_seconds() < 5.0
        && Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(5));
    }
    if let DecodeStatus::Failed(message) = track.status() {
        eprintln!("decoding stopped: {message}");
    }
}

fn run_for(controller: &mut Controller, seconds: Option<f64>) {
    let started = Instant::now();
    loop {
        std::thread::sleep(Duration::from_secs(1));
        println!("{}", status_line(controller, None));
        let finished = seconds.is_some_and(|limit| started.elapsed().as_secs_f64() >= limit);
        let all_stopped = DeckId::ALL
            .iter()
            .all(|deck| !controller.deck_state(*deck).playing);
        if finished || all_stopped {
            return;
        }
    }
}

fn run_interactive(controller: &mut Controller, deck_count: usize) -> std::io::Result<()> {
    let mut selected = DeckId::A;
    let mut crossfader: f32 = if deck_count == 2 { 0.5 } else { 0.0 };
    let mut next_print = Instant::now() + Duration::from_secs(1);
    loop {
        let timeout = next_print.saturating_duration_since(Instant::now());
        if event::poll(timeout)?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            let tempo = controller.deck_state(selected).tempo;
            let result = match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Char(' ') => controller.toggle_play(selected),
                KeyCode::Char('c') => controller.cue(selected),
                KeyCode::Tab if deck_count == 2 => {
                    selected = if selected == DeckId::A {
                        DeckId::B
                    } else {
                        DeckId::A
                    };
                    Ok(())
                }
                KeyCode::Up => controller.set_tempo(selected, tempo + 0.01),
                KeyCode::Down => controller.set_tempo(selected, (tempo - 0.01).max(0.01)),
                KeyCode::Left | KeyCode::Right | KeyCode::Char('x') => {
                    crossfader = match key.code {
                        KeyCode::Left => (crossfader - 0.1).max(0.0),
                        KeyCode::Right => (crossfader + 0.1).min(1.0),
                        _ if crossfader < 0.5 => 1.0,
                        _ => 0.0,
                    };
                    controller.set_crossfader(crossfader)
                }
                _ => Ok(()),
            };
            if let Err(error) = result {
                print!("{error}\r\n");
            }
        }
        if Instant::now() >= next_print {
            print!("{}\r\n", status_line(controller, Some(selected)));
            std::io::stdout().flush()?;
            next_print += Duration::from_secs(1);
        }
    }
}

fn status_line(controller: &Controller, selected: Option<DeckId>) -> String {
    let meters = controller.take_meters();
    let decks: Vec<String> = DeckId::ALL
        .iter()
        .filter(|deck| controller.deck_state(**deck).loaded)
        .map(|deck| {
            let state = controller.deck_state(*deck);
            format!(
                "{}{:?} {} {} {:+.1}%",
                if selected == Some(*deck) { ">" } else { " " },
                deck,
                if state.playing { "play" } else { "stop" },
                clock(state.position_seconds),
                (state.tempo - 1.0) * 100.0
            )
        })
        .collect();
    let stats = controller.stats();
    format!(
        "{} | master {:.1} dB | {}",
        decks.join(" | "),
        gain_to_decibels(meters.master_peak),
        summary(&stats)
    )
}

fn summary(stats: &EngineStats) -> String {
    let output_latency = stats
        .output_latency_ms
        .map_or_else(|| "n/a".to_owned(), |latency| format!("{latency:.1} ms"));
    format!(
        "buffer {} frames ({:.1} ms), output latency {}, callbacks {}, overruns {} (late {}, starved {}, gaps {}, device errors {}), peak load {:.1}%",
        stats.buffer_frames,
        stats.buffer_latency_ms,
        output_latency,
        stats.callbacks,
        stats.overruns(),
        stats.late_callbacks,
        stats.starved_callbacks,
        stats.device_gaps,
        stats.device_errors,
        stats.max_callback_load * 100.0
    )
}

fn clock(seconds: f64) -> String {
    let minutes = (seconds / 60.0).floor();
    format!("{}:{:04.1}", minutes as u64, seconds - minutes * 60.0)
}
