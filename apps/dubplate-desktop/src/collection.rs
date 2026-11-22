//! The library as the browser and the decks use it: every track from the DJ's music folders and,
//! when they chose to import it, the rekordbox collection, with the text the table shows, its
//! hot cues from `master.db`, and the crates.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dubplate_library::{FolderScan, LibraryTrack};
use dubplate_prep::{RekordboxCollection, file_track, folder_track};
use dubplate_rekordbox::{
    Cue, DatabaseKey, HotCuePad, Library, Playlist, PlaylistKind, Track, default_master_db,
};
use gpui::SharedString;

use crate::search::SearchText;

/// The position of a track in the collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TrackIndex(usize);

/// The position of a crate in the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct CrateIndex(usize);

impl CrateIndex {
    /// The whole collection, always first in the sidebar.
    pub const COLLECTION: Self = Self(0);

    /// Its position in the sidebar.
    pub fn position(self) -> usize {
        self.0
    }
}

/// A hot cue as rekordbox saved it in `master.db`.
#[derive(Debug, Clone, PartialEq)]
pub struct HotCue {
    /// Pad A to H.
    pub pad: HotCuePad,
    /// Where the cue is, in seconds from the start of the track.
    pub seconds: f64,
}

/// A track of the collection with what the browser and the decks need of it.
#[derive(Debug, Clone)]
pub struct CollectionTrack {
    /// The track as rekordbox describes it.
    pub track: Track,
    /// Whether it comes from the rekordbox collection.
    pub origin: TrackOrigin,
    /// The text of its row in the browser.
    pub row: RowText,
    /// Its hot cues, ordered by pad.
    pub hot_cues: Vec<HotCue>,
    /// Its title and artist folded for search.
    pub search_text: SearchText,
}

/// Where a track on a deck comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackOrigin {
    /// The rekordbox collection, with its analysis and cues.
    Rekordbox,
    /// One of the DJ's music folders.
    Folder,
    /// An audio file loaded on its own.
    File,
}

/// The cells of a browser row, formatted once when the collection loads.
#[derive(Debug, Clone, PartialEq)]
pub struct RowText {
    /// The title, or the file name when rekordbox has none.
    pub title: SharedString,
    /// The artist.
    pub artist: SharedString,
    /// The tempo with two decimals.
    pub bpm: SharedString,
    /// The key as rekordbox shows it.
    pub key: SharedString,
    /// The length as minutes and seconds.
    pub time: SharedString,
    /// Where the track comes from: its music folder's name, or rekordbox.
    pub source: SharedString,
}

/// What a crate in the sidebar is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrateKind {
    /// Every track.
    Collection,
    /// A rekordbox playlist.
    Playlist,
    /// A rekordbox folder of playlists.
    Folder,
    /// A rekordbox smart playlist, whose rules Dubplate does not run yet.
    Smart,
    /// One of the DJ's music folders, with every track in it and its subfolders.
    MusicFolder,
}

/// An entry of the sidebar.
#[derive(Debug, Clone, PartialEq)]
pub struct Crate {
    /// The name shown.
    pub name: SharedString,
    /// How deep in rekordbox's folders it sits; 0 at the top.
    pub depth: usize,
    /// Collection, playlist, folder or smart playlist.
    pub kind: CrateKind,
    /// Its tracks, in order.
    pub tracks: Vec<TrackIndex>,
}

/// rekordbox's library, read from a copy of `master.db`.
#[derive(Debug, Clone)]
pub struct RekordboxLibrary {
    tracks: Vec<Track>,
    cues: Vec<Cue>,
    playlists: Vec<Playlist>,
    share_directory: PathBuf,
}

impl RekordboxLibrary {
    /// Reads the library at rekordbox's usual place with the key from the user's configuration.
    /// It copies `master.db` and reads the copy, so this belongs on a background thread.
    pub fn load_default() -> Result<Self, dubplate_rekordbox::Error> {
        let key = DatabaseKey::from_user_configuration()?;
        let library = Library::open_copy(&default_master_db()?, &key)?;
        Ok(Self::new(
            library.tracks()?,
            library.cues()?,
            library.playlists()?,
            library.share_directory(),
        ))
    }

    /// A library holding these tracks, cues and playlists.
    pub fn new(
        tracks: Vec<Track>,
        cues: Vec<Cue>,
        playlists: Vec<Playlist>,
        share_directory: &Path,
    ) -> Self {
        Self {
            tracks,
            cues,
            playlists,
            share_directory: share_directory.to_path_buf(),
        }
    }

    /// How many tracks rekordbox holds.
    pub fn track_count(&self) -> usize {
        self.tracks.len()
    }

    /// The tracks, cues and analysis folder, as the export to rekordbox reads them.
    pub fn collection(&self) -> RekordboxCollection<'_> {
        RekordboxCollection {
            tracks: &self.tracks,
            cues: &self.cues,
            share_directory: &self.share_directory,
        }
    }
}

/// Every track Dubplate knows, from the DJ's music folders and rekordbox.
#[derive(Debug, Clone)]
pub struct Collection {
    tracks: Vec<Arc<CollectionTrack>>,
    crates: Vec<Crate>,
    share_directory: Option<PathBuf>,
    by_identity: HashMap<String, TrackIndex>,
    by_path: HashMap<PathBuf, TrackIndex>,
}

impl Collection {
    /// The collection without any relinked file.
    #[cfg(test)]
    pub fn build(rekordbox: Option<&RekordboxLibrary>, folders: &[FolderScan]) -> Self {
        Self::build_relinked(rekordbox, folders, &HashMap::new())
    }

    /// Builds the collection from rekordbox, when imported, and the scans of the music folders,
    /// in the order the DJ added them. A file in rekordbox and in a folder is listed once, as
    /// rekordbox's track with its cues and analysis; a file in two folders is listed once too.
    /// A rekordbox track whose file the DJ relinked plays from `relinks`' new place, which
    /// Dubplate keeps itself: rekordbox's own library is never written.
    pub fn build_relinked(
        rekordbox: Option<&RekordboxLibrary>,
        folders: &[FolderScan],
        relinks: &HashMap<PathBuf, PathBuf>,
    ) -> Self {
        let mut tracks: Vec<Arc<CollectionTrack>> = Vec::new();
        let mut crates = vec![Crate {
            name: "Collection".into(),
            depth: 0,
            kind: CrateKind::Collection,
            tracks: Vec::new(),
        }];
        let mut index_by_path: HashMap<PathBuf, TrackIndex> = HashMap::new();
        let mut playlist_crates = Vec::new();
        if let Some(rekordbox) = rekordbox {
            let mut cues_by_track: HashMap<&str, Vec<&Cue>> = HashMap::new();
            for cue in &rekordbox.cues {
                cues_by_track.entry(&cue.track_id).or_default().push(cue);
            }
            for original in &rekordbox.tracks {
                let mut track = original.clone();
                if let Some(relinked) = track.file_path.as_ref().and_then(|path| relinks.get(path))
                {
                    track.file_path = Some(relinked.clone());
                }
                let track = &track;
                let track_cues = cues_by_track
                    .get(track.id.as_str())
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                let index = TrackIndex(tracks.len());
                if let Some(path) = &track.file_path {
                    index_by_path.insert(canonical(path), index);
                }
                tracks.push(Arc::new(CollectionTrack::new(
                    track.clone(),
                    track_cues,
                    TrackOrigin::Rekordbox,
                    "rekordbox",
                )));
            }
            let index_by_id: HashMap<&str, TrackIndex> = tracks
                .iter()
                .enumerate()
                .map(|(position, entry)| (entry.track.id.as_str(), TrackIndex(position)))
                .collect();
            add_playlist_crates(
                &rekordbox.playlists,
                None,
                0,
                &index_by_id,
                &mut playlist_crates,
            );
        }
        for scan in folders {
            let name = folder_name(&scan.folder);
            let mut folder_tracks = Vec::with_capacity(scan.tracks.len());
            for found in &scan.tracks {
                let index = *index_by_path.entry(found.path.clone()).or_insert_with(|| {
                    let index = TrackIndex(tracks.len());
                    tracks.push(Arc::new(CollectionTrack::from_folder(found, &name)));
                    index
                });
                folder_tracks.push(index);
            }
            crates.push(Crate {
                name: name.into(),
                depth: 0,
                kind: CrateKind::MusicFolder,
                tracks: folder_tracks,
            });
        }
        crates[0].tracks = (0..tracks.len()).map(TrackIndex).collect();
        crates.extend(playlist_crates);
        let by_identity = tracks
            .iter()
            .enumerate()
            .map(|(position, track)| (track.identity(), TrackIndex(position)))
            .collect();
        let by_path = tracks
            .iter()
            .enumerate()
            .filter_map(|(position, track)| {
                Some((track.track.file_path.clone()?, TrackIndex(position)))
            })
            .collect();
        Self {
            tracks,
            crates,
            share_directory: rekordbox.map(|rekordbox| rekordbox.share_directory.clone()),
            by_identity,
            by_path,
        }
    }

    /// How many tracks the collection holds.
    pub fn track_count(&self) -> usize {
        self.tracks.len()
    }

    /// The track that is the file or rekordbox entry `identity` names, if the collection holds
    /// it.
    pub fn find(&self, identity: &str) -> Option<TrackIndex> {
        self.by_identity.get(identity).copied()
    }

    /// The track whose audio file is `path`, if the collection holds it.
    pub fn find_path(&self, path: &Path) -> Option<TrackIndex> {
        self.by_path.get(path).copied()
    }

    /// Every track with its position, in the collection's order.
    pub fn indexed_tracks(&self) -> impl Iterator<Item = (TrackIndex, &Arc<CollectionTrack>)> {
        self.tracks
            .iter()
            .enumerate()
            .map(|(position, track)| (TrackIndex(position), track))
    }

    /// The track at `index`.
    pub fn track(&self, index: TrackIndex) -> &Arc<CollectionTrack> {
        &self.tracks[index.0]
    }

    /// The crate at `index`, or the collection when there is none.
    pub fn crate_at(&self, index: CrateIndex) -> &Crate {
        self.crates.get(index.0).unwrap_or(&self.crates[0])
    }

    /// Every crate with its position.
    pub fn indexed_crates(&self) -> impl Iterator<Item = (CrateIndex, &Crate)> {
        self.crates
            .iter()
            .enumerate()
            .map(|(position, entry)| (CrateIndex(position), entry))
    }

    /// rekordbox's `share` directory, which holds the analysis files, when rekordbox is
    /// imported.
    pub fn share_directory(&self) -> Option<&Path> {
        self.share_directory.as_deref()
    }
}

impl CollectionTrack {
    /// An audio file that is not in the rekordbox collection, such as one dropped from Finder.
    /// It has no cues, beat grid or tempo.
    pub fn from_file(path: &Path) -> Self {
        Self::new(file_track(path), &[], TrackOrigin::File, "")
    }

    /// A track found in the music folder called `folder`.
    fn from_folder(found: &LibraryTrack, folder: &str) -> Self {
        Self::new(folder_track(found), &[], TrackOrigin::Folder, folder)
    }

    /// A track from `origin` with no cues, for tests elsewhere in the app.
    #[cfg(test)]
    pub fn for_tests(track: Track, origin: TrackOrigin) -> Self {
        Self::new(track, &[], origin, "")
    }

    fn new(track: Track, cues: &[&Cue], origin: TrackOrigin, source: &str) -> Self {
        let mut hot_cues: Vec<HotCue> = cues
            .iter()
            .filter_map(|cue| {
                let pad = cue.kind.pad()?;
                Some(HotCue {
                    pad,
                    seconds: milliseconds_to_seconds(cue.start_ms),
                })
            })
            .collect();
        hot_cues.sort_by_key(|cue| cue.pad);
        hot_cues.dedup_by_key(|cue| cue.pad);
        let title = track.title.clone().unwrap_or_else(|| file_name(&track));
        let artist = track.artist.clone().unwrap_or_default();
        let search_text = SearchText::new([title.as_str(), artist.as_str()]);
        let row = RowText {
            title: title.into(),
            artist: artist.into(),
            bpm: track
                .bpm
                .map(|bpm| format!("{bpm:.2}"))
                .unwrap_or_default()
                .into(),
            key: track.key.clone().unwrap_or_default().into(),
            time: track
                .length_seconds
                .map(|seconds| clock(f64::from(seconds)))
                .unwrap_or_default()
                .into(),
            source: SharedString::from(source.to_owned()),
        };
        Self {
            track,
            origin,
            row,
            hot_cues,
            search_text,
        }
    }

    /// What tells this track apart from every other: its rekordbox id, or its file's path.
    pub fn identity(&self) -> String {
        match (self.origin, &self.track.file_path) {
            (TrackOrigin::Folder | TrackOrigin::File, Some(path)) => path.display().to_string(),
            _ => self.track.id.clone(),
        }
    }
}

fn add_playlist_crates(
    playlists: &[Playlist],
    parent_id: Option<&str>,
    depth: usize,
    index_by_id: &HashMap<&str, TrackIndex>,
    crates: &mut Vec<Crate>,
) {
    let mut children: Vec<&Playlist> = playlists
        .iter()
        .filter(|playlist| playlist.parent_id.as_deref() == parent_id)
        .collect();
    children.sort_by_key(|playlist| playlist.position);
    for playlist in children {
        let kind = match playlist.kind {
            PlaylistKind::Playlist => CrateKind::Playlist,
            PlaylistKind::Folder => CrateKind::Folder,
            PlaylistKind::Smart => CrateKind::Smart,
            PlaylistKind::Unknown(_) => continue,
        };
        crates.push(Crate {
            name: playlist.name.clone().into(),
            depth,
            kind,
            tracks: playlist
                .track_ids
                .iter()
                .filter_map(|id| index_by_id.get(id.as_str()).copied())
                .collect(),
        });
        if kind == CrateKind::Folder {
            add_playlist_crates(
                playlists,
                Some(&playlist.id),
                depth + 1,
                index_by_id,
                crates,
            );
        }
    }
}

/// The name a music folder shows in the sidebar: its last component.
pub fn folder_name(folder: &Path) -> String {
    folder
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| folder.display().to_string())
}

/// `path` with symbolic links resolved when the file is there, so the same file is recognised
/// whichever way it was reached.
fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn file_name(track: &Track) -> String {
    track
        .file_path
        .as_deref()
        .and_then(Path::file_stem)
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn milliseconds_to_seconds(milliseconds: u32) -> f64 {
    f64::from(milliseconds) / 1000.0
}

/// `seconds` as `m:ss`, rounded down to the second.
pub fn clock(seconds: f64) -> String {
    let whole_seconds = seconds.max(0.0).floor() as u64;
    format!("{}:{:02}", whole_seconds / 60, whole_seconds % 60)
}

#[cfg(test)]
mod tests {
    use dubplate_rekordbox::CueKind;

    use super::*;

    fn track(id: &str, title: &str) -> Track {
        Track {
            id: id.to_owned(),
            title: Some(title.to_owned()),
            artist: Some("Artist".to_owned()),
            album: None,
            genre: None,
            label: None,
            key: Some("8A".to_owned()),
            bpm: Some(124.0),
            length_seconds: Some(408),
            file_path: None,
            file_size: None,
            rating: None,
            comment: None,
            analysis_path: None,
            date_added: None,
        }
    }

    fn cue(track_id: &str, kind: i64, start_ms: u32) -> Cue {
        Cue {
            id: format!("{track_id}-{kind}-{start_ms}"),
            track_id: track_id.to_owned(),
            kind: match kind {
                0 => CueKind::Memory,
                other => CueKind::Hot {
                    pad: HotCuePad::from_rekordbox_kind(other).unwrap(),
                },
            },
            start_ms,
            loop_end_ms: None,
            color: None,
            comment: None,
        }
    }

    fn playlist(id: &str, parent: Option<&str>, kind: PlaylistKind, tracks: &[&str]) -> Playlist {
        Playlist {
            id: id.to_owned(),
            parent_id: parent.map(str::to_owned),
            name: id.to_owned(),
            kind,
            position: 0,
            track_ids: tracks
                .iter()
                .map(|track_id| (*track_id).to_owned())
                .collect(),
        }
    }

    #[test]
    fn rows_are_formatted_once_with_cues_by_pad() {
        let collection = Collection::build(
            Some(&RekordboxLibrary::new(
                vec![track("1", "Saltwater Hymn")],
                vec![cue("1", 2, 45_000), cue("1", 1, 41), cue("1", 0, 90_500)],
                Vec::new(),
                Path::new("/share"),
            )),
            &[],
        );
        let entry = collection.track(TrackIndex(0));
        assert_eq!(entry.row.bpm.as_ref(), "124.00");
        assert_eq!(entry.row.time.as_ref(), "6:48");
        let pads: Vec<char> = entry.hot_cues.iter().map(|cue| cue.pad.letter()).collect();
        assert_eq!(pads, ['A', 'B']);
        let pad_b = entry
            .hot_cues
            .iter()
            .find(|cue| cue.pad == HotCuePad::B)
            .unwrap();
        assert!((pad_b.seconds - 45.0).abs() < 1e-9);
    }

    #[test]
    fn crates_follow_the_folder_tree_after_the_collection() {
        let collection = Collection::build(
            Some(&RekordboxLibrary::new(
                vec![track("1", "One"), track("2", "Two")],
                Vec::new(),
                vec![
                    playlist("Sets", None, PlaylistKind::Folder, &[]),
                    playlist("Peak", Some("Sets"), PlaylistKind::Playlist, &["2", "gone"]),
                    playlist("New", None, PlaylistKind::Smart, &[]),
                ],
                Path::new("/share"),
            )),
            &[],
        );
        let crates: Vec<(&str, usize, usize)> = collection
            .indexed_crates()
            .map(|(_, entry)| (entry.name.as_ref(), entry.depth, entry.tracks.len()))
            .collect();
        assert_eq!(
            crates,
            [
                ("Collection", 0, 2),
                ("Sets", 0, 0),
                ("Peak", 1, 1),
                ("New", 0, 0)
            ]
        );
        assert_eq!(collection.crate_at(CrateIndex(2)).tracks, [TrackIndex(1)]);
    }

    #[test]
    fn a_file_outside_the_collection_is_named_after_the_file() {
        let entry = CollectionTrack::from_file(Path::new("/Music/Artist - Title.flac"));
        assert_eq!(entry.row.title.as_ref(), "Artist - Title");
        assert_eq!(entry.origin, TrackOrigin::File);
        assert!(entry.hot_cues.is_empty());
        assert_eq!(entry.row.bpm.as_ref(), "");
    }

    fn found(path: &str, title: Option<&str>) -> LibraryTrack {
        LibraryTrack {
            path: PathBuf::from(path),
            title: title.map(str::to_owned),
            artist: Some("Artist".to_owned()),
            album: None,
            bpm: Some(122.0),
            key: Some("9A".to_owned()),
            duration_seconds: Some(300.4),
            size: 1,
            modified_ms: 1,
        }
    }

    fn folder(path: &str, tracks: Vec<LibraryTrack>) -> FolderScan {
        FolderScan {
            folder: PathBuf::from(path),
            tracks,
            problems: Vec::new(),
            scanned_at_ms: 0,
        }
    }

    #[test]
    fn music_folders_become_crates_and_a_file_is_listed_once() {
        let mut in_rekordbox = track("1", "From rekordbox");
        in_rekordbox.file_path = Some(PathBuf::from("/Music/House/Shared.mp3"));
        let rekordbox = RekordboxLibrary::new(
            vec![in_rekordbox],
            Vec::new(),
            Vec::new(),
            Path::new("/share"),
        );
        let folders = [
            folder(
                "/Music/House",
                vec![
                    found("/Music/House/Shared.mp3", Some("Tagged differently")),
                    found("/Music/House/Deep/Only Here.flac", None),
                ],
            ),
            folder(
                "/Music/House/Deep",
                vec![found("/Music/House/Deep/Only Here.flac", None)],
            ),
        ];
        let collection = Collection::build(Some(&rekordbox), &folders);
        assert_eq!(collection.track_count(), 2);
        let crates: Vec<(&str, CrateKind, usize)> = collection
            .indexed_crates()
            .map(|(_, entry)| (entry.name.as_ref(), entry.kind, entry.tracks.len()))
            .collect();
        assert_eq!(
            crates,
            [
                ("Collection", CrateKind::Collection, 2),
                ("House", CrateKind::MusicFolder, 2),
                ("Deep", CrateKind::MusicFolder, 1),
            ]
        );
        let shared = collection.track(TrackIndex(0));
        assert_eq!(shared.origin, TrackOrigin::Rekordbox);
        assert_eq!(shared.row.source.as_ref(), "rekordbox");
        let only_here = collection.track(TrackIndex(1));
        assert_eq!(only_here.origin, TrackOrigin::Folder);
        assert_eq!(only_here.row.title.as_ref(), "Only Here");
        assert_eq!(only_here.row.source.as_ref(), "House");
        assert_eq!(only_here.row.bpm.as_ref(), "122.00");
        assert_eq!(only_here.row.time.as_ref(), "5:00");
        assert_eq!(collection.find(&only_here.identity()), Some(TrackIndex(1)));
        assert!(collection.share_directory().is_some());
    }

    #[test]
    fn without_rekordbox_there_is_no_analysis_folder() {
        let collection = Collection::build(None, &[folder("/Music", Vec::new())]);
        assert_eq!(collection.track_count(), 0);
        assert!(collection.share_directory().is_none());
    }

    #[test]
    fn clock_shows_minutes_and_seconds() {
        assert_eq!(clock(0.0), "0:00");
        assert_eq!(clock(408.9), "6:48");
        assert_eq!(clock(-3.0), "0:00");
    }
}
