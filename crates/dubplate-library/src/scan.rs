//! Walking a music folder and reading its tracks' tags.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use lofty::prelude::{Accessor, AudioFile, ItemKey, TaggedFileExt};
use serde::{Deserialize, Serialize};

/// The file types Dubplate plays, by extension.
pub const AUDIO_EXTENSIONS: [&str; 9] = [
    "mp3", "m4a", "mp4", "aac", "flac", "wav", "aif", "aiff", "ogg",
];

/// Whether a file's extension is one Dubplate plays, in any case.
pub fn is_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            AUDIO_EXTENSIONS
                .iter()
                .any(|audio| audio.eq_ignore_ascii_case(extension))
        })
}

/// A music folder that could not be scanned at all.
#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    /// The folder does not exist, or is not a folder.
    #[error("{0} is not a folder")]
    NotAFolder(PathBuf),
    /// The folder could not be read.
    #[error("{path} could not be read: {source}")]
    Unreadable {
        /// The folder.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
}

/// A track found in a music folder, with what its tags say.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LibraryTrack {
    /// The file, with symbolic links resolved.
    pub path: PathBuf,
    /// The title tag.
    pub title: Option<String>,
    /// The artist tag.
    pub artist: Option<String>,
    /// The album tag.
    pub album: Option<String>,
    /// The genre tag.
    #[serde(default)]
    pub genre: Option<String>,
    /// The comment tag.
    #[serde(default)]
    pub comment: Option<String>,
    /// The BPM tag.
    pub bpm: Option<f32>,
    /// The key tag, as written by the tagging software.
    pub key: Option<String>,
    /// The length of the audio.
    pub duration_seconds: Option<f32>,
    /// The file's size in bytes, to notice when it changes.
    pub size: u64,
    /// When the file last changed, in milliseconds since 1970, to notice when it changes.
    pub modified_ms: u64,
}

/// A file that was skipped, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanProblem {
    /// The file or folder.
    pub path: PathBuf,
    /// Why it was skipped.
    pub reason: String,
}

/// What one scan of a music folder found.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FolderScan {
    /// The folder, as the DJ chose it.
    pub folder: PathBuf,
    /// Its tracks, ordered by path, each file once.
    pub tracks: Vec<LibraryTrack>,
    /// The files and folders that were skipped because they could not be read.
    pub problems: Vec<ScanProblem>,
    /// When the scan finished, in milliseconds since 1970.
    pub scanned_at_ms: u64,
}

/// Scans `folder` and every folder in it for audio files, skipping hidden ones. Files that have
/// the same size and modification time as in `previous` keep what `previous` read from them,
/// without being opened. `progress` counts the files looked at, for showing progress while the
/// scan runs on another thread.
pub fn scan_folder(
    folder: &Path,
    previous: Option<&FolderScan>,
    progress: &AtomicUsize,
) -> Result<FolderScan, ScanError> {
    if !folder.is_dir() {
        return Err(ScanError::NotAFolder(folder.to_path_buf()));
    }
    let known: HashMap<&Path, &LibraryTrack> = previous
        .map(|scan| {
            scan.tracks
                .iter()
                .map(|track| (track.path.as_path(), track))
                .collect()
        })
        .unwrap_or_default();
    let mut tracks = Vec::new();
    let mut problems = Vec::new();
    let mut seen_files = HashSet::new();
    let mut seen_folders = HashSet::new();
    let mut pending = vec![folder.to_path_buf()];
    let mut first = true;
    while let Some(directory) = pending.pop() {
        let canonical = match directory.canonicalize() {
            Ok(canonical) => canonical,
            Err(source) if first => {
                return Err(ScanError::Unreadable {
                    path: directory,
                    source,
                });
            }
            Err(error) => {
                problems.push(problem(&directory, &error));
                keep_known_tracks_under(&directory, &known, &mut seen_files, &mut tracks);
                continue;
            }
        };
        if !seen_folders.insert(canonical.clone()) {
            continue;
        }
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(source) if first => {
                return Err(ScanError::Unreadable {
                    path: directory,
                    source,
                });
            }
            Err(error) => {
                problems.push(problem(&directory, &error));
                keep_known_tracks_under(&canonical, &known, &mut seen_files, &mut tracks);
                continue;
            }
        };
        first = false;
        for entry in entries {
            let path = match entry {
                Ok(entry) => entry.path(),
                Err(error) => {
                    problems.push(problem(&directory, &error));
                    continue;
                }
            };
            if is_hidden(&path) {
                continue;
            }
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            if !is_audio_file(&path) {
                continue;
            }
            progress.fetch_add(1, Ordering::Relaxed);
            match read_track(&path, &known) {
                Ok(track) => {
                    if seen_files.insert(track.path.clone()) {
                        tracks.push(track);
                    }
                }
                Err(reason) => problems.push(ScanProblem { path, reason }),
            }
        }
    }
    tracks.sort_by(|first, second| first.path.cmp(&second.path));
    problems.sort_by(|first, second| first.path.cmp(&second.path));
    Ok(FolderScan {
        folder: folder.to_path_buf(),
        tracks,
        problems,
        scanned_at_ms: milliseconds_since_epoch(SystemTime::now()),
    })
}

/// Keeps what the last scan found under a folder that cannot be read this time, so a busy disk
/// or a folder without permission never drops its tracks from the library.
fn keep_known_tracks_under(
    folder: &Path,
    known: &HashMap<&Path, &LibraryTrack>,
    seen_files: &mut HashSet<PathBuf>,
    tracks: &mut Vec<LibraryTrack>,
) {
    for (path, track) in known {
        if path.starts_with(folder) && seen_files.insert(path.to_path_buf()) {
            tracks.push((*track).clone());
        }
    }
}

/// The artists of a track as one line: every artist value, and every artist inside a value
/// some taggers separate with `;`, joined with commas as rekordbox shows them.
fn artists<'tag>(values: impl Iterator<Item = &'tag str>) -> Option<String> {
    let names: Vec<&str> = values
        .flat_map(|value| value.split(';'))
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect();
    (!names.is_empty()).then(|| names.join(", "))
}

pub(crate) fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with('.'))
}

fn problem(path: &Path, error: &dyn std::fmt::Display) -> ScanProblem {
    ScanProblem {
        path: path.to_path_buf(),
        reason: error.to_string(),
    }
}

fn milliseconds_since_epoch(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map(|since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or_default()
}

/// The track at `path`, reusing what `known` read when the file has not changed.
pub(crate) fn read_track(
    path: &Path,
    known: &HashMap<&Path, &LibraryTrack>,
) -> Result<LibraryTrack, String> {
    let path = path.canonicalize().map_err(|error| error.to_string())?;
    let metadata = std::fs::metadata(&path).map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Err("not a regular file".to_owned());
    }
    let size = metadata.len();
    let modified_ms = metadata
        .modified()
        .map(milliseconds_since_epoch)
        .unwrap_or_default();
    if let Some(track) = known.get(path.as_path())
        && track.size == size
        && track.modified_ms == modified_ms
    {
        return Ok((*track).clone());
    }
    let tagged = lofty::read_from_path(&path).map_err(|error| error.to_string())?;
    let duration = tagged.properties().duration();
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
    let text = |key: ItemKey| {
        tag.and_then(|tag| tag.get_string(key))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };
    let bpm = text(ItemKey::Bpm)
        .or_else(|| text(ItemKey::IntegerBpm))
        .and_then(|bpm| bpm.parse::<f32>().ok())
        .filter(|bpm| bpm.is_finite() && *bpm > 0.0);
    let accessor_text = |value: Option<std::borrow::Cow<'_, str>>| {
        value
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    };
    Ok(LibraryTrack {
        title: accessor_text(tag.and_then(|tag| tag.title())),
        artist: tag.and_then(|tag| artists(tag.get_strings(ItemKey::TrackArtist))),
        album: accessor_text(tag.and_then(|tag| tag.album())),
        genre: accessor_text(tag.and_then(|tag| tag.genre())),
        comment: accessor_text(tag.and_then(|tag| tag.comment())),
        bpm,
        key: text(ItemKey::InitialKey),
        duration_seconds: (!duration.is_zero()).then_some(duration.as_secs_f32()),
        size,
        modified_ms,
        path,
    })
}

#[cfg(test)]
mod tests {
    use lofty::config::WriteOptions;
    use lofty::prelude::TagExt;
    use lofty::tag::{Tag, TagType};

    use super::*;

    /// A tenth of a second of silent 16-bit stereo WAV.
    fn write_wav(path: &Path) {
        let sample_rate: u32 = 44_100;
        let frames = sample_rate / 10;
        let data_length = frames * 4;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_length).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&sample_rate.to_le_bytes());
        bytes.extend_from_slice(&(sample_rate * 4).to_le_bytes());
        bytes.extend_from_slice(&4_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_length.to_le_bytes());
        bytes.resize(bytes.len() + data_length as usize, 0);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, bytes).unwrap();
    }

    fn tag_wav(path: &Path, title: &str, artist: &str, bpm: &str, key: &str) {
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_title(title.to_owned());
        tag.set_artist(artist.to_owned());
        tag.set_album("Fixtures".to_owned());
        tag.set_genre("Amapiano".to_owned());
        tag.set_comment("Log drum from bar 33".to_owned());
        tag.insert_text(ItemKey::IntegerBpm, bpm.to_owned());
        tag.insert_text(ItemKey::InitialKey, key.to_owned());
        tag.save_to_path(path, WriteOptions::default()).unwrap();
    }

    fn scan(folder: &Path, previous: Option<&FolderScan>) -> FolderScan {
        scan_folder(folder, previous, &AtomicUsize::new(0)).unwrap()
    }

    fn names(scan: &FolderScan) -> Vec<String> {
        scan.tracks
            .iter()
            .map(|track| {
                track
                    .path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }

    #[test]
    fn tracks_in_nested_folders_are_found_with_their_tags() {
        let root = tempfile::tempdir().unwrap();
        let tagged = root.path().join("Deep/House/Tagged.wav");
        write_wav(&tagged);
        tag_wav(&tagged, "Saltwater Hymn", "Mara Okonjo", "124", "8A");
        write_wav(&root.path().join("Untagged.WAV"));
        let progress = AtomicUsize::new(0);
        let found = scan_folder(root.path(), None, &progress).unwrap();
        assert_eq!(names(&found), ["Tagged.wav", "Untagged.WAV"]);
        assert_eq!(progress.load(Ordering::Relaxed), 2);
        let track = &found.tracks[0];
        assert_eq!(track.title.as_deref(), Some("Saltwater Hymn"));
        assert_eq!(track.artist.as_deref(), Some("Mara Okonjo"));
        assert_eq!(track.album.as_deref(), Some("Fixtures"));
        assert_eq!(track.genre.as_deref(), Some("Amapiano"));
        assert_eq!(track.comment.as_deref(), Some("Log drum from bar 33"));
        assert_eq!(track.bpm, Some(124.0));
        assert_eq!(track.key.as_deref(), Some("8A"));
        let duration = track.duration_seconds.unwrap();
        assert!((duration - 0.1).abs() < 0.01, "{duration}");
        assert_eq!(found.tracks[1].title, None);
        assert!(found.problems.is_empty(), "{:?}", found.problems);
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_subfolder_keeps_the_tracks_the_last_scan_found_there() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        write_wav(&root.path().join("Top.wav"));
        let locked = root.path().join("Locked");
        write_wav(&locked.join("Inside.wav"));
        let first = scan(root.path(), None);
        assert_eq!(names(&first), ["Inside.wav", "Top.wav"]);
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let second = scan(root.path(), Some(&first));
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(names(&second), ["Inside.wav", "Top.wav"]);
        assert_eq!(second.problems.len(), 1, "{:?}", second.problems);
    }

    #[test]
    fn a_folder_that_disappears_is_an_error_not_an_empty_scan() {
        let root = tempfile::tempdir().unwrap();
        let gone = root.path().join("Unplugged");
        let result = scan_folder(&gone, None, &AtomicUsize::new(0));
        assert!(result.is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_pipe_named_like_audio_is_skipped_without_blocking() {
        let root = tempfile::tempdir().unwrap();
        let pipe = root.path().join("loop.mp3");
        let status = std::process::Command::new("mkfifo")
            .arg(&pipe)
            .status()
            .unwrap();
        assert!(status.success());
        write_wav(&root.path().join("Real.wav"));
        let found = scan(root.path(), None);
        assert_eq!(names(&found), ["Real.wav"]);
        assert_eq!(found.problems.len(), 1, "{:?}", found.problems);
    }

    #[test]
    fn hidden_and_non_audio_files_are_skipped() {
        let root = tempfile::tempdir().unwrap();
        write_wav(&root.path().join("Track.wav"));
        write_wav(&root.path().join(".Hidden.wav"));
        write_wav(&root.path().join(".hidden-folder/Inside.wav"));
        std::fs::write(root.path().join("cover.jpg"), b"jpeg").unwrap();
        std::fs::write(root.path().join("notes.txt"), b"notes").unwrap();
        let found = scan(root.path(), None);
        assert_eq!(names(&found), ["Track.wav"]);
        assert!(found.problems.is_empty());
    }

    #[test]
    fn an_unreadable_file_is_reported_and_the_rest_still_scan() {
        let root = tempfile::tempdir().unwrap();
        write_wav(&root.path().join("Good.wav"));
        std::fs::write(root.path().join("Broken.mp3"), b"not audio at all").unwrap();
        let found = scan(root.path(), None);
        assert_eq!(names(&found), ["Good.wav"]);
        assert_eq!(found.problems.len(), 1);
        assert!(found.problems[0].path.ends_with("Broken.mp3"));
    }

    #[cfg(unix)]
    #[test]
    fn a_file_reached_twice_is_listed_once_and_folder_loops_end() {
        let root = tempfile::tempdir().unwrap();
        let original = root.path().join("Album/Track.wav");
        write_wav(&original);
        std::os::unix::fs::symlink(&original, root.path().join("Link.wav")).unwrap();
        std::os::unix::fs::symlink(root.path(), root.path().join("Album/Loop")).unwrap();
        let found = scan(root.path(), None);
        assert_eq!(names(&found), ["Track.wav"]);
        assert_eq!(found.tracks[0].path, original.canonicalize().unwrap());
    }

    #[test]
    fn a_rescan_keeps_unchanged_files_and_notices_added_and_removed_ones() {
        let root = tempfile::tempdir().unwrap();
        let kept = root.path().join("Kept.wav");
        let removed = root.path().join("Removed.wav");
        write_wav(&kept);
        write_wav(&removed);
        let mut first = scan(root.path(), None);
        first.tracks[0].title = Some("Read before".to_owned());
        std::fs::remove_file(&removed).unwrap();
        write_wav(&root.path().join("Added.wav"));
        let second = scan(root.path(), Some(&first));
        assert_eq!(names(&second), ["Added.wav", "Kept.wav"]);
        assert_eq!(
            second.tracks[1].title.as_deref(),
            Some("Read before"),
            "an unchanged file is not read again"
        );
    }

    #[test]
    fn artists_read_as_one_line_with_commas() {
        assert_eq!(
            artists(["DJ Zinhle;Nayvee Womculo; Young Stunna"].into_iter()).as_deref(),
            Some("DJ Zinhle, Nayvee Womculo, Young Stunna")
        );
        assert_eq!(
            artists(["Shimza", "Kabza De Small"].into_iter()).as_deref(),
            Some("Shimza, Kabza De Small")
        );
        assert_eq!(artists([" ", ""].into_iter()), None);
    }

    #[test]
    fn a_missing_folder_is_an_error() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("Gone");
        assert!(matches!(
            scan_folder(&missing, None, &AtomicUsize::new(0)),
            Err(ScanError::NotAFolder(_))
        ));
        assert!(is_audio_file(Path::new("a.FLAC")));
        assert!(!is_audio_file(Path::new("a.mp3.zip")));
    }
}
