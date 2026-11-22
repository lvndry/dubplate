//! Relinking rekordbox tracks whose files moved: which tracks are missing, and the new places the
//! DJ accepted, which Dubplate keeps in its own settings and never writes into rekordbox.

use std::collections::HashMap;
use std::path::PathBuf;

use dubplate_library::{MissingTrack, Relink, Relocation};

use crate::collection::{Collection, TrackIndex, TrackOrigin};

/// A rekordbox track whose file is not where it should be.
#[derive(Debug, Clone, PartialEq)]
pub struct Missing {
    /// The track in the collection.
    pub track: TrackIndex,
    /// What the relocation search needs, by the path rekordbox gave.
    pub file: MissingTrack,
}

/// The rekordbox tracks of `collection` whose file is not on disk, each by the path rekordbox
/// gave, so a file relinked once and moved again is found from where rekordbox put it. Disk
/// work: run it off the interface thread.
pub fn missing_tracks(
    collection: &Collection,
    relinks: &HashMap<PathBuf, PathBuf>,
) -> Vec<Missing> {
    let originals: HashMap<&PathBuf, &PathBuf> =
        relinks.iter().map(|(from, to)| (to, from)).collect();
    collection
        .indexed_tracks()
        .filter(|(_, entry)| entry.origin == TrackOrigin::Rekordbox)
        .filter_map(|(index, entry)| {
            let current = entry.track.file_path.as_ref()?;
            if current.is_file() {
                return None;
            }
            let original = originals.get(current).copied().unwrap_or(current);
            Some(Missing {
                track: index,
                file: MissingTrack {
                    path: original.clone(),
                    size: entry.track.file_size,
                    duration_seconds: entry.track.length_seconds.map(|seconds| seconds as f32),
                },
            })
        })
        .collect()
}

/// `relinks` with `accepted` added, each replacing an earlier relink of the same file.
pub fn with_relocations(relinks: &[Relink], accepted: &[Relocation]) -> Vec<Relink> {
    let mut updated: Vec<Relink> = relinks
        .iter()
        .filter(|relink| {
            !accepted
                .iter()
                .any(|relocation| relocation.missing == relink.from)
        })
        .cloned()
        .collect();
    updated.extend(accepted.iter().map(|relocation| Relink {
        from: relocation.missing.clone(),
        to: relocation.found.clone(),
    }));
    updated
}

#[cfg(test)]
mod tests {
    use dubplate_library::{LibrarySettings, RelocationConfidence};
    use dubplate_rekordbox::Track;

    use super::*;
    use crate::collection::RekordboxLibrary;

    fn rekordbox_track(id: &str, path: PathBuf) -> Track {
        Track {
            id: id.to_owned(),
            title: Some(id.to_owned()),
            artist: None,
            album: None,
            genre: None,
            label: None,
            key: None,
            bpm: Some(124.0),
            length_seconds: Some(300),
            file_path: Some(path),
            file_size: Some(1_234),
            rating: None,
            comment: None,
            analysis_path: None,
            date_added: None,
        }
    }

    #[test]
    fn missing_tracks_are_named_by_rekordbox_path_and_relinked_ones_play() {
        let directory = tempfile::tempdir().unwrap();
        let present = directory.path().join("present.mp3");
        let moved_to = directory.path().join("moved.mp3");
        std::fs::write(&present, b"audio").unwrap();
        std::fs::write(&moved_to, b"audio").unwrap();
        let gone = directory.path().join("gone.mp3");
        let relinked_from = directory.path().join("old").join("moved.mp3");
        let rekordbox = RekordboxLibrary::new(
            vec![
                rekordbox_track("present", present),
                rekordbox_track("gone", gone.clone()),
                rekordbox_track("moved", relinked_from.clone()),
            ],
            Vec::new(),
            Vec::new(),
            directory.path(),
        );
        let relinks = HashMap::from([(relinked_from, moved_to.clone())]);
        let collection = Collection::build_relinked(Some(&rekordbox), &[], &relinks);
        let missing = missing_tracks(&collection, &relinks);
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].file.path, gone);
        assert_eq!(missing[0].file.size, Some(1_234));
        assert_eq!(missing[0].file.duration_seconds, Some(300.0));
        assert!(
            collection.find_path(&moved_to).is_some(),
            "the relinked file is the track's"
        );
    }

    fn relink(from: &str, to: &str) -> Relink {
        Relink {
            from: PathBuf::from(from),
            to: PathBuf::from(to),
        }
    }

    #[test]
    fn a_new_relink_of_a_file_replaces_the_old_one() {
        let relinks = vec![
            relink("/old/a.mp3", "/one/a.mp3"),
            relink("/old/b.mp3", "/one/b.mp3"),
        ];
        let accepted = [Relocation {
            missing: PathBuf::from("/old/a.mp3"),
            found: PathBuf::from("/two/a.mp3"),
            confidence: RelocationConfidence::SameSize,
        }];
        let updated = with_relocations(&relinks, &accepted);
        assert_eq!(
            updated,
            [
                relink("/old/b.mp3", "/one/b.mp3"),
                relink("/old/a.mp3", "/two/a.mp3")
            ]
        );
        let map = LibrarySettings {
            relinks: updated,
            ..LibrarySettings::default()
        }
        .relink_map();
        assert_eq!(
            map.get(&PathBuf::from("/old/a.mp3")),
            Some(&PathBuf::from("/two/a.mp3"))
        );
    }
}
