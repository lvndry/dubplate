//! `dubplate usb`: exports a set to a USB stick, or a folder, that CDJs and XDJs read as a
//! rekordbox export. rekordbox's own files are only ever read, from a copy.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Subcommand;
use dubplate_prep::{AnalysisCache, AudioAnalysis, analyse_track};
use dubplate_usb::{
    UsbExportProgress, UsbExportStep, export_to_usb, local_today, playlists_from_set,
};

use crate::set::{SetSources, data_folder};

#[derive(Subcommand)]
pub(crate) enum UsbCommand {
    /// Export a set to a USB stick or a folder as rekordbox's Export mode does: the audio, each
    /// track's beat grid, hot cues, memory cues, loops and waveforms, and the set as a playlist.
    /// A stick holding a library rekordbox wrote is refused; one Dubplate wrote is added to.
    Export {
        /// The set's name.
        set: String,
        /// The stick or folder, such as /Volumes/STICK.
        #[arg(long)]
        to: PathBuf,
        /// The rekordbox library's master.db. Defaults to rekordbox's usual location.
        #[arg(long)]
        library: Option<PathBuf>,
    },
}

pub(crate) fn run(command: UsbCommand) -> Result<()> {
    match command {
        UsbCommand::Export { set, to, library } => export(&set, &to, library),
    }
}

/// Dubplate's analysis of a folder track: the cached one, else a new one made now.
fn dubplate_analysis(cache: Option<&AnalysisCache>, path: &Path) -> Option<AudioAnalysis> {
    if let Some(analysis) = cache.and_then(|cache| cache.get(path).ok().flatten()) {
        return Some(analysis);
    }
    eprintln!("Analysing {} for its beat grid…", path.display());
    match analyse_track(path) {
        Ok(analysis) => Some(analysis),
        Err(error) => {
            eprintln!("warning: {}: {error}", path.display());
            None
        }
    }
}

fn export(name: &str, destination: &Path, library: Option<PathBuf>) -> Result<()> {
    let sources = SetSources::load(name, library)?;
    let cache_path = data_folder()?.analysis_cache();
    let cache = AnalysisCache::open(&cache_path)
        .map_err(|error| eprintln!("warning: {}: {error}", cache_path.display()))
        .ok();
    let export_library = sources.export_library();
    let (playlists, problems) = playlists_from_set(&export_library, &sources.set, &mut |path| {
        dubplate_analysis(cache.as_ref(), path)
    });
    for problem in &problems {
        eprintln!("warning: {problem}");
    }
    let today = local_today();
    let report =
        export_to_usb(destination, &playlists, today, &mut print_progress).with_context(|| {
            format!(
                "exporting \"{}\" to {}",
                sources.set.name,
                destination.display()
            )
        })?;
    eprintln!();
    println!(
        "Exported \"{}\" to {}: {} track{}, {} copied, {} already there.",
        sources.set.name,
        report.destination.display(),
        report.tracks,
        if report.tracks == 1 { "" } else { "s" },
        report.copied,
        report.already_there
    );
    println!("Playlists: {}", report.playlists.join(", "));
    for warning in &report.warnings {
        eprintln!("warning: {warning}");
    }
    Ok(())
}

fn print_progress(progress: UsbExportProgress) {
    let what = match &progress.step {
        UsbExportStep::Track { title } => title.as_str(),
        UsbExportStep::Library => "the library",
    };
    eprint!("\r\x1b[2K[{}/{}] {what}", progress.done, progress.total);
    let _ = std::io::stderr().flush();
}
