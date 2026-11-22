//! Plays one or two tracks on the default sound card and prints latency and dropouts.
//!
//! ```text
//! cargo run --release -p dubplate-engine --example play -- <file-a> [file-b] [--bpm BPM]... [--buffer FRAMES] [--seconds N] [--volume DB] [--device NAME] [--key-lock] [--tempo PERCENT] [--sync] [--fx]
//! cargo run --release -p dubplate-engine --example play -- --devices
//! ```
//!
//! `--key-lock` switches key lock on both decks, `--tempo PERCENT` sets deck A's tempo,
//! `--sync` beat-syncs deck B to deck A and `--fx` switches the three effect units on, for
//! listening to or timing the whole engine at once.
//!
//! `--device` plays on the output device with that name, such as `DDJ-FLX4`; on a device with
//! four channels the headphones play on outputs 3 and 4. `--devices` lists the devices.
//!
//! `--bpm` gives a track's tempo so effects follow its beat: the first one is deck A's, the
//! second deck B's.
//!
//! In a terminal: space plays or pauses the selected deck, tab selects the other deck, c is
//! the cue button, up and down change the tempo by 1%, left and right move the crossfader,
//! x throws the crossfader to the other side, and q quits. On the selected deck's channel, k
//! kills or restores the low, h the high, and j and l turn the filter knob left and right. The
//! jog: , and . nudge the selected deck back and forward, n and m search back and forward. s
//! switches beat sync on the selected deck, p makes it the tempo master and r steps through the
//! tempo ranges. Loops: 4 sets a 4-beat loop, i and o are loop in and out, 9 and 0 halve and
//! double the loop, u is reloop/exit, and ; and ' jump 4 beats back and forward. Headphones: v
//! cues the selected deck, f and g turn the headphone mix towards the cue and the master, and
//! a switches key lock on the selected deck. For
//! the effects: 1, 2 and 3 switch effect units 1 to 3 on and off, e selects the next effect on
//! unit 1, t moves unit 1 to the next target (deck A, deck B, master), [ and ] step its
//! parameter (its beat, or a reverb's size) down and up, and - and = turn its depth down and up.

use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use dubplate_engine::{
    AudioOutput, BeatGrid, BeatLength, Controller, DeckId, DecodeStatus, EffectKind, EffectTarget,
    EffectUnitId, EngineError, EngineStats, EqBand, MasterTempo, OutputConfig, SyncMode, Track,
    decibels_to_gain, gain_to_decibels,
};

const USAGE: &str = "usage: play <file-a> [file-b] [--bpm BPM]... [--buffer FRAMES] [--seconds N] [--volume DB] [--device NAME] [--key-lock] [--tempo PERCENT] [--sync] [--fx] | play --devices";
const MIX_STEP: f32 = 0.1;
const DEPTH_STEP: f32 = 0.1;
const FILTER_STEP: f32 = 0.05;
const NUDGE_TICKS: i32 = 30;
const SEARCH_TICKS: i32 = 60;

struct Options {
    files: Vec<PathBuf>,
    bpms: Vec<f64>,
    buffer_frames: Option<u32>,
    seconds: Option<f64>,
    volume_db: f32,
    key_lock: bool,
    tempo_percent: f64,
    sync: bool,
    effects: bool,
    device: Option<String>,
    list_devices: bool,
}

fn parse_options() -> Result<Options, String> {
    let mut options = Options {
        files: Vec::new(),
        bpms: Vec::new(),
        buffer_frames: None,
        seconds: None,
        volume_db: 0.0,
        key_lock: false,
        tempo_percent: 0.0,
        sync: false,
        effects: false,
        device: None,
        list_devices: false,
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
            "--bpm" => {
                options
                    .bpms
                    .push(value("--bpm")?.parse().map_err(|_| "bad --bpm")?);
            }
            "--device" => options.device = Some(value("--device")?),
            "--devices" => options.list_devices = true,
            "--key-lock" => options.key_lock = true,
            "--sync" => options.sync = true,
            "--fx" => options.effects = true,
            "--tempo" => {
                options.tempo_percent = value("--tempo")?.parse().map_err(|_| "bad --tempo")?;
            }
            "--volume" => {
                options.volume_db = value("--volume")?.parse().map_err(|_| "bad --volume")?;
            }
            _ => options.files.push(PathBuf::from(argument)),
        }
    }
    if options.list_devices {
        return Ok(options);
    }
    if options.files.is_empty() || options.files.len() > 2 || options.bpms.len() > 2 {
        return Err(USAGE.to_owned());
    }
    Ok(options)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = parse_options()?;
    if options.list_devices {
        for device in AudioOutput::output_devices()? {
            println!(
                "{}{}: {} channels, {} Hz{}",
                device.name,
                if device.is_default { " (default)" } else { "" },
                device.channel_count,
                device.sample_rate,
                if device.has_headphone_output() {
                    ", headphones on 3 and 4"
                } else {
                    ""
                }
            );
        }
        return Ok(());
    }
    let config = OutputConfig {
        buffer_frames: options.buffer_frames,
    };
    let (mut controller, output) = match &options.device {
        Some(name) => AudioOutput::start_on(name, &config)?,
        None => AudioOutput::start(&config)?,
    };
    println!(
        "Output: {} ({} channels, {} Hz{})",
        output.device_name(),
        output.channel_count(),
        controller.stats().sample_rate,
        if output.has_headphone_output() {
            ", headphones on 3 and 4"
        } else {
            ""
        }
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
    for (deck, bpm) in DeckId::ALL.into_iter().zip(&options.bpms) {
        controller.set_beat_grid(deck, Some(BeatGrid::new(*bpm, 0.0)?))?;
    }
    for track in &tracks {
        wait_for_first_seconds(track);
    }
    let volume = decibels_to_gain(options.volume_db);
    controller.set_master_gain(volume)?;
    controller.set_headphone_level(volume.min(1.0).sqrt())?;
    controller.set_crossfader(if tracks.len() == 2 { 0.5 } else { 0.0 })?;
    controller.set_tempo(DeckId::A, 1.0 + options.tempo_percent / 100.0)?;
    controller.set_master_tempo(MasterTempo::Deck(DeckId::A))?;
    for deck in DeckId::ALL {
        controller.set_key_lock(deck, options.key_lock)?;
        if options.sync && deck == DeckId::B {
            controller.set_sync(deck, SyncMode::Beat)?;
        }
    }
    if options.effects {
        for unit in EffectUnitId::ALL {
            controller.set_effect_enabled(unit, true)?;
        }
    }
    for deck in DeckId::ALL.into_iter().take(tracks.len()) {
        controller.play(deck)?;
    }

    let interactive = options.seconds.is_none() && std::io::stdin().is_terminal();
    if interactive {
        println!(
            "space play/pause, tab switch deck, c cue, up/down tempo, left/right crossfader, x throw crossfader, q quit\r\n\
             k/h kill low/high, j/l filter, ,/. nudge, n/m search, s sync, p master, r tempo range\r\n\
             4 loop, i/o loop in/out, 9/0 halve/double, u reloop/exit, ;/' beat jump\r\n\
             v headphone cue, f/g headphone mix, a key lock\r\n\
             1/2/3 effect units on/off, e next effect, t next target, [/] beat, -/= depth (unit 1)"
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
                KeyCode::Char(
                    key @ ('k' | 'h' | 'j' | 'l' | ',' | '.' | 'n' | 'm' | 's' | 'p' | 'r'),
                ) => control_channel(controller, selected, key),
                KeyCode::Char(key) => control_effects(controller, key),
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

/// Plays the channel and jog keys on the selected deck.
fn control_channel(
    controller: &mut Controller,
    deck: DeckId,
    key: char,
) -> Result<(), EngineError> {
    let channel = controller.channel(deck);
    let toggle_kill = |position: f32| if position > 0.0 { 0.0 } else { 0.5 };
    match key {
        'k' => controller.set_eq(deck, EqBand::Low, toggle_kill(channel.eq(EqBand::Low))),
        'h' => controller.set_eq(deck, EqBand::High, toggle_kill(channel.eq(EqBand::High))),
        'j' => controller.set_filter(deck, (channel.filter - FILTER_STEP).max(0.0)),
        'l' => controller.set_filter(deck, (channel.filter + FILTER_STEP).min(1.0)),
        ',' => controller.jog_nudge(deck, -NUDGE_TICKS),
        '.' => controller.jog_nudge(deck, NUDGE_TICKS),
        'n' => controller.jog_search(deck, -SEARCH_TICKS),
        'm' => controller.jog_search(deck, SEARCH_TICKS),
        's' => {
            let sync = match controller.tempo_state(deck).sync {
                SyncMode::Off => SyncMode::Beat,
                SyncMode::Tempo | SyncMode::Beat => SyncMode::Off,
            };
            controller.set_sync(deck, sync)
        }
        'p' => controller.set_master_tempo(MasterTempo::Deck(deck)),
        '4' => controller.beat_loop(deck, BeatLength::FOUR),
        'v' => {
            let cued = controller.headphones().is_cued(deck);
            controller.set_headphone_cue(deck, !cued)
        }
        'a' => {
            let locked = controller.key_lock(deck);
            controller.set_key_lock(deck, !locked)
        }
        'f' => controller.set_headphone_mix((controller.headphones().mix - MIX_STEP).max(0.0)),
        'g' => controller.set_headphone_mix((controller.headphones().mix + MIX_STEP).min(1.0)),
        'i' => controller.loop_in(deck),
        'o' => controller.loop_out(deck),
        '9' => controller.halve_loop(deck),
        '0' => controller.double_loop(deck),
        'u' => controller.reloop_or_exit(deck),
        ';' => controller.beat_jump(deck, -4.0),
        '\'' => controller.beat_jump(deck, 4.0),
        'r' => {
            let range = controller.tempo_state(deck).tempo_range.next();
            controller.set_tempo_range(deck, range)
        }
        _ => Ok(()),
    }
}

/// Plays the effect keys: 1 to 3 switch units on and off, the others turn unit 1.
fn control_effects(controller: &mut Controller, key: char) -> Result<(), EngineError> {
    if let Some(digit) = key.to_digit(10)
        && let Some(index) = (digit as usize).checked_sub(1)
        && let Ok(unit) = EffectUnitId::new(index)
    {
        return controller.toggle_effect(unit);
    }
    let unit = EffectUnitId::ALL[0];
    let state = controller.effect_unit(unit);
    match key {
        'e' => controller.set_effect_kind(unit, next(&EffectKind::ALL, state.kind)),
        't' => controller.set_effect_target(unit, next(&EffectTarget::ALL, state.target)),
        '[' | ']' => {
            let steps = if key == '[' { -1 } else { 1 };
            match (state.kind.parameter(), state.parameter) {
                (Some(spec), Some(value)) => {
                    controller.set_effect_parameter(unit, spec.step(value, steps))
                }
                _ => Ok(()),
            }
        }
        '-' => controller.set_effect_depth(unit, (state.depth - DEPTH_STEP).max(0.0)),
        '=' => controller.set_effect_depth(unit, (state.depth + DEPTH_STEP).min(1.0)),
        _ => Ok(()),
    }
}

/// The item after `current` in `items`, wrapping around to the first.
fn next<T: Copy + PartialEq>(items: &[T], current: T) -> T {
    let index = items.iter().position(|item| *item == current).unwrap_or(0);
    items[(index + 1) % items.len()]
}

fn status_line(controller: &Controller, selected: Option<DeckId>) -> String {
    let meters = controller.take_meters();
    let decks: Vec<String> = DeckId::ALL
        .iter()
        .filter(|deck| controller.deck_state(**deck).loaded)
        .map(|deck| {
            let state = controller.deck_state(*deck);
            let channel = controller.channel(*deck);
            let tempo = controller.tempo_state(*deck);
            let bpm = tempo
                .bpm
                .map_or_else(|| "---".to_owned(), |bpm| format!("{bpm:.2}"));
            format!(
                "{}{:?} {} {} {} BPM {:+.2}% {}{}{} eq {:.2}/{:.2}/{:.2} filter {:.2}",
                if selected == Some(*deck) { ">" } else { " " },
                deck,
                if state.playing { "play" } else { "stop" },
                clock(state.position_seconds),
                bpm,
                (state.tempo - 1.0) * 100.0,
                tempo.tempo_range,
                if tempo.is_master { " MASTER" } else { "" },
                if tempo.sync == SyncMode::Off {
                    String::new()
                } else {
                    format!(" {}", tempo.sync)
                },
                channel.eq(EqBand::High),
                channel.eq(EqBand::Mid),
                channel.eq(EqBand::Low),
                channel.filter
            )
        })
        .collect();
    let effects: Vec<String> = EffectUnitId::ALL
        .into_iter()
        .map(|unit| {
            let state = controller.effect_unit(unit);
            let target = match state.target {
                EffectTarget::Deck(deck) => format!("{deck:?}"),
                EffectTarget::BothDecks => "both decks".to_owned(),
                EffectTarget::Master => "master".to_owned(),
            };
            format!(
                "{unit} {} {} {target} {:.0}% {}",
                state.kind,
                state
                    .parameter
                    .map_or_else(|| "-".to_owned(), |value| value.to_string()),
                state.depth * 100.0,
                if state.enabled { "on" } else { "off" }
            )
        })
        .collect();
    let stats = controller.stats();
    format!(
        "{} | {} | master {:.1} dB | {}",
        decks.join(" | "),
        effects.join(", "),
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
