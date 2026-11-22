//! The `dubplate` command line tool.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use dubplate_rekordbox::{DatabaseKey, Library, Playlist, PlaylistKind, Track, default_master_db};

mod analyse;
mod analysis;
mod midi;
mod set;
mod usb;

#[derive(Parser)]
#[command(
    name = "dubplate",
    version,
    about = "Pro DJ software that works with rekordbox"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Work with a rekordbox library.
    #[command(subcommand)]
    Rekordbox(RekordboxCommand),
    /// Work with MIDI controllers.
    #[command(subcommand)]
    Midi(midi::MidiCommand),
    /// Work with the sets prepared in Dubplate, and export them to rekordbox.
    #[command(subcommand)]
    Set(set::SetCommand),
    /// Export sets to USB sticks that CDJs and XDJs read.
    #[command(subcommand)]
    Usb(usb::UsbCommand),
    /// Find the tempo, beat grid, key and energy of audio files as Dubplate analyses them, and
    /// compare them with rekordbox's. Audio files and rekordbox's files are only read.
    Analyse(analyse::AnalyseArgs),
}

#[derive(Subcommand)]
enum RekordboxCommand {
    /// Read a copy of the rekordbox library and describe it. rekordbox's own file is never changed.
    Inspect {
        /// The library's master.db. Defaults to rekordbox's usual location for this user.
        #[arg(long)]
        library: Option<PathBuf>,
        /// Print the whole library as JSON instead of a summary.
        #[arg(long)]
        json: bool,
        /// How many tracks the summary lists.
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Show a track's beat grid, cues, phrases and waveforms from rekordbox's analysis files.
    /// The files are only read.
    Analysis {
        /// The track's rekordbox ID or its exact title.
        track: String,
        /// The library's master.db. Defaults to rekordbox's usual location for this user.
        #[arg(long)]
        library: Option<PathBuf>,
        /// Print the whole analysis, waveforms included, as JSON.
        #[arg(long)]
        json: bool,
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Rekordbox(RekordboxCommand::Inspect {
            library,
            json,
            limit,
        }) => inspect(&master_db_or_default(library)?, json, limit),
        Command::Rekordbox(RekordboxCommand::Analysis {
            track,
            library,
            json,
        }) => analysis(&master_db_or_default(library)?, &track, json),
        Command::Midi(command) => midi::run(command),
        Command::Set(command) => set::run(command),
        Command::Usb(command) => usb::run(command),
        Command::Analyse(arguments) => analyse::run(arguments),
    }
}

pub(crate) fn master_db_or_default(library: Option<PathBuf>) -> Result<PathBuf> {
    match library {
        Some(path) => Ok(path),
        None => Ok(default_master_db()?),
    }
}

fn inspect(master_db: &Path, json: bool, limit: usize) -> Result<()> {
    let key = DatabaseKey::from_user_configuration()?;
    let library = Library::open_copy(master_db, &key)
        .with_context(|| format!("reading {}", master_db.display()))?;
    let summary = library.summary()?;
    let tracks = library.tracks()?;
    let playlists = library.playlists()?;

    if json {
        let cues = library.cues()?;
        let document = serde_json::json!({
            "summary": summary,
            "tracks": tracks,
            "cues": cues,
            "playlists": playlists,
        });
        println!("{}", serde_json::to_string_pretty(&document)?);
        return Ok(());
    }

    println!("Library     {}", master_db.display());
    println!(
        "Schema      {}",
        summary.database_version.as_deref().unwrap_or("unknown")
    );
    println!("Tracks      {}", summary.tracks);
    println!("Playlists   {}", summary.playlists);
    println!("Cues        {}", summary.cues);
    println!();
    print_playlist_tree(&playlists, None, 0);
    println!();
    println!(
        "{:<40} {:<28} {:>7} {:>4} {:>6}",
        "Title", "Artist", "BPM", "Key", "Time"
    );
    for track in tracks.iter().take(limit) {
        print_track(track);
    }
    if tracks.len() > limit {
        println!(
            "… and {} more. Use --limit or --json to see them.",
            tracks.len() - limit
        );
    }
    Ok(())
}

fn analysis(master_db: &Path, track: &str, json: bool) -> Result<()> {
    let key = DatabaseKey::from_user_configuration()?;
    let library = Library::open_copy(master_db, &key)
        .with_context(|| format!("reading {}", master_db.display()))?;
    analysis::show(&library, track, json)
}

fn print_playlist_tree(playlists: &[Playlist], parent_id: Option<&str>, depth: usize) {
    let mut children: Vec<&Playlist> = playlists
        .iter()
        .filter(|playlist| playlist.parent_id.as_deref() == parent_id)
        .collect();
    children.sort_by_key(|playlist| playlist.position);
    for playlist in children {
        let detail = match playlist.kind {
            PlaylistKind::Folder => "folder".to_owned(),
            PlaylistKind::Smart => "smart playlist".to_owned(),
            PlaylistKind::Playlist => format!("{} tracks", playlist.track_ids.len()),
            PlaylistKind::Unknown(attribute) => format!("unknown kind {attribute}"),
        };
        println!("{}{}  ({detail})", "  ".repeat(depth), playlist.name);
        print_playlist_tree(playlists, Some(&playlist.id), depth + 1);
    }
}

fn print_track(track: &Track) {
    let bpm = track.bpm.map(|bpm| format!("{bpm:.2}")).unwrap_or_default();
    let time = track
        .length_seconds
        .map(|seconds| format!("{}:{:02}", seconds / 60, seconds % 60))
        .unwrap_or_default();
    println!(
        "{:<40} {:<28} {:>7} {:>4} {:>6}",
        truncate(track.title.as_deref().unwrap_or(""), 40),
        truncate(track.artist.as_deref().unwrap_or(""), 28),
        bpm,
        track.key.as_deref().unwrap_or(""),
        time,
    );
}

fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_owned();
    }
    let mut shortened: String = text.chars().take(width.saturating_sub(1)).collect();
    shortened.push('…');
    shortened
}
