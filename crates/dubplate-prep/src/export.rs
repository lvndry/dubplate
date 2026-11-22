//! Hands a prepared set to rekordbox as a rekordbox XML file: the set becomes a playlist in its
//! order, and each track carries its beat grid with the DJ's corrections, the mix-in and
//! mix-out as memory cues, and the cues rekordbox already had. rekordbox's own files are never
//! written: the DJ imports the file from rekordbox's rekordbox xml tree.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use dubplate_library::{FolderScan, LibraryTrack};
use dubplate_rekordbox::analysis::{Beat, BeatGrid};
use dubplate_rekordbox::xml::{
    MarkSlot, PositionMark, TempoMarker, XmlLibrary, XmlPlaylist, XmlTrack,
};
use dubplate_rekordbox::{Cue, CueKind, Error, Track, TrackAnalysis};

use crate::grid::GridCorrection;
use crate::store::{Set, SetEntry, is_inside_rekordbox};

/// The name of the memory cue where the track comes in.
pub const MIX_IN_CUE_NAME: &str = "Mix in";
/// The name of the memory cue where the track goes out.
pub const MIX_OUT_CUE_NAME: &str = "Mix out";
/// The folder in the DJ's music folder that exports go to unless the DJ picks another.
const EXPORT_FOLDER_NAME: &str = "Dubplate Exports";
/// How the DJ brings an export into rekordbox, as rekordbox 6 and 7 name each step.
pub const REKORDBOX_IMPORT_STEPS: &str = "In rekordbox, choose the file in Preferences > \
    Advanced > Database > rekordbox xml, then open the rekordbox xml tree in the sidebar. \
    Select the playlist's tracks and choose Import To Collection, so tracks rekordbox already \
    has take the new grids and cues, then Import Playlist.";
/// How far a beat may sit from where the current tempo marker puts it before the grid gets a
/// new marker there. rekordbox keeps beats to the millisecond, so a steady grid stays within
/// one millisecond of its first beat's tempo.
const BEAT_DRIFT_TOLERANCE_MS: f64 = 2.0;
/// Milliseconds in a minute times one hundred, for tempos kept in hundredths of a BPM.
const HUNDREDTH_BEATS_PER_MINUTE_MS: f64 = 6_000_000.0;

/// A set track ready to export: what the set says about it, what rekordbox or its file says,
/// and rekordbox's grid and cues for it.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportTrack {
    /// The set's entry: its mix cues and grid corrections.
    pub entry: SetEntry,
    /// The track's details, with `file_path` where the file is now.
    pub track: Track,
    /// Whether rekordbox's collection holds the track.
    pub in_rekordbox: bool,
    /// rekordbox's beat grid, before the DJ's corrections.
    pub beat_grid: Option<BeatGrid>,
    /// rekordbox's own memory cues, hot cues and saved loops for the track.
    pub cues: Vec<Cue>,
}

impl ExportTrack {
    fn location(&self) -> &Path {
        self.track
            .file_path
            .as_deref()
            .unwrap_or(&self.entry.track_path)
    }
}

/// The rekordbox XML of a set named `set_name` whose tracks, backups included, are `tracks` in
/// the set's order: one playlist with the set's order and, when the set keeps backups, a second
/// one named after the set with them.
pub fn rekordbox_xml(set_name: &str, tracks: &[ExportTrack]) -> String {
    let mut library = XmlLibrary::default();
    let mut ids_by_location: HashMap<&Path, u32> = HashMap::new();
    let mut main = XmlPlaylist {
        name: set_name.to_owned(),
        track_ids: Vec::new(),
    };
    let mut backups = XmlPlaylist {
        name: format!("{set_name} backups"),
        track_ids: Vec::new(),
    };
    for track in tracks {
        let next_id = u32::try_from(library.tracks.len() + 1).unwrap_or(u32::MAX);
        let track_id = *ids_by_location.entry(track.location()).or_insert_with(|| {
            library.tracks.push(xml_track(next_id, track));
            next_id
        });
        let playlist = if track.entry.is_backup {
            &mut backups
        } else {
            &mut main
        };
        playlist.track_ids.push(track_id);
    }
    library.playlists.push(main);
    if !backups.track_ids.is_empty() {
        library.playlists.push(backups);
    }
    library.to_xml()
}

fn xml_track(track_id: u32, export: &ExportTrack) -> XmlTrack {
    let track = &export.track;
    XmlTrack {
        track_id,
        location: export.location().to_path_buf(),
        name: track.title.clone(),
        artist: track.artist.clone(),
        album: track.album.clone(),
        genre: track.genre.clone(),
        label: track.label.clone(),
        comments: track.comment.clone(),
        tonality: track.key.clone(),
        average_bpm: track.bpm,
        total_time_seconds: track.length_seconds,
        size: track.file_size,
        rating: track.rating,
        date_added: track.date_added.clone(),
        tempos: export
            .beat_grid
            .as_ref()
            .map(|grid| tempo_markers(&GridCorrection::of(&export.entry).apply(grid)))
            .unwrap_or_default(),
        position_marks: position_marks(&export.entry, &export.cues),
    }
}

/// The tempo markers that rebuild `grid`: one at its first beat and one wherever the tempo
/// changes, the bars break their count, or a beat drifts from where the last marker puts it.
pub fn tempo_markers(grid: &BeatGrid) -> Vec<TempoMarker> {
    let mut markers = Vec::new();
    let mut anchor: Option<(usize, &Beat)> = None;
    let timed_beats = grid.beats.iter().filter(|beat| beat.tempo_hundredths != 0);
    for (index, beat) in timed_beats.enumerate() {
        let follows = anchor.is_some_and(|(anchor_index, anchor_beat)| {
            follows_marker(anchor_beat, index - anchor_index, beat)
        });
        if !follows {
            markers.push(TempoMarker {
                start_ms: beat.time_ms,
                tempo_hundredths: beat.tempo_hundredths,
                beat_in_bar: beat.beat_in_bar,
            });
            anchor = Some((index, beat));
        }
    }
    markers
}

/// Whether `beat`, `beats_since` beats after the marker's beat `anchor`, is where that marker
/// alone would put it: same tempo, the next place in the bar, and on time.
fn follows_marker(anchor: &Beat, beats_since: usize, beat: &Beat) -> bool {
    let beats_per_bar = crate::grid::BEATS_PER_BAR as usize;
    let expected_in_bar =
        (usize::from(anchor.beat_in_bar.saturating_sub(1)) + beats_since) % beats_per_bar + 1;
    let expected_ms = f64::from(anchor.time_ms)
        + beats_since as f64 * HUNDREDTH_BEATS_PER_MINUTE_MS / f64::from(anchor.tempo_hundredths);
    beat.tempo_hundredths == anchor.tempo_hundredths
        && usize::from(beat.beat_in_bar) == expected_in_bar
        && (f64::from(beat.time_ms) - expected_ms).abs() <= BEAT_DRIFT_TOLERANCE_MS
}

/// The DJ's mix-in and mix-out as named memory cues, then rekordbox's own cues. A rekordbox
/// memory cue at the very place of a mix cue gives way to it.
fn position_marks(entry: &SetEntry, cues: &[Cue]) -> Vec<PositionMark> {
    let mix_cues: Vec<PositionMark> = [
        (entry.mix_in_ms, MIX_IN_CUE_NAME),
        (entry.mix_out_ms, MIX_OUT_CUE_NAME),
    ]
    .into_iter()
    .filter_map(|(start_ms, name)| {
        Some(PositionMark {
            name: name.to_owned(),
            start_ms: start_ms?,
            loop_end_ms: None,
            slot: MarkSlot::Memory,
        })
    })
    .collect();
    let mut memory: Vec<PositionMark> = cues
        .iter()
        .filter(|cue| cue.kind == CueKind::Memory)
        .filter(|cue| {
            cue.loop_end_ms.is_some() || !mix_cues.iter().any(|mix| mix.start_ms == cue.start_ms)
        })
        .map(|cue| rekordbox_mark(cue, MarkSlot::Memory))
        .chain(mix_cues.iter().cloned())
        .collect();
    memory.sort_by_key(|mark| mark.start_ms);
    let mut hot: Vec<PositionMark> = cues
        .iter()
        .filter_map(|cue| Some(rekordbox_mark(cue, MarkSlot::Hot(cue.kind.pad()?))))
        .collect();
    hot.sort_by_key(|mark| match mark.slot {
        MarkSlot::Hot(pad) => pad.index(),
        MarkSlot::Memory => 0,
    });
    memory.extend(hot);
    memory
}

fn rekordbox_mark(cue: &Cue, slot: MarkSlot) -> PositionMark {
    PositionMark {
        name: cue.comment.clone().unwrap_or_default(),
        start_ms: cue.start_ms,
        loop_end_ms: cue.loop_end_ms,
        slot,
    }
}

/// rekordbox's collection as the export reads it.
#[derive(Debug, Clone, Copy)]
pub struct RekordboxCollection<'a> {
    /// Every track.
    pub tracks: &'a [Track],
    /// Every cue and saved loop.
    pub cues: &'a [Cue],
    /// rekordbox's `share` directory, which holds the analysis files.
    pub share_directory: &'a Path,
}

/// Everything the export finds a set's tracks in: rekordbox's collection when the DJ uses it,
/// the scans of the music folders, and the files the DJ relinked.
#[derive(Debug)]
pub struct ExportLibrary<'a> {
    rekordbox_by_id: HashMap<&'a str, &'a Track>,
    rekordbox_by_path: HashMap<PathBuf, &'a Track>,
    cues_by_track: HashMap<&'a str, Vec<&'a Cue>>,
    share_directory: Option<&'a Path>,
    folder_tracks: HashMap<&'a Path, &'a LibraryTrack>,
    relinks: &'a HashMap<PathBuf, PathBuf>,
}

/// A set exported to rekordbox XML, with what the DJ should know about it.
#[derive(Debug, Clone, PartialEq)]
pub struct SetExport {
    /// The rekordbox XML.
    pub xml: String,
    /// How many tracks the file holds.
    pub track_count: usize,
    /// How many of them rekordbox's collection does not hold yet: rekordbox adds and analyses
    /// them on import.
    pub new_to_rekordbox: usize,
    /// What could not be exported as it should, one sentence each.
    pub problems: Vec<String>,
}

/// Why an export could not be written.
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    /// The file would go inside one of rekordbox's folders, which Dubplate never writes.
    #[error("{0} is inside a rekordbox folder; Dubplate never writes there")]
    InsideRekordbox(PathBuf),
    /// The file or its folder could not be written.
    #[error("{path}: {source}")]
    Io {
        /// The file or folder.
        path: PathBuf,
        /// What went wrong.
        source: std::io::Error,
    },
}

impl SetExport {
    /// Writes the XML to `path`, creating its folder, through a temporary file so a failure
    /// never leaves half a file. Refuses any path inside a rekordbox folder.
    pub fn write_to(&self, path: &Path) -> Result<(), ExportError> {
        if is_inside_rekordbox(path) {
            return Err(ExportError::InsideRekordbox(path.to_path_buf()));
        }
        let io_error = |path: &Path| {
            let path = path.to_path_buf();
            move |source| ExportError::Io { path, source }
        };
        if let Some(folder) = path
            .parent()
            .filter(|folder| !folder.as_os_str().is_empty())
        {
            std::fs::create_dir_all(folder).map_err(io_error(folder))?;
        }
        let temporary = path.with_extension("xml.partial");
        std::fs::write(&temporary, &self.xml).map_err(io_error(&temporary))?;
        std::fs::rename(&temporary, path).map_err(io_error(path))
    }
}

/// Where exports go unless the DJ picks another place: `Dubplate Exports` in their music
/// folder, or `None` when the system has no home folder for them.
pub fn default_export_folder() -> Option<PathBuf> {
    dirs::audio_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join("Music")))
        .map(|music| music.join(EXPORT_FOLDER_NAME))
}

/// The file name an export of the set named `set_name` gets: the name, with the characters a
/// file name cannot hold replaced by `-`, and `.xml`.
pub fn export_file_name(set_name: &str) -> String {
    let name: String = set_name
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' => '-',
            control if control.is_control() => '-',
            kept => kept,
        })
        .collect();
    let name = name.trim().trim_start_matches('.');
    if name.is_empty() {
        "Set.xml".to_owned()
    } else {
        format!("{name}.xml")
    }
}

impl<'a> ExportLibrary<'a> {
    /// Indexes rekordbox's collection, the music folders' tracks and the relinks, as the
    /// library lists them: a file in rekordbox and in a folder is rekordbox's track.
    pub fn new(
        rekordbox: Option<RekordboxCollection<'a>>,
        folder_scans: &'a [FolderScan],
        relinks: &'a HashMap<PathBuf, PathBuf>,
    ) -> Self {
        let mut rekordbox_by_id = HashMap::new();
        let mut rekordbox_by_path = HashMap::new();
        let mut cues_by_track: HashMap<&str, Vec<&Cue>> = HashMap::new();
        if let Some(rekordbox) = rekordbox {
            for track in rekordbox.tracks {
                rekordbox_by_id.insert(track.id.as_str(), track);
                if let Some(path) = &track.file_path {
                    if let Some(relinked) = relinks.get(path) {
                        rekordbox_by_path.insert(relinked.clone(), track);
                    }
                    rekordbox_by_path.insert(path.clone(), track);
                }
            }
            for cue in rekordbox.cues {
                cues_by_track.entry(&cue.track_id).or_default().push(cue);
            }
        }
        let folder_tracks = folder_scans
            .iter()
            .flat_map(|scan| &scan.tracks)
            .map(|found| (found.path.as_path(), found))
            .collect();
        Self {
            rekordbox_by_id,
            rekordbox_by_path,
            cues_by_track,
            share_directory: rekordbox.map(|rekordbox| rekordbox.share_directory),
            folder_tracks,
            relinks,
        }
    }

    /// Exports `set`, its backups included, reading each rekordbox track's beat grid from
    /// rekordbox's analysis files. Disk work: run it off the interface thread.
    pub fn export(&self, set: &Set) -> SetExport {
        let mut problems = Vec::new();
        let tracks: Vec<ExportTrack> = set
            .entries
            .iter()
            .map(|entry| self.resolve(entry, &mut problems))
            .collect();
        for track in &tracks {
            if !track.location().is_file() {
                problems.push(format!(
                    "{} is not at {}.",
                    display_title(track),
                    track.location().display()
                ));
            }
        }
        let locations: HashSet<&Path> = tracks.iter().map(ExportTrack::location).collect();
        let new_to_rekordbox: HashSet<&Path> = tracks
            .iter()
            .filter(|track| !track.in_rekordbox)
            .map(ExportTrack::location)
            .collect();
        SetExport {
            xml: rekordbox_xml(&set.name, &tracks),
            track_count: locations.len(),
            new_to_rekordbox: new_to_rekordbox.len(),
            problems,
        }
    }

    /// Where the file of `entry` is now: rekordbox's track for it, found by its rekordbox id or
    /// its path, at the place the relinks give; otherwise the entry's own file, relinked.
    pub fn current_path(&self, entry: &SetEntry) -> PathBuf {
        match self.rekordbox_track(entry) {
            Some(track) => track.file_path.as_deref().map_or_else(
                || self.relinked(&entry.track_path),
                |path| self.relinked(path),
            ),
            None => self.relinked(&entry.track_path),
        }
    }

    /// The set's files that are somewhere else now, each old place with its new one.
    pub fn moved_files(&self, set: &Set) -> HashMap<PathBuf, PathBuf> {
        set.entries
            .iter()
            .filter_map(|entry| {
                let current = self.current_path(entry);
                (current != entry.track_path).then(|| (entry.track_path.clone(), current))
            })
            .collect()
    }

    fn rekordbox_track(&self, entry: &SetEntry) -> Option<&'a Track> {
        entry
            .rekordbox_id
            .as_deref()
            .and_then(|id| self.rekordbox_by_id.get(id))
            .or_else(|| self.rekordbox_by_path.get(&entry.track_path))
            .copied()
    }

    fn resolve(&self, entry: &SetEntry, problems: &mut Vec<String>) -> ExportTrack {
        let current_path = self.current_path(entry);
        if let Some(original) = self.rekordbox_track(entry) {
            let mut track = original.clone();
            track.file_path = Some(current_path.clone());
            let beat_grid = self.beat_grid(&track, problems);
            let cues = self
                .cues_by_track
                .get(track.id.as_str())
                .map(|cues| cues.iter().map(|cue| (*cue).clone()).collect())
                .unwrap_or_default();
            return ExportTrack {
                entry: entry.clone(),
                track,
                in_rekordbox: true,
                beat_grid,
                cues,
            };
        }
        let track = self
            .folder_tracks
            .get(current_path.as_path())
            .map(|found| folder_track(found))
            .unwrap_or_else(|| file_track(&current_path));
        ExportTrack {
            entry: entry.clone(),
            track,
            in_rekordbox: false,
            beat_grid: None,
            cues: Vec::new(),
        }
    }

    fn relinked(&self, path: &Path) -> PathBuf {
        self.relinks
            .get(path)
            .cloned()
            .unwrap_or_else(|| path.to_path_buf())
    }

    /// rekordbox's grid for `track`, or `None` when rekordbox has not analysed it. An analysis
    /// that cannot be read is a problem the DJ hears about.
    fn beat_grid(&self, track: &Track, problems: &mut Vec<String>) -> Option<BeatGrid> {
        let share_directory = self.share_directory?;
        let analysis_path = track
            .analysis_path
            .as_deref()
            .filter(|path| !path.is_empty())?;
        match TrackAnalysis::read(share_directory, analysis_path) {
            Ok(analysis) => analysis.beat_grid.filter(|grid| !grid.beats.is_empty()),
            Err(Error::NoAnalysis(_)) => None,
            Err(error) => {
                problems.push(format!(
                    "{}: its beat grid could not be read, so rekordbox keeps its own ({error}).",
                    track.title.as_deref().unwrap_or(&track.id)
                ));
                None
            }
        }
    }
}

fn display_title(track: &ExportTrack) -> String {
    track.track.title.clone().unwrap_or_else(|| {
        track
            .location()
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    })
}

/// A track found in a music folder, described as a rekordbox track: what its tags say, and no
/// rekordbox id, analysis or cues.
pub fn folder_track(found: &LibraryTrack) -> Track {
    Track {
        title: found.title.clone(),
        artist: found.artist.clone(),
        album: found.album.clone(),
        genre: found.genre.clone(),
        comment: found.comment.clone(),
        key: found.key.clone(),
        bpm: found.bpm.map(f64::from),
        length_seconds: found
            .duration_seconds
            .map(|seconds| seconds.round().max(0.0) as u32),
        file_size: Some(found.size),
        ..file_track(&found.path)
    }
}

/// An audio file known by its path alone, described as a rekordbox track with nothing else.
pub fn file_track(path: &Path) -> Track {
    Track {
        id: String::new(),
        title: None,
        artist: None,
        album: None,
        genre: None,
        label: None,
        key: None,
        bpm: None,
        length_seconds: None,
        file_path: Some(path.to_path_buf()),
        file_size: None,
        rating: None,
        comment: None,
        analysis_path: None,
        date_added: None,
        auto_gain_db: None,
    }
}
