//! The export itself: audio files, analysis files and the Device Library, written to the stick
//! in that order, so a library on the stick only ever points at files that are there.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use dubplate_engine::probe_file;
use dubplate_prep::Key;
use dubplate_rekordbox::analysis::AnalysisFileKind;
use dubplate_rekordbox::device_library::{
    AnalysisPath, DeviceLibrary, DeviceTrack, FileType, LibraryId, TrackId, analysis_path,
    export_ext_pdb,
};
use jiff::civil::Date;

use crate::analysis::analysis_files;
use crate::contents::{contents_path, numbered};
use crate::error::UsbExportError;
use crate::files::{copy_atomically, same_contents, write_atomically};
use crate::library_builder::LibraryBuilder;
use crate::request::{UsbPlaylist, UsbTrack};
use crate::stick::{Manifest, Stick, StickLibrary};

/// How far an export has come, for a progress bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsbExportProgress {
    /// Steps finished.
    pub done: usize,
    /// Steps in all: one a track, then one for the library.
    pub total: usize,
    /// What is being done now.
    pub step: UsbExportStep,
}

/// What an export is doing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsbExportStep {
    /// Copying a track and writing its analysis.
    Track {
        /// The track's title.
        title: String,
    },
    /// Writing the library players browse.
    Library,
}

/// What an export did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsbExportReport {
    /// The stick or folder.
    pub destination: PathBuf,
    /// Tracks the export put on the stick, each counted once.
    pub tracks: usize,
    /// Audio files copied.
    pub copied: usize,
    /// Audio files already on the stick, byte for byte, and left as they were.
    pub already_there: usize,
    /// The playlists written.
    pub playlists: Vec<String>,
    /// What could not be exported as it should, one sentence each.
    pub warnings: Vec<String>,
}

/// The most a title, a name or a comment keeps, so every track row fits in a page.
const TEXT_LIMIT: usize = 255;
const FNV_OFFSET: u32 = 0x811c_9dc5;
const FNV_PRIME: u32 = 0x0100_0193;
const DEFAULT_SAMPLE_DEPTH: u16 = 16;
const DATE_LENGTH: usize = 10;

/// Exports `playlists` to the stick or folder at `destination`, as rekordbox's Export mode
/// does: each track's audio under `Contents`, its analysis under `PIONEER/USBANLZ`, and the
/// library in `PIONEER/rekordbox`. `today` dates the analysis and the new library.
///
/// A stick with a library Dubplate wrote is added to: its tracks stay, a playlist of the same
/// name is replaced. A stick with a library another program wrote is refused. Files Dubplate
/// did not write are never replaced or removed; a track whose place is taken by such a file
/// goes beside it with a number. Disk and decoding work: run it off the interface thread.
pub fn export_to_usb(
    destination: &Path,
    playlists: &[UsbPlaylist],
    today: Date,
    progress: &mut dyn FnMut(UsbExportProgress),
) -> Result<UsbExportReport, UsbExportError> {
    let stick = Stick::open(destination)?;
    let unique: Vec<&UsbTrack> = unique_tracks(playlists);
    if unique.is_empty() {
        return Err(UsbExportError::Empty);
    }
    let (existing, mut manifest) = stick.library()?;
    let today_text = today.to_string();
    let (library, has_export_ext) = match existing {
        StickLibrary::Empty => (
            DeviceLibrary::new(new_library_id(destination), today_text.clone()),
            false,
        ),
        StickLibrary::Dubplate {
            library,
            has_export_ext,
        } => (*library, has_export_ext),
    };
    let library_id = library.library_id;
    let mut builder = LibraryBuilder::new(library);
    for file in Stick::library_files() {
        manifest.record(file);
    }
    stick.write_manifest(&manifest)?;

    let mut export = Export {
        stick: &stick,
        manifest,
        today: today_text,
        planned_paths: HashMap::new(),
        analysis_paths: builder
            .library()
            .tracks
            .iter()
            .map(|track| (track.analyze_path.clone(), track.file_path.clone()))
            .collect(),
        report: UsbExportReport {
            destination: stick.root().to_path_buf(),
            tracks: unique.len(),
            copied: 0,
            already_there: 0,
            playlists: playlists
                .iter()
                .map(|playlist| playlist.name.clone())
                .collect(),
            warnings: Vec::new(),
        },
    };
    let total = unique.len() + 1;
    let mut ids: HashMap<&Path, TrackId> = HashMap::new();
    for (done, track) in unique.iter().enumerate() {
        progress(UsbExportProgress {
            done,
            total,
            step: UsbExportStep::Track {
                title: title_of(track),
            },
        });
        let id = export.track(track, &mut builder)?;
        ids.insert(track.source.as_path(), id);
    }

    progress(UsbExportProgress {
        done: unique.len(),
        total,
        step: UsbExportStep::Library,
    });
    for playlist in playlists {
        let tracks: Vec<TrackId> = playlist
            .tracks
            .iter()
            .filter_map(|track| ids.get(track.source.as_path()).copied())
            .collect();
        builder.put_playlist(&clamped(&playlist.name), &tracks);
    }
    let library = builder.finish();
    write_atomically(&stick.export_pdb(), &library.to_export_pdb()?)?;
    if !has_export_ext {
        write_atomically(&stick.export_ext_pdb(), &export_ext_pdb(library_id)?)?;
    }
    stick.write_manifest(&export.manifest)?;
    progress(UsbExportProgress {
        done: total,
        total,
        step: UsbExportStep::Library,
    });
    Ok(export.report)
}

/// Today's date where this computer is, which dates an export.
pub fn local_today() -> Date {
    jiff::tz::TimeZone::system()
        .to_datetime(jiff::Timestamp::now())
        .date()
}

/// Each track once, by its audio file, in the order the playlists first name it.
fn unique_tracks(playlists: &[UsbPlaylist]) -> Vec<&UsbTrack> {
    let mut seen = HashSet::new();
    playlists
        .iter()
        .flat_map(|playlist| &playlist.tracks)
        .filter(|track| seen.insert(track.source.as_path()))
        .collect()
}

struct Export<'a> {
    stick: &'a Stick,
    manifest: Manifest,
    today: String,
    /// The stick paths this export gives its tracks, each with its audio file.
    planned_paths: HashMap<String, PathBuf>,
    /// The analysis file of every track on the stick, with the track's stick path.
    analysis_paths: HashMap<String, String>,
    report: UsbExportReport,
}

impl Export<'_> {
    /// Puts `track` on the stick: its audio, its analysis and its row.
    fn track(
        &mut self,
        track: &UsbTrack,
        builder: &mut LibraryBuilder,
    ) -> Result<TrackId, UsbExportError> {
        let (device_path, needs_copy) = self.place_audio(track)?;
        let analysis = self.place_analysis(&device_path);
        for kind in ANALYSIS_KINDS {
            self.manifest.record(analysis.file(kind));
        }
        if needs_copy {
            self.manifest.record(device_path.clone());
        }
        self.stick.write_manifest(&self.manifest)?;

        let target = self.stick.file(&device_path);
        if needs_copy {
            copy_atomically(&track.source, &target)?;
            self.report.copied += 1;
        } else {
            self.report.already_there += 1;
        }
        let (files, facts) = analysis_files(track, &device_path, &mut self.report.warnings)?;
        for kind in ANALYSIS_KINDS {
            write_atomically(&self.stick.file(&analysis.file(kind)), files.file(kind))?;
        }

        let id = builder.track_id(&device_path);
        let row = self.row(
            track,
            id,
            &device_path,
            analysis.to_string(),
            facts.duration_seconds,
            builder,
        )?;
        builder.put_track(row);
        Ok(id)
    }

    /// Where `track`'s audio goes on the stick, and whether it has to be copied there. A place
    /// that holds a file Dubplate did not write, or that another track of this export takes, is
    /// passed over for a numbered one.
    fn place_audio(&mut self, track: &UsbTrack) -> Result<(String, bool), UsbExportError> {
        if !track.source.is_file() {
            return Err(UsbExportError::Io {
                path: track.source.clone(),
                source: std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "the audio file is not there",
                ),
            });
        }
        let file_name = track
            .source
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let first_choice = contents_path(
            track.tags.artist.as_deref(),
            track.tags.album.as_deref(),
            &file_name,
        );
        let mut number = 0;
        loop {
            number += 1;
            let candidate = if number == 1 {
                first_choice.clone()
            } else {
                numbered(&first_choice, number)
            };
            if self
                .planned_paths
                .get(&candidate)
                .is_some_and(|source| *source != track.source)
            {
                continue;
            }
            let target = self.stick.file(&candidate);
            let decision = if !target.exists() {
                Some(true)
            } else if same_contents(&track.source, &target)? {
                Some(false)
            } else if self.manifest.wrote(&candidate) {
                Some(true)
            } else {
                None
            };
            if let Some(needs_copy) = decision {
                self.planned_paths
                    .insert(candidate.clone(), track.source.clone());
                return Ok((candidate, needs_copy));
            }
        }
    }

    /// The analysis files of the track at `device_path`: rekordbox's folder for it, numbered
    /// past any another track's analysis already takes.
    fn place_analysis(&mut self, device_path: &str) -> AnalysisPath {
        let base = analysis_path(device_path);
        let chosen = (0..=u16::MAX)
            .map(|number| base.numbered(number))
            .find(|candidate| {
                self.analysis_paths
                    .get(&candidate.to_string())
                    .is_none_or(|owner| owner == device_path)
            })
            .unwrap_or(base);
        self.analysis_paths
            .insert(chosen.to_string(), device_path.to_owned());
        chosen
    }

    fn row(
        &mut self,
        track: &UsbTrack,
        id: TrackId,
        device_path: &str,
        analyze_path: String,
        decoded_seconds: Option<f64>,
        builder: &mut LibraryBuilder,
    ) -> Result<DeviceTrack, UsbExportError> {
        let tags = &track.tags;
        let target = self.stick.file(device_path);
        let file_size = std::fs::metadata(&target)
            .map_err(UsbExportError::io(&target))?
            .len();
        let header = match probe_file(&track.source) {
            Ok(header) => Some(header),
            Err(error) => {
                self.report.warnings.push(format!(
                    "{}: its sample rate could not be read ({error}).",
                    track.source.display()
                ));
                None
            }
        };
        let duration = track
            .duration_seconds
            .or(decoded_seconds)
            .filter(|seconds| seconds.is_finite() && *seconds > 0.0);
        if track.beat_grid.is_none() {
            self.report.warnings.push(format!(
                "{} has no beat grid: players cannot sync it or show its bars.",
                title_of(track)
            ));
        }
        let bpm = track.bpm.filter(|bpm| *bpm > 0.0).or_else(|| {
            track
                .beat_grid
                .as_ref()
                .and_then(|grid| grid.beats.first())
                .map(|beat| beat.bpm())
        });
        let artist = builder.artist(tags.artist.as_deref().map(clamped).as_deref());
        let album = builder.album(tags.album.as_deref().map(clamped).as_deref(), artist);
        let extension = track
            .source
            .extension()
            .map(|extension| extension.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(DeviceTrack {
            id,
            title: clamped(&title_of(track)),
            artist,
            composer: None,
            original_artist: None,
            remixer: None,
            album,
            genre: builder.genre(tags.genre.as_deref().map(clamped).as_deref()),
            label: builder.label(tags.label.as_deref().map(clamped).as_deref()),
            key: builder.key(tags.key.as_deref().map(key_name).as_deref()),
            artwork: None,
            color: tags.color,
            rating: tags.rating.min(5),
            tempo_hundredths: bpm.map_or(0, |bpm| (bpm * 100.0).round() as u32),
            duration_seconds: duration
                .map_or(0, |seconds| seconds.round().min(f64::from(u16::MAX)) as u16),
            sample_rate: header.map_or(0, |header| header.sample_rate),
            sample_depth: header
                .and_then(|header| header.bits_per_sample)
                .and_then(|bits| u16::try_from(bits).ok())
                .unwrap_or(DEFAULT_SAMPLE_DEPTH),
            bitrate: duration.map_or(0, |seconds| {
                (file_size as f64 * 8.0 / seconds / 1000.0).round() as u32
            }),
            file_size: u32::try_from(file_size).unwrap_or(u32::MAX),
            file_type: FileType::from_extension(&extension),
            track_number: 0,
            disc_number: 0,
            year: tags.year.unwrap_or(0),
            play_count: 0,
            checksum: fnv1a(device_path),
            file_path: device_path.to_owned(),
            analyze_path,
            analyze_date: self.today.clone(),
            date_added: tags
                .date_added
                .as_deref()
                .and_then(date_part)
                .unwrap_or(&self.today)
                .to_owned(),
            release_date: String::new(),
            comment: tags.comment.as_deref().map(clamped).unwrap_or_default(),
            mix_name: String::new(),
            isrc: String::new(),
            lyricist: String::new(),
            message: String::new(),
            kuvo_public: false,
            autoload_hot_cues: true,
        })
    }
}

const ANALYSIS_KINDS: [AnalysisFileKind; 3] = [
    AnalysisFileKind::Dat,
    AnalysisFileKind::Ext,
    AnalysisFileKind::TwoEx,
];

/// The title players show: the tags' title, else the file name without its extension.
fn title_of(track: &UsbTrack) -> String {
    track
        .tags
        .title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            track
                .source
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default()
        })
}

/// The key as rekordbox's key table names it in its classic notation, such as `Am` or `Db`;
/// a key Dubplate cannot read is kept as written.
fn key_name(key: &str) -> String {
    Key::parse_file_key(key).map_or_else(|| clamped(key), |key| key.short_name())
}

/// `text` cut to [`TEXT_LIMIT`] characters.
fn clamped(text: &str) -> String {
    text.chars().take(TEXT_LIMIT).collect()
}

/// The `yyyy-mm-dd` at the start of `text`, when it starts with one.
fn date_part(text: &str) -> Option<&str> {
    let date = text.get(..DATE_LENGTH)?;
    date.parse::<Date>().is_ok().then_some(date)
}

fn fnv1a(text: &str) -> u32 {
    text.bytes().fold(FNV_OFFSET, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(FNV_PRIME)
    })
}

/// A number for a new library, different on every stick.
fn new_library_id(destination: &Path) -> LibraryId {
    let nanoseconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            elapsed.subsec_nanos() ^ elapsed.as_secs() as u32
        });
    LibraryId(fnv1a(&destination.to_string_lossy()) ^ nanoseconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_written_in_rekordbox_s_classic_notation() {
        assert_eq!(key_name("8A"), "Am");
        assert_eq!(key_name("Am"), "Am");
        assert_eq!(key_name("3B"), "Db");
        assert_eq!(key_name("weird"), "weird");
    }

    #[test]
    fn only_a_leading_date_is_kept() {
        assert_eq!(date_part("2024-02-26 10:11:12"), Some("2024-02-26"));
        assert_eq!(date_part("26/02/2024"), None);
    }
}
