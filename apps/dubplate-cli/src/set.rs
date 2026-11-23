//! `dubplate set`: the sets prepared in Dubplate, and their export to rekordbox as a rekordbox
//! XML file. rekordbox's own files are only ever read, from a copy.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use dubplate_library::{DataFolder, FolderScan, LibraryCache, LibrarySettings};
use dubplate_prep::{
    ExportLibrary, REKORDBOX_IMPORT_STEPS, RekordboxCollection, Set, SetStore, TrackIdentity,
    default_export_folder, export_file_name,
};
use dubplate_rekordbox::{Cue, Library, Track, TrackCues};

use crate::{master_db_or_default, open_library};

#[derive(Subcommand)]
pub(crate) enum SetCommand {
    /// List the sets prepared in Dubplate.
    List,
    /// Write a set to a rekordbox XML file for rekordbox to import: the set as a playlist, each
    /// track with its corrected beat grid, its mix-in and mix-out as memory cues, and its cues:
    /// the ones made in Dubplate, else the ones rekordbox already had. rekordbox's own files are
    /// never changed.
    Export {
        /// The set's name.
        name: String,
        /// Where to write the file. Defaults to "~/Music/Dubplate Exports/<set name>.xml".
        #[arg(long)]
        xml: Option<PathBuf>,
        /// The rekordbox library's master.db. Defaults to rekordbox's usual location.
        #[arg(long)]
        library: Option<PathBuf>,
    },
}

pub(crate) fn run(command: SetCommand) -> Result<()> {
    match command {
        SetCommand::List => list(),
        SetCommand::Export { name, xml, library } => export(&name, xml, library),
    }
}

pub(crate) fn data_folder() -> Result<DataFolder> {
    DataFolder::for_user().context("this system has no configuration directory for Dubplate")
}

/// Opens Dubplate's set database, which only exists once the desktop app has made a set.
fn open_sets(data: &DataFolder) -> Result<SetStore> {
    let path = data.sets();
    if !path.is_file() {
        bail!(
            "there are no sets yet: Dubplate keeps them in {}",
            path.display()
        );
    }
    SetStore::open(&path).with_context(|| format!("opening {}", path.display()))
}

fn list() -> Result<()> {
    let store = open_sets(&data_folder()?)?;
    for (_, name, track_count) in store.list_sets()? {
        let plural = if track_count == 1 { "" } else { "s" };
        println!("{name}  ({track_count} track{plural})");
    }
    Ok(())
}

/// The set named `name`, or else the only one named so in another case.
fn find_set(store: &SetStore, name: &str) -> Result<Set> {
    let sets = store.list_sets()?;
    let exact: Vec<_> = sets
        .iter()
        .filter(|(_, candidate, _)| candidate == name)
        .collect();
    let matching = if exact.is_empty() {
        sets.iter()
            .filter(|(_, candidate, _)| candidate.to_lowercase() == name.to_lowercase())
            .collect()
    } else {
        exact
    };
    match matching.as_slice() {
        [(id, _, _)] => Ok(store.load_set(*id)?),
        [] => {
            let names: Vec<&str> = sets.iter().map(|(_, name, _)| name.as_str()).collect();
            bail!(
                "no set is named \"{name}\". The sets are: {}",
                names.join(", ")
            )
        }
        _ => bail!("several sets are named \"{name}\"; rename one in Dubplate first"),
    }
}

/// rekordbox's tracks and cues, read from a copy of its library that lives as long as this.
struct RekordboxData {
    library: Library,
    tracks: Vec<Track>,
    cues: Vec<Cue>,
}

impl RekordboxData {
    fn read(master_db: &Path) -> Result<Self> {
        let library = open_library(master_db)?;
        Ok(Self {
            tracks: library.tracks()?,
            cues: library.cues()?,
            library,
        })
    }

    fn collection(&self) -> RekordboxCollection<'_> {
        RekordboxCollection {
            tracks: &self.tracks,
            cues: &self.cues,
            share_directory: self.library.share_directory(),
        }
    }
}

/// Everything a set's export reads: the set, the cues kept in Dubplate, the music folders'
/// scans, the relinks and, when the set uses it, rekordbox's collection.
pub(crate) struct SetSources {
    pub(crate) set: Set,
    track_cues: HashMap<TrackIdentity, Arc<TrackCues>>,
    scans: Vec<FolderScan>,
    relinks: HashMap<PathBuf, PathBuf>,
    rekordbox: Option<RekordboxData>,
}

impl SetSources {
    /// Reads the set named `name` and what its export needs; `library` is rekordbox's
    /// master.db, rekordbox's usual one when `None`.
    pub(crate) fn load(name: &str, library: Option<PathBuf>) -> Result<Self> {
        let data = data_folder()?;
        let store = open_sets(&data)?;
        let set = find_set(&store, name)?;
        let track_cues = store
            .track_cues()?
            .into_iter()
            .map(|(identity, cues)| (identity, Arc::new(cues)))
            .collect();
        let settings = LibrarySettings::load(&data.settings())?;
        let scans = LibraryCache::load(&data.library_cache())
            .scans
            .into_iter()
            .filter(|scan| settings.music_folders.contains(&scan.folder))
            .collect();
        let uses_rekordbox = settings.import_rekordbox
            || set.entries.iter().any(|entry| entry.rekordbox_id.is_some());
        let rekordbox = if uses_rekordbox {
            Some(RekordboxData::read(&master_db_or_default(library)?)?)
        } else {
            None
        };
        Ok(Self {
            set,
            track_cues,
            scans,
            relinks: settings.relink_map(),
            rekordbox,
        })
    }

    /// The set's tracks as the exports find them.
    pub(crate) fn export_library(&self) -> ExportLibrary<'_> {
        ExportLibrary::new(
            self.rekordbox.as_ref().map(RekordboxData::collection),
            &self.scans,
            &self.relinks,
        )
        .with_track_cues(&self.track_cues)
    }
}

fn export(name: &str, xml: Option<PathBuf>, library: Option<PathBuf>) -> Result<()> {
    let sources = SetSources::load(name, library)?;
    let set = &sources.set;
    let export = sources.export_library().export(set);
    let path = match xml {
        Some(path) => path,
        None => default_export_folder()
            .context("this system has no music folder; pass --xml")?
            .join(export_file_name(&set.name)),
    };
    export
        .write_to(&path)
        .with_context(|| format!("writing {}", path.display()))?;

    println!("Exported \"{}\" to {}", set.name, path.display());
    let plural = if export.track_count == 1 { "" } else { "s" };
    println!(
        "{} track{plural}, {} new to rekordbox.",
        export.track_count, export.new_to_rekordbox
    );
    for problem in &export.problems {
        eprintln!("warning: {problem}");
    }
    println!();
    println!("{REKORDBOX_IMPORT_STEPS}");
    Ok(())
}
