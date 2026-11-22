//! Finding the new home of a file that was moved after it was imported into rekordbox.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::scan::{LibraryTrack, is_audio_file, is_hidden, read_track};

/// A rekordbox track whose file is not where rekordbox says it is.
#[derive(Debug, Clone, PartialEq)]
pub struct MissingTrack {
    /// Where rekordbox says the file is.
    pub path: PathBuf,
    /// The size rekordbox recorded, in bytes, if it did.
    pub size: Option<u64>,
    /// The length rekordbox recorded, in seconds, if it did.
    pub duration_seconds: Option<f32>,
}

/// How sure a relocation is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelocationConfidence {
    /// The found file has the same size rekordbox recorded.
    SameSize,
    /// No matching size, but the found file's length is within two seconds of what rekordbox recorded.
    SameLength,
}

/// A track's file, found again at a new location.
#[derive(Debug, Clone, PartialEq)]
pub struct Relocation {
    /// Where rekordbox says the file is.
    pub missing: PathBuf,
    /// Where the file actually is now.
    pub found: PathBuf,
    /// How sure the match is.
    pub confidence: RelocationConfidence,
}

/// rekordbox records a track's length in whole seconds, cut down, and decoders disagree by a
/// fraction of a second on the same file, so a 7:24 in rekordbox can read 7:25.9 from the file.
const LENGTH_TOLERANCE_SECONDS: f32 = 2.0;

/// Finds, for each missing track, the library track that is its file at a new location.
///
/// Only candidates whose file name matches, ignoring case, are considered. A candidate with the
/// same size wins outright when the size is known; failing that, a candidate whose length is
/// within two seconds wins when both lengths are known. Two equally good candidates are never
/// guessed between, so an ambiguous name yields nothing. The result keeps the order of `missing`.
pub fn find_relocations(missing: &[MissingTrack], candidates: &[LibraryTrack]) -> Vec<Relocation> {
    // Overlapping source folders (and aliases of a folder) can report the same file more
    // than once. Ambiguity means distinct files, never duplicate scan results.
    let mut seen_paths = HashSet::new();
    let mut seen_identities = HashSet::new();
    let mut by_name: HashMap<String, Vec<&LibraryTrack>> = HashMap::new();
    for candidate in candidates {
        if !seen_paths.insert(&candidate.path) {
            continue;
        }
        let Some(name) = lowercase_name(&candidate.path) else {
            continue;
        };
        let identity = candidate
            .path
            .canonicalize()
            .unwrap_or_else(|_| candidate.path.clone());
        // The same physical file may have aliases with different names. Keep each usable
        // name, but count a physical file only once when assessing that name's ambiguity.
        if seen_identities.insert((name.clone(), identity)) {
            by_name.entry(name).or_default().push(candidate);
        }
    }
    missing
        .iter()
        .filter_map(|missing| {
            let candidates = by_name.get(&lowercase_name(&missing.path)?)?;
            find_one(missing, candidates)
        })
        .collect()
}

/// Chooses among distinct files whose names already match this missing file.
fn find_one(missing: &MissingTrack, candidates: &[&LibraryTrack]) -> Option<Relocation> {
    let mut same_size: Option<&Path> = None;
    let mut same_length: Option<&Path> = None;
    let mut length_is_ambiguous = false;
    for candidate in candidates {
        if candidate.path == missing.path {
            continue;
        }
        let size_matches = missing.size.is_some_and(|size| candidate.size == size);
        if size_matches {
            if same_size.is_some() {
                return None;
            }
            same_size = Some(candidate.path.as_path());
            continue;
        }
        let length_matches = match (missing.duration_seconds, candidate.duration_seconds) {
            (Some(missing_length), Some(candidate_length)) => {
                (missing_length - candidate_length).abs() <= LENGTH_TOLERANCE_SECONDS
            }
            _ => false,
        };
        if length_matches {
            length_is_ambiguous |= same_length.is_some();
            same_length = Some(candidate.path.as_path());
        }
    }
    let (found, confidence) = if let Some(found) = same_size {
        (found, RelocationConfidence::SameSize)
    } else if length_is_ambiguous {
        return None;
    } else {
        (same_length?, RelocationConfidence::SameLength)
    };
    Some(Relocation {
        missing: missing.path.clone(),
        found: found.to_path_buf(),
        confidence,
    })
}

fn lowercase_name(path: &Path) -> Option<String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_lowercase)
}

/// The audio files under `folders`, including every subfolder, whose file name is in `names`.
///
/// Hidden entries are skipped and a symlink loop cannot repeat a folder, so a folder the DJ
/// points at can be searched without it joining the library. A folder that cannot be read is
/// skipped, and the result is ordered by path.
pub fn search_folders_for(names: &HashSet<String>, folders: &[PathBuf]) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut seen_folders = HashSet::new();
    let mut pending: Vec<PathBuf> = folders.to_vec();
    while let Some(directory) = pending.pop() {
        let canonical = match directory.canonicalize() {
            Ok(canonical) => canonical,
            Err(_) => continue,
        };
        if !seen_folders.insert(canonical.clone()) {
            continue;
        }
        let entries = match std::fs::read_dir(&canonical) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries {
            let path = match entry {
                Ok(entry) => entry.path(),
                Err(_) => continue,
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
            let Some(name) = lowercase_name(&path) else {
                continue;
            };
            if names.contains(&name) {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// The library values of `paths`, reading size and length the same way the scanner does.
///
/// A path that cannot be read, or that is not an audio file, is skipped.
pub fn candidates_from_paths(paths: &[PathBuf]) -> Vec<LibraryTrack> {
    let none: HashMap<&Path, &LibraryTrack> = HashMap::new();
    let mut candidates = Vec::with_capacity(paths.len());
    for path in paths {
        if !is_audio_file(path) {
            continue;
        }
        if let Ok(track) = read_track(path, &none) {
            candidates.push(track);
        }
    }
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A WAV of silent 16-bit stereo audio, `frames` long at 44.1 kHz.
    fn write_wav(path: &Path, frames: u32) {
        let sample_rate: u32 = 44_100;
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

    fn candidate(path: &Path, size: u64, duration_seconds: Option<f32>) -> LibraryTrack {
        LibraryTrack {
            path: path.to_path_buf(),
            title: None,
            artist: None,
            album: None,
            bpm: None,
            key: None,
            duration_seconds,
            size,
            modified_ms: 0,
        }
    }

    fn moved_track() -> MissingTrack {
        MissingTrack {
            path: "/Music/afro house/warmup/Moved.mp3".into(),
            size: Some(1_000_000),
            duration_seconds: Some(300.0),
        }
    }

    fn new_home() -> PathBuf {
        Path::new("/Music/amapiano/warmup/Moved.mp3").to_path_buf()
    }

    #[test]
    fn a_moved_file_is_found_by_its_size() {
        let found = find_relocations(
            &[moved_track()],
            &[candidate(&new_home(), 1_000_000, Some(299.9))],
        );

        assert_eq!(
            found,
            vec![Relocation {
                missing: moved_track().path,
                found: new_home(),
                confidence: RelocationConfidence::SameSize,
            }]
        );
    }

    #[test]
    fn two_same_named_candidates_are_told_apart_by_their_size() {
        let decoy = Path::new("/Music/afro house/peak/Moved.mp3");
        let candidates = vec![
            candidate(decoy, 999_000, Some(300.0)),
            candidate(&new_home(), 1_000_000, Some(299.9)),
        ];

        let found = find_relocations(&[moved_track()], &candidates);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].found, new_home());
        assert_eq!(found[0].confidence, RelocationConfidence::SameSize);
    }

    #[cfg(unix)]
    #[test]
    fn differently_named_alias_does_not_hide_a_matching_file() {
        let directory = tempfile::tempdir().unwrap();
        let actual = directory.path().join("Moved.mp3");
        std::fs::write(&actual, b"audio").unwrap();
        let alias = directory.path().join("Other.mp3");
        std::os::unix::fs::symlink(&actual, &alias).unwrap();
        let found = find_relocations(
            &[moved_track()],
            &[
                candidate(&alias, 1_000_000, None),
                candidate(&actual, 1_000_000, None),
            ],
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].found, actual);
    }

    #[test]
    fn a_size_match_wins_even_after_two_length_matches() {
        let first_decoy = Path::new("/Music/afro house/peak/Moved.mp3");
        let second_decoy = Path::new("/Music/afro house/closing/Moved.mp3");
        let candidates = vec![
            candidate(first_decoy, 990_000, Some(300.0)),
            candidate(second_decoy, 980_000, Some(300.2)),
            candidate(&new_home(), 1_000_000, Some(299.9)),
        ];

        let found = find_relocations(&[moved_track()], &candidates);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].found, new_home());
        assert_eq!(found[0].confidence, RelocationConfidence::SameSize);
    }

    #[test]
    fn an_unknown_size_is_matched_by_length_within_two_seconds() {
        let missing = MissingTrack {
            path: "/Music/afro house/warmup/Moved.mp3".into(),
            size: None,
            duration_seconds: Some(300.0),
        };
        let far = Path::new("/Music/afro house/peak/Moved.mp3");
        let candidates = vec![
            candidate(far, 999_000, Some(295.0)),
            candidate(&new_home(), 1_000_000, Some(301.6)),
        ];

        let found = find_relocations(&[missing], &candidates);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].found, new_home());
        assert_eq!(found[0].confidence, RelocationConfidence::SameLength);
    }

    #[test]
    fn duplicate_scans_of_one_file_are_not_ambiguous() {
        let track = candidate(&new_home(), 1_000_000, Some(300.0));
        let found = find_relocations(&[moved_track()], &[track.clone(), track]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].found, new_home());
    }

    #[cfg(unix)]
    #[test]
    fn folder_aliases_of_one_file_are_not_ambiguous() {
        let directory = tempfile::tempdir().unwrap();
        let actual = directory.path().join("actual");
        std::fs::create_dir(&actual).unwrap();
        std::fs::write(actual.join("Moved.mp3"), b"audio").unwrap();
        let alias = directory.path().join("alias");
        std::os::unix::fs::symlink(&actual, &alias).unwrap();
        let candidates = [
            candidate(&actual.join("Moved.mp3"), 1_000_000, None),
            candidate(&alias.join("Moved.mp3"), 1_000_000, None),
        ];
        assert_eq!(find_relocations(&[moved_track()], &candidates).len(), 1);
    }

    #[test]
    fn two_equally_good_candidates_yield_nothing() {
        let by_size_missing = MissingTrack {
            path: "/Music/afro house/warmup/Moved.mp3".into(),
            size: Some(1_000_000),
            duration_seconds: None,
        };
        let first = Path::new("/Music/amapiano/warmup/Moved.mp3");
        let second = Path::new("/Music/afro house/peak/Moved.mp3");

        let by_size = find_relocations(
            &[by_size_missing],
            &[
                candidate(first, 1_000_000, None),
                candidate(second, 1_000_000, None),
            ],
        );
        assert_eq!(by_size, Vec::<Relocation>::new());

        let by_length_missing = MissingTrack {
            path: "/Music/afro house/warmup/Moved.mp3".into(),
            size: None,
            duration_seconds: Some(300.0),
        };
        let by_length = find_relocations(
            &[by_length_missing],
            &[
                candidate(first, 1_000_000, Some(299.5)),
                candidate(second, 1_000_001, Some(299.8)),
            ],
        );
        assert_eq!(by_length, Vec::<Relocation>::new());
    }

    #[test]
    fn a_candidate_at_the_missing_path_is_ignored() {
        let candidates = vec![candidate(
            Path::new("/Music/afro house/warmup/Moved.mp3"),
            1_000_000,
            Some(300.0),
        )];

        assert_eq!(
            find_relocations(&[moved_track()], &candidates),
            Vec::<Relocation>::new()
        );
    }

    #[test]
    fn different_file_names_yield_nothing() {
        let renamed = Path::new("/Music/amapiano/warmup/Moved (VIP).mp3");
        let candidates = vec![candidate(renamed, 1_000_000, Some(300.0))];

        assert_eq!(
            find_relocations(&[moved_track()], &candidates),
            Vec::<Relocation>::new()
        );
    }

    #[test]
    fn file_names_match_ignoring_case() {
        let missing = MissingTrack {
            path: "/Music/afro house/warmup/moved.MP3".into(),
            size: Some(1_000_000),
            duration_seconds: None,
        };

        let found = find_relocations(&[missing], &[candidate(&new_home(), 1_000_000, None)]);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].confidence, RelocationConfidence::SameSize);
    }

    #[test]
    fn search_folders_for_walks_nested_folders_and_skips_hidden_ones() {
        let root = tempfile::tempdir().unwrap();
        let nested = root.path().join("amapiano/warmup/Moved.mp3");
        write_wav(&nested, 4_410);
        write_wav(&root.path().join(".hidden/Moved.mp3"), 4_410);
        write_wav(&root.path().join("Other.mp3"), 4_410);
        let names: HashSet<String> = ["moved.mp3".to_owned()].into_iter().collect();

        let found = search_folders_for(&names, &[root.path().to_path_buf()]);

        assert_eq!(found, [nested.canonicalize().unwrap()]);
    }

    #[test]
    fn candidates_from_paths_read_size_and_duration_like_the_scanner() {
        let root = tempfile::tempdir().unwrap();
        let audio = root.path().join("Moved.wav");
        write_wav(&audio, 4_410);
        std::fs::write(root.path().join("notes.txt"), b"not audio").unwrap();
        let paths = vec![
            audio.clone(),
            root.path().join("notes.txt"),
            root.path().join("Gone.wav"),
        ];

        let candidates = candidates_from_paths(&paths);

        assert_eq!(
            candidates.len(),
            1,
            "non-audio and unreadable files are skipped"
        );
        assert_eq!(candidates[0].path, audio.canonicalize().unwrap());
        assert_eq!(candidates[0].size, std::fs::metadata(&audio).unwrap().len());
        let duration = candidates[0].duration_seconds.unwrap();
        assert!((duration - 0.1).abs() < 0.01, "{duration}");
    }
}
