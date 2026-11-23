//! Reads what players wrote back to a rekordbox USB stick since Dubplate exported it: the
//! histories players recorded in `export.pdb`, and the hot cues, memory cues and saved loops
//! players added or moved on a CDJ, which players save in each track's analysis files.
//!
//! Everything is read only: the files are opened, read and closed, and nothing on the stick is
//! ever changed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::analysis::{AnalysisFile, AnalysisFileKind, TrackAnalysis};
use crate::device_library::{AnalysisPath, DatabaseFile, DeviceLibrary, DeviceTrack, TrackId};
use crate::track_cues::{CueChange, TrackCues};

/// One track played in a history, with what the stick's library says about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryTrack {
    /// The track row of the stick's library, or `None` when the library does not hold it.
    pub track: Option<DeviceTrack>,
}

/// A history players recorded on the stick, with its tracks in the order they played. The
/// format keeps no date for it, only its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct History {
    /// The history's name, such as `HISTORY 001`.
    pub name: String,
    /// The tracks in the order they played.
    pub tracks: Vec<HistoryTrack>,
}

/// The hot cues, memory cues and saved loops of one track as the stick's analysis files hold
/// them, or empty when the stick has no analysis file for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StickTrackCues {
    /// The cues as the stick has them.
    pub cues: TrackCues,
    /// The stick has no analysis file for the track, so a player could not have changed its
    /// cues: they are empty only because there is no file.
    pub missing: bool,
}

/// The stick's hot cues, memory cues and saved loops, by the stick's track id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StickTrackCuesMap {
    cues: HashMap<TrackId, StickTrackCues>,
}

impl StickTrackCuesMap {
    /// The cues `id` points at, when the stick holds a track with it.
    pub fn get(&self, id: TrackId) -> Option<&StickTrackCues> {
        self.cues.get(&id)
    }
}

/// What players changed on the stick since Dubplate exported it: the changes to each track's
/// cues, keyed by the stick's track id. A track of Dubplate's that the stick does not hold has
/// no changes reported, since the stick cannot say.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StickCueChanges {
    changes: HashMap<TrackId, Vec<CueChange>>,
}

impl StickCueChanges {
    /// The changes to the cues of `id`, empty when players did not touch them or the track is
    /// not on the stick.
    pub fn get(&self, id: TrackId) -> &[CueChange] {
        self.changes.get(&id).map_or(&[], Vec::as_slice)
    }

    /// Every track players changed a cue on, with its changes.
    pub fn changed(&self) -> impl Iterator<Item = (TrackId, &[CueChange])> + '_ {
        self.changes
            .iter()
            .filter(|(_, changes)| !changes.is_empty())
            .map(|(id, changes)| (*id, changes.as_slice()))
    }
}

/// What could not be read from a stick, or was not a stick Dubplate could read from.
#[derive(Debug, Error)]
pub enum StickImportError {
    /// The stick holds no rekordbox export: no `PIONEER/rekordbox/export.pdb`.
    #[error("{0} holds no rekordbox export: there is no PIONEER/rekordbox/export.pdb")]
    NoExport(PathBuf),
    /// A file could not be read.
    #[error("{0}: {1}")]
    Io(PathBuf, #[source] std::io::Error),
    /// `export.pdb` is not a Device Library.
    #[error("the export on {0} could not be read: {1}")]
    Library(PathBuf, #[source] crate::device_library::DeviceLibraryError),
    /// `exportExt.pdb` is not a DeviceSQL database.
    #[error("the exportExt.pdb beside the export could not be read: {0}")]
    ExportExt(#[source] crate::device_library::DeviceLibraryError),
    /// An analysis file is not in the format players write.
    #[error("the analysis at {0} is not in the format players write")]
    Analysis(PathBuf),
}

/// The root of the stick, once checked: it must be a folder.
pub(crate) struct MountedStick {
    root: PathBuf,
}

impl MountedStick {
    fn open(root: &Path) -> Result<Self, StickImportError> {
        if !root.is_dir() {
            let path = root.to_path_buf();
            return Err(StickImportError::Io(
                path,
                std::io::Error::new(std::io::ErrorKind::NotFound, "not a folder"),
            ));
        }
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    fn export_pdb_path(&self) -> PathBuf {
        self.root
            .join("PIONEER")
            .join("rekordbox")
            .join("export.pdb")
    }

    fn file(&self, device_path: &str) -> PathBuf {
        device_path
            .trim_start_matches('/')
            .split('/')
            .fold(self.root.clone(), |path, part| path.join(part))
    }
}

/// The stick's library, with its tracks indexed by their id.
pub(crate) struct StickLibrary {
    library: DeviceLibrary,
    tracks_by_id: HashMap<TrackId, DeviceTrack>,
    export_pdb: PathBuf,
}

impl StickLibrary {
    fn track(&self, id: TrackId) -> Option<&DeviceTrack> {
        self.tracks_by_id.get(&id)
    }
}

/// The stick's `export.pdb`, read and decoded, with its companion `exportExt.pdb` checked to
/// be a DeviceSQL database when it is there. Fails when the export is missing or does not
/// match the layout players write.
pub(crate) fn stick_library(root: &Path) -> Result<StickLibrary, StickImportError> {
    let stick = MountedStick::open(root)?;
    let export_pdb = stick.export_pdb_path();
    let bytes = match std::fs::read(&export_pdb) {
        Ok(bytes) => bytes,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            return Err(StickImportError::NoExport(stick.root));
        }
        Err(source) => return Err(StickImportError::Io(export_pdb, source)),
    };
    check_export_ext(&stick)?;
    let library = DeviceLibrary::parse_export_pdb(&bytes)
        .map_err(|source| StickImportError::Library(export_pdb.clone(), source))?;
    let tracks_by_id = library
        .tracks
        .iter()
        .map(|track| (track.id, track.clone()))
        .collect();
    Ok(StickLibrary {
        library,
        tracks_by_id,
        export_pdb,
    })
}

/// A rekordbox export always writes `exportExt.pdb` beside `export.pdb`. When that file is
/// there but does not match the layout, the stick is not a rekordbox export Dubplate can read.
fn check_export_ext(stick: &MountedStick) -> Result<(), StickImportError> {
    let export_ext = stick.export_pdb_path().with_file_name("exportExt.pdb");
    if !export_ext.is_file() {
        return Ok(());
    }
    let bytes =
        std::fs::read(&export_ext).map_err(|source| StickImportError::Io(export_ext, source))?;
    DatabaseFile::parse(&bytes)
        .map(|_| ())
        .map_err(StickImportError::ExportExt)
}

/// Reads the histories players recorded on the stick at `root`, each with its tracks in the
/// order they played, mapped to the stick's track rows.
///
/// Fails when `root` holds no rekordbox export or its `export.pdb` does not match the layout
/// players write. The stick is only read, never changed.
pub fn stick_histories(root: &Path) -> Result<Vec<History>, StickImportError> {
    let library = stick_library(root)?;
    let export_pdb = library.export_pdb.clone();
    let histories = library
        .library
        .histories()
        .map_err(|source| StickImportError::Library(export_pdb, source))?;
    Ok(histories
        .into_iter()
        .map(|history| History {
            name: history.name,
            tracks: history
                .tracks
                .into_iter()
                .map(|id| HistoryTrack {
                    track: library.track(id).cloned(),
                })
                .collect(),
        })
        .collect())
}

/// Reads every track's hot cues, memory cues and saved loops as the stick's analysis files
/// hold them. A track whose analysis files are missing is kept with empty cues and a flag;
/// a track whose analysis files are there but unreadable is an error.
pub fn stick_track_cues(root: &Path) -> Result<StickTrackCuesMap, StickImportError> {
    let stick = MountedStick::open(root)?;
    let library = stick_library(&stick.root)?;
    let mut cues = StickTrackCuesMap::default();
    for track in &library.library.tracks {
        let track_cues = read_stick_track_cues(&stick, track)?;
        cues.cues.insert(track.id, track_cues);
    }
    Ok(cues)
}

fn read_stick_track_cues(
    stick: &MountedStick,
    track: &DeviceTrack,
) -> Result<StickTrackCues, StickImportError> {
    let analysis_path = AnalysisPath::from_stored(&track.analyze_path)
        .ok_or_else(|| StickImportError::Analysis(stick.file(&track.analyze_path)))?;
    Ok(match read_analysis(stick, &analysis_path)? {
        Some(analysis) => StickTrackCues {
            cues: TrackCues::from_analysis(&analysis.cues),
            missing: false,
        },
        None => StickTrackCues {
            cues: TrackCues::default(),
            missing: true,
        },
    })
}

/// The track's analysis files on the stick, or `None` when the `.DAT` file is not there.
fn read_analysis(
    stick: &MountedStick,
    analysis_path: &AnalysisPath,
) -> Result<Option<TrackAnalysis>, StickImportError> {
    let mut files = Vec::new();
    for kind in [
        AnalysisFileKind::Dat,
        AnalysisFileKind::Ext,
        AnalysisFileKind::TwoEx,
    ] {
        let path = stick.file(&analysis_path.file(kind));
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(source)
                if source.kind() == std::io::ErrorKind::NotFound
                    && kind == AnalysisFileKind::Dat =>
            {
                return Ok(None);
            }
            Err(source)
                if source.kind() == std::io::ErrorKind::NotFound
                    && kind != AnalysisFileKind::Dat =>
            {
                continue;
            }
            Err(source) => return Err(StickImportError::Io(path, source)),
        };
        let file =
            AnalysisFile::parse(&bytes).map_err(|_| StickImportError::Analysis(path.clone()))?;
        files.push((kind, path, file));
    }
    Ok(Some(TrackAnalysis::from_files(files)))
}

/// What players changed on the stick at `root` against the cues Dubplate exported with it,
/// `dubplate` keyed by the stick's track id, in any order.
///
/// A cue within 1 ms of where Dubplate put it is the same cue. Fails when `root` holds no
/// rekordbox export. The stick is only read, never changed.
pub fn stick_cue_changes(
    root: &Path,
    dubplate: impl IntoIterator<Item = (TrackId, TrackCues)>,
) -> Result<StickCueChanges, StickImportError> {
    let stick = MountedStick::open(root)?;
    let library = stick_library(&stick.root)?;
    let mut changes = StickCueChanges::default();
    for (id, cues) in dubplate {
        let Some(track) = library.track(id) else {
            continue;
        };
        let stick_cues = read_stick_track_cues(&stick, track)?;
        let track_changes = stick_cues.cues.changes_from(&cues);
        if !track_changes.is_empty() {
            changes.changes.insert(id, track_changes);
        }
    }
    Ok(changes)
}
