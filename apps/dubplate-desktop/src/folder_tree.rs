//! The subfolders of a music folder, worked out from the paths of the tracks the scan found:
//! no walk of the disk, and a folder with no audio file anywhere below it never appears.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use crate::collection::{Collection, CrateIndex, CrateKind, TrackIndex};

/// One subfolder of a music folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderNode {
    /// The folder.
    pub path: PathBuf,
    /// How deep below the music folder it sits: 1 for a folder directly inside it.
    pub depth: usize,
    /// Whether it has subfolders with audio in them.
    pub has_children: bool,
    /// Every track in it and in the folders below it, in the order the files were given.
    pub tracks: Vec<TrackIndex>,
}

/// The subfolders of `root` that hold audio, each above its own subfolders and ordered by name
/// without regard to case. `files` are the tracks' paths; a file outside `root` is ignored.
pub fn folder_nodes(root: &Path, files: &[(PathBuf, TrackIndex)]) -> Vec<FolderNode> {
    let mut found: BTreeMap<Vec<String>, FolderNode> = BTreeMap::new();
    for (file, track) in files {
        let Some(parent) = file
            .parent()
            .and_then(|parent| parent.strip_prefix(root).ok())
        else {
            continue;
        };
        let mut path = root.to_path_buf();
        let mut key = Vec::new();
        for component in parent.components() {
            path.push(component);
            key.push(component.as_os_str().to_string_lossy().to_lowercase());
            found
                .entry(key.clone())
                .or_insert_with(|| FolderNode {
                    path: path.clone(),
                    depth: key.len(),
                    has_children: false,
                    tracks: Vec::new(),
                })
                .tracks
                .push(*track);
        }
    }
    let mut nodes: Vec<FolderNode> = found.into_values().collect();
    for position in 0..nodes.len().saturating_sub(1) {
        nodes[position].has_children = nodes[position + 1].depth > nodes[position].depth;
    }
    nodes
}

/// The crates the sidebar shows: all of them, except the subfolders under a music folder that
/// is collapsed, whichever of its folders `expanded` holds the path of.
pub fn visible_crates(collection: &Collection, expanded: &HashSet<PathBuf>) -> Vec<CrateIndex> {
    let mut shown = Vec::new();
    let mut hidden_below: Option<usize> = None;
    for (index, entry) in collection.indexed_crates() {
        let is_folder = entry.kind == CrateKind::MusicFolder;
        match hidden_below {
            Some(depth) if is_folder && entry.depth > depth => continue,
            _ => hidden_below = None,
        }
        shown.push(index);
        if is_folder
            && entry.has_subfolders
            && !entry
                .path
                .as_ref()
                .is_some_and(|path| expanded.contains(path))
        {
            hidden_below = Some(entry.depth);
        }
    }
    shown
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(paths: &[&str]) -> Vec<(PathBuf, TrackIndex)> {
        paths
            .iter()
            .enumerate()
            .map(|(position, path)| (PathBuf::from(path), TrackIndex::for_tests(position)))
            .collect()
    }

    fn summary(nodes: &[FolderNode]) -> Vec<(String, usize, bool, usize)> {
        nodes
            .iter()
            .map(|node| {
                (
                    node.path.display().to_string(),
                    node.depth,
                    node.has_children,
                    node.tracks.len(),
                )
            })
            .collect()
    }

    #[test]
    fn nested_folders_come_in_tree_order_with_every_track_below_them() {
        let nodes = folder_nodes(
            Path::new("/Music"),
            &files(&[
                "/Music/loose.mp3",
                "/Music/House/Deep/one.mp3",
                "/Music/House/Deep/two.mp3",
                "/Music/House/three.mp3",
                "/Music/afro/Warmup/four.mp3",
            ]),
        );
        assert_eq!(
            summary(&nodes),
            [
                ("/Music/afro".to_owned(), 1, true, 1),
                ("/Music/afro/Warmup".to_owned(), 2, false, 1),
                ("/Music/House".to_owned(), 1, true, 3),
                ("/Music/House/Deep".to_owned(), 2, false, 2),
            ]
        );
    }

    #[test]
    fn a_folder_without_audio_below_it_never_appears() {
        let nodes = folder_nodes(Path::new("/Music"), &files(&["/Music/a/one.mp3"]));
        assert_eq!(nodes.len(), 1);
        assert!(
            folder_nodes(Path::new("/Music"), &files(&["/Music/loose.mp3"])).is_empty(),
            "only the music folder itself holds audio"
        );
    }

    #[test]
    fn a_folder_holding_audio_only_in_a_subfolder_is_still_listed() {
        let nodes = folder_nodes(Path::new("/Music"), &files(&["/Music/a/b/c/one.mp3"]));
        assert_eq!(
            summary(&nodes),
            [
                ("/Music/a".to_owned(), 1, true, 1),
                ("/Music/a/b".to_owned(), 2, true, 1),
                ("/Music/a/b/c".to_owned(), 3, false, 1),
            ]
        );
    }

    #[test]
    fn collapsed_folders_hide_what_is_under_them() {
        use dubplate_library::{FolderScan, LibraryTrack};

        let scan = FolderScan {
            folder: PathBuf::from("/Music"),
            tracks: [
                "/Music/a/b/one.mp3",
                "/Music/a/two.mp3",
                "/Music/c/three.mp3",
            ]
            .iter()
            .map(|path| LibraryTrack {
                path: PathBuf::from(path),
                title: None,
                artist: None,
                album: None,
                genre: None,
                comment: None,
                bpm: None,
                key: None,
                duration_seconds: None,
                size: 1,
                modified_ms: 1,
            })
            .collect(),
            problems: Vec::new(),
            scanned_at_ms: 0,
        };
        let collection = Collection::build(None, &[scan]);
        let names = |expanded: &[&str]| -> Vec<String> {
            let expanded = expanded.iter().map(PathBuf::from).collect();
            visible_crates(&collection, &expanded)
                .into_iter()
                .map(|index| collection.crate_at(index).name.to_string())
                .collect()
        };
        assert_eq!(names(&[]), ["Collection", "Music"]);
        assert_eq!(names(&["/Music"]), ["Collection", "Music", "a", "c"]);
        assert_eq!(
            names(&["/Music", "/Music/a"]),
            ["Collection", "Music", "a", "b", "c"]
        );
        assert_eq!(
            names(&["/Music/a"]),
            ["Collection", "Music"],
            "a folder inside a collapsed one stays hidden"
        );
    }

    #[test]
    fn files_outside_the_music_folder_are_ignored() {
        let nodes = folder_nodes(Path::new("/Music"), &files(&["/Other/a/one.mp3"]));
        assert!(nodes.is_empty());
    }
}
