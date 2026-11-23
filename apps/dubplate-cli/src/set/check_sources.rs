//! The saved set's own files, library tags and previously prepared analysis.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use dubplate_library::{
    DataFolder, LibraryCache, LibrarySettings, is_audio_file, read_audio_metadata,
};
use dubplate_prep::{
    AnalysisCache, AudioAnalysis, ExportLibrary, PrepTrack, RekordboxCollection, SeamStatus, Set,
    SetCheckTrack, SetEntry, file_track, folder_track, preferred_track_value,
};
use dubplate_rekordbox::Track;
use dubplate_stems::{StemsIndex, VocalPresence};

use crate::{master_db_or_default, open_library};

pub(super) struct CheckSource {
    track: Option<PrepTrack>,
    file_found: bool,
    vocals: Option<VocalPresence>,
}

impl CheckSource {
    pub(super) fn facts(&self) -> SetCheckTrack<'_> {
        let vocals = self.vocals.unwrap_or_default();
        SetCheckTrack {
            track: self.track.as_ref(),
            file_found: self.file_found,
            vocals_at_start: vocals.at_start,
            vocals_at_end: vocals.at_end,
        }
    }
}

pub(super) fn load(
    data: &DataFolder,
    set: &Set,
    library: Option<PathBuf>,
) -> Result<(HashMap<PathBuf, CheckSource>, Vec<String>)> {
    if set.entries.is_empty() {
        return Ok((HashMap::new(), Vec::new()));
    }
    let settings = LibrarySettings::load(&data.settings())?;
    let scans: Vec<_> = LibraryCache::load(&data.library_cache())
        .scans
        .into_iter()
        .filter(|scan| settings.music_folders.contains(&scan.folder))
        .collect();
    let folders: Vec<_> = settings
        .music_folders
        .iter()
        .map(|folder| canonical(folder))
        .collect();
    let relinks = settings.relink_map();
    let mut notes = Vec::new();
    let uses_rekordbox = library.is_some()
        || set.entries.iter().any(|entry| {
            entry.rekordbox_id.is_some()
                || (settings.import_rekordbox
                    && !in_folders(&canonical(&entry.track_path), &folders)
                    && !scans.iter().any(|scan| {
                        scan.tracks
                            .binary_search_by(|track| track.path.cmp(&entry.track_path))
                            .is_ok()
                    }))
        });
    let rekordbox = if uses_rekordbox {
        match read_rekordbox(library) {
            Ok(rekordbox) => Some(rekordbox),
            Err(error) => {
                notes.push(format!("rekordbox metadata is unavailable ({error:#})."));
                None
            }
        }
    } else {
        None
    };
    let matching = ExportLibrary::new(
        rekordbox
            .as_ref()
            .map(|(tracks, directory)| RekordboxCollection {
                tracks,
                cues: &[],
                share_directory: directory,
            }),
        &scans,
        &relinks,
    );
    let cache_path = data.analysis_cache();
    let analysis = match cache_path.try_exists() {
        Ok(false) => None,
        Ok(true) => match AnalysisCache::open_read_only(&cache_path) {
            Ok(cache) => Some(cache),
            Err(error) => {
                notes.push(format!(
                    "Dubplate's analysis cache could not be read ({error})."
                ));
                None
            }
        },
        Err(error) => {
            notes.push(format!(
                "The analysis cache at {} could not be checked ({error}).",
                cache_path.display()
            ));
            None
        }
    };
    let stems = StemsIndex::open(&data.stems());
    let sources = Sources {
        matching,
        folders: &folders,
        analysis: analysis.as_ref(),
        stems: &stems,
    };
    let mut tracks = HashMap::with_capacity(set.entries.len());
    for entry in &set.entries {
        tracks
            .entry(entry.track_path.clone())
            .or_insert_with(|| sources.resolve(entry, &mut notes));
    }
    note_incomplete_seams(set, &tracks, &mut notes);
    Ok((tracks, notes))
}

fn read_rekordbox(library: Option<PathBuf>) -> Result<(Vec<Track>, PathBuf)> {
    let library = open_library(&master_db_or_default(library)?)?;
    Ok((library.tracks()?, library.share_directory().to_path_buf()))
}

struct Sources<'a> {
    matching: ExportLibrary<'a>,
    folders: &'a [PathBuf],
    analysis: Option<&'a AnalysisCache>,
    stems: &'a StemsIndex,
}

impl Sources<'_> {
    fn resolve(&self, entry: &SetEntry, notes: &mut Vec<String>) -> CheckSource {
        let library_path = self.matching.current_path(entry);
        let path = if entry.track_path.is_file() {
            &entry.track_path
        } else {
            &library_path
        };
        let canonical_path = canonical(path);
        let file_found = path.is_file();
        let rekordbox = self
            .matching
            .rekordbox_track(entry)
            .filter(|_| library_path == *path || canonical(&library_path) == canonical_path);
        let analysis =
            self.analysis.and_then(
                |cache| match cached_analysis(cache, path, &canonical_path) {
                    Ok(analysis) => analysis,
                    Err(error) => {
                        notes.push(format!(
                            "Could not read cached analysis for {} ({error}).",
                            path.display()
                        ));
                        None
                    }
                },
            );
        let stored = if let Some(rekordbox) = rekordbox {
            Some(rekordbox.clone())
        } else {
            let scanned = self
                .matching
                .scanned_track(&canonical_path)
                .or_else(|| self.matching.scanned_track(path));
            if file_found
                && is_audio_file(path)
                && (scanned.is_some()
                    || in_folders(&canonical_path, self.folders)
                    || entry.rekordbox_id.is_some()
                    || path != &entry.track_path)
            {
                match read_audio_metadata(path, scanned) {
                    Ok(metadata) => Some(folder_track(&metadata)),
                    Err(error) => {
                        notes.push(format!(
                            "Could not read audio tags for {} ({error}).",
                            path.display()
                        ));
                        None
                    }
                }
            } else {
                scanned.map(folder_track)
            }
        };
        let track = match stored {
            Some(track) => Some(track),
            None if analysis.is_some() => Some(file_track(path)),
            None => None,
        }
        .and_then(|mut track| {
            track.file_path = Some(canonical_path.clone());
            mixing_metadata(&track, rekordbox.is_some(), analysis.as_ref())
        });
        let vocals = self
            .stems
            .lookup(path)
            .or_else(|| self.stems.lookup(&canonical_path))
            .and_then(|stems| stems.cached_vocal_presence());
        CheckSource {
            file_found,
            track,
            vocals,
        }
    }
}

fn cached_analysis(
    cache: &AnalysisCache,
    path: &Path,
    canonical_path: &Path,
) -> Result<Option<AudioAnalysis>, dubplate_prep::AnalysisCacheError> {
    let analysis = cache.get(path)?;
    if analysis.is_none() && canonical_path != path {
        return cache.get(canonical_path);
    }
    Ok(analysis)
}

fn mixing_metadata(
    track: &Track,
    rekordbox: bool,
    analysed: Option<&AudioAnalysis>,
) -> Option<PrepTrack> {
    let energy = analysed
        .and_then(|analysis| analysis.energy.as_ref())
        .map(|energy| f32::from(energy.rating));
    let mut prep = PrepTrack::from_rekordbox(track, energy)?;
    if !rekordbox {
        prep.rekordbox_id = None;
    }
    if track.title.is_none() {
        prep.title = prep
            .track_path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
    }
    prep.bpm = preferred_track_value(
        rekordbox,
        positive_bpm(prep.bpm),
        positive_bpm(analysed.and_then(|analysis| analysis.bpm)),
    );
    prep.key = preferred_track_value(
        rekordbox,
        prep.key,
        analysed.and_then(|analysis| analysis.key),
    );
    Some(prep)
}

fn positive_bpm(bpm: Option<f32>) -> Option<f32> {
    bpm.filter(|value| value.is_finite() && *value > 0.0)
}

fn in_folders(path: &Path, folders: &[PathBuf]) -> bool {
    folders.iter().any(|folder| path.starts_with(folder))
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn note_incomplete_seams(
    set: &Set,
    tracks: &HashMap<PathBuf, CheckSource>,
    notes: &mut Vec<String>,
) {
    let entries: Vec<_> = set.main_entries().collect();
    if entries.len() < 2 {
        return;
    }
    let (mut tempos, mut keys, mut energies, mut vocals) = (0, 0, 0, 0);
    let seam_checks: Vec<_> = (0..entries.len() - 1)
        .map(|index| {
            !set.seam_after(index)
                .is_some_and(|seam| seam.status == SeamStatus::Free)
        })
        .collect();
    for (index, entry) in entries.into_iter().enumerate() {
        if !seam_checks.get(index).copied().unwrap_or(false)
            && !index
                .checked_sub(1)
                .is_some_and(|previous| seam_checks[previous])
        {
            continue;
        }
        let source = tracks.get(&entry.track_path);
        let track = source.and_then(|source| source.track.as_ref());
        tempos += usize::from(track.and_then(|track| track.bpm).is_none());
        keys += usize::from(track.and_then(|track| track.key).is_none());
        energies += usize::from(
            entry
                .energy_override
                .or_else(|| track.and_then(|track| track.energy))
                .is_none(),
        );
        vocals += usize::from(source.and_then(|source| source.vocals).is_none());
    }
    let missing = [
        ("tempo", tempos),
        ("key", keys),
        ("energy", energies),
        ("vocals", vocals),
    ]
    .into_iter()
    .filter(|(_, count)| *count > 0)
    .map(|(field, count)| format!("{field} for {count}"))
    .collect::<Vec<_>>();
    if !missing.is_empty() {
        notes.push(format!(
            "Some transition checks have incomplete metadata ({}).",
            missing.join(", ")
        ));
    }
}
