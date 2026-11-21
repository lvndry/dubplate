//! The rekordbox collection as the browser and the decks use it: every track with the text the
//! table shows, its hot cues from `master.db`, and the crates.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

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

/// The collection read from a copy of rekordbox's library.
#[derive(Debug, Clone)]
pub struct Collection {
    tracks: Vec<Arc<CollectionTrack>>,
    crates: Vec<Crate>,
    share_directory: PathBuf,
}

impl Collection {
    /// Reads the library at rekordbox's usual place with the key from the user's configuration.
    /// It copies `master.db` and reads the copy, so this belongs on a background thread.
    pub fn load_default() -> Result<Self, dubplate_rekordbox::Error> {
        let key = DatabaseKey::from_user_configuration()?;
        let library = Library::open_copy(&default_master_db()?, &key)?;
        Ok(Self::new(
            library.tracks()?,
            &library.cues()?,
            &library.playlists()?,
            library.share_directory(),
        ))
    }

    /// Builds the collection from what the library holds.
    pub fn new(
        tracks: Vec<Track>,
        cues: &[Cue],
        playlists: &[Playlist],
        share_directory: &Path,
    ) -> Self {
        let mut cues_by_track: HashMap<&str, Vec<&Cue>> = HashMap::new();
        for cue in cues {
            cues_by_track.entry(&cue.track_id).or_default().push(cue);
        }
        let tracks: Vec<Arc<CollectionTrack>> = tracks
            .into_iter()
            .map(|track| {
                let track_cues = cues_by_track
                    .get(track.id.as_str())
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                Arc::new(CollectionTrack::new(
                    track,
                    track_cues,
                    TrackOrigin::Rekordbox,
                ))
            })
            .collect();
        let index_by_id: HashMap<&str, TrackIndex> = tracks
            .iter()
            .enumerate()
            .map(|(position, entry)| (entry.track.id.as_str(), TrackIndex(position)))
            .collect();
        let mut crates = vec![Crate {
            name: "Collection".into(),
            depth: 0,
            kind: CrateKind::Collection,
            tracks: (0..tracks.len()).map(TrackIndex).collect(),
        }];
        add_playlist_crates(playlists, None, 0, &index_by_id, &mut crates);
        Self {
            tracks,
            crates,
            share_directory: share_directory.to_path_buf(),
        }
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

    /// rekordbox's `share` directory, which holds the analysis files.
    pub fn share_directory(&self) -> &Path {
        &self.share_directory
    }
}

impl CollectionTrack {
    /// An audio file that is not in the rekordbox collection, such as one dropped from Finder.
    /// It has no cues, beat grid or tempo.
    pub fn from_file(path: &Path) -> Self {
        let track = Track {
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
            rating: None,
            comment: None,
            analysis_path: None,
            date_added: None,
        };
        Self::new(track, &[], TrackOrigin::File)
    }

    fn new(track: Track, cues: &[&Cue], origin: TrackOrigin) -> Self {
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
        };
        Self {
            track,
            origin,
            row,
            hot_cues,
            search_text,
        }
    }

    /// The hot cue on `pad`, if one is set.
    pub fn hot_cue(&self, pad: HotCuePad) -> Option<&HotCue> {
        self.hot_cues.iter().find(|cue| cue.pad == pad)
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
        let collection = Collection::new(
            vec![track("1", "Saltwater Hymn")],
            &[cue("1", 2, 45_000), cue("1", 1, 41), cue("1", 0, 90_500)],
            &[],
            Path::new("/share"),
        );
        let entry = collection.track(TrackIndex(0));
        assert_eq!(entry.row.bpm.as_ref(), "124.00");
        assert_eq!(entry.row.time.as_ref(), "6:48");
        let pads: Vec<char> = entry.hot_cues.iter().map(|cue| cue.pad.letter()).collect();
        assert_eq!(pads, ['A', 'B']);
        assert!((entry.hot_cue(HotCuePad::B).unwrap().seconds - 45.0).abs() < 1e-9);
    }

    #[test]
    fn crates_follow_the_folder_tree_after_the_collection() {
        let collection = Collection::new(
            vec![track("1", "One"), track("2", "Two")],
            &[],
            &[
                playlist("Sets", None, PlaylistKind::Folder, &[]),
                playlist("Peak", Some("Sets"), PlaylistKind::Playlist, &["2", "gone"]),
                playlist("New", None, PlaylistKind::Smart, &[]),
            ],
            Path::new("/share"),
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

    #[test]
    fn clock_shows_minutes_and_seconds() {
        assert_eq!(clock(0.0), "0:00");
        assert_eq!(clock(408.9), "6:48");
        assert_eq!(clock(-3.0), "0:00");
    }
}
