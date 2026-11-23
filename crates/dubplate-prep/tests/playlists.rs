//! The DJ's playlists and folders: made, renamed, moved and deleted in Dubplate's database,
//! holding tracks in the DJ's order, and kept across a relaunch.

use std::collections::HashMap;
use std::path::PathBuf;

use dubplate_prep::{
    PlaylistEdit, PlaylistId, PlaylistKind, PlaylistTrack, PlaylistTree, SetStore, StoreError,
    TrackIdentity,
};

fn file(path: &str) -> PlaylistTrack {
    PlaylistTrack {
        identity: TrackIdentity::File(PathBuf::from(path)),
        track_path: Some(PathBuf::from(path)),
    }
}

fn rekordbox(id: &str) -> PlaylistTrack {
    PlaylistTrack {
        identity: TrackIdentity::Rekordbox(id.to_owned()),
        track_path: None,
    }
}

#[allow(clippy::unwrap_used)]
fn create(
    store: &mut SetStore,
    parent: Option<PlaylistId>,
    name: &str,
    kind: PlaylistKind,
) -> PlaylistId {
    store
        .edit_playlists(PlaylistEdit::Create {
            parent,
            name: name.to_owned(),
            kind,
            tracks: Vec::new(),
        })
        .unwrap()
        .created
        .unwrap()
}

fn names(tree: &PlaylistTree) -> Vec<(String, usize)> {
    tree.entries()
        .iter()
        .map(|entry| (entry.name.clone(), entry.depth))
        .collect()
}

fn track_names(tree: &PlaylistTree, playlist: PlaylistId) -> Vec<String> {
    tree.tracks(playlist)
        .into_iter()
        .map(|track| match &track.identity {
            TrackIdentity::File(path) => path.to_string_lossy().into_owned(),
            TrackIdentity::Rekordbox(id) => format!("rekordbox {id}"),
        })
        .collect()
}

#[test]
fn playlists_and_folders_make_a_tree_that_survives_a_relaunch() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("dubplate.db");
    let mut store = SetStore::open(&path).unwrap();
    let gigs = create(&mut store, None, "Gigs", PlaylistKind::Folder);
    let warm_up = create(&mut store, Some(gigs), "Warm up", PlaylistKind::Playlist);
    let peak = create(&mut store, Some(gigs), "Peak", PlaylistKind::Playlist);
    let digging = create(&mut store, None, "Digging", PlaylistKind::Playlist);
    store
        .edit_playlists(PlaylistEdit::AddTracks {
            playlist: peak,
            tracks: vec![file("/Music/a.mp3"), rekordbox("7")],
        })
        .unwrap();
    drop(store);

    let store = SetStore::open(&path).unwrap();
    let tree = store.playlists().unwrap();
    assert_eq!(
        names(&tree),
        [
            ("Gigs".to_owned(), 0),
            ("Warm up".to_owned(), 1),
            ("Peak".to_owned(), 1),
            ("Digging".to_owned(), 0),
        ]
    );
    assert_eq!(track_names(&tree, peak), ["/Music/a.mp3", "rekordbox 7"]);
    assert!(tree.tracks(warm_up).is_empty());
    assert!(tree.tracks(digging).is_empty());
    assert_eq!(tree.get(peak).unwrap().parent, Some(gigs));
}

#[test]
fn adding_skips_tracks_the_playlist_holds_and_keeps_the_order() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = SetStore::open(&directory.path().join("dubplate.db")).unwrap();
    let peak = create(&mut store, None, "Peak", PlaylistKind::Playlist);
    let first = store
        .edit_playlists(PlaylistEdit::AddTracks {
            playlist: peak,
            tracks: vec![file("/a.mp3"), file("/b.mp3")],
        })
        .unwrap();
    assert_eq!((first.added, first.already_there), (2, 0));
    let second = store
        .edit_playlists(PlaylistEdit::AddTracks {
            playlist: peak,
            tracks: vec![file("/b.mp3"), file("/c.mp3"), file("/c.mp3")],
        })
        .unwrap();
    assert_eq!((second.added, second.already_there), (1, 2));
    let tree = store.playlists().unwrap();
    assert_eq!(track_names(&tree, peak), ["/a.mp3", "/b.mp3", "/c.mp3"]);
}

#[test]
fn tracks_go_into_a_playlist_where_the_dj_points() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = SetStore::open(&directory.path().join("dubplate.db")).unwrap();
    let peak = create(&mut store, None, "Peak", PlaylistKind::Playlist);
    store
        .edit_playlists(PlaylistEdit::AddTracks {
            playlist: peak,
            tracks: vec![file("/a.mp3"), file("/b.mp3"), file("/c.mp3")],
        })
        .unwrap();
    store
        .edit_playlists(PlaylistEdit::AddTracksAt {
            playlist: peak,
            position: 1,
            tracks: vec![file("/b.mp3"), file("/x.mp3"), file("/y.mp3")],
        })
        .unwrap();
    assert_eq!(
        track_names(&store.playlists().unwrap(), peak),
        ["/a.mp3", "/x.mp3", "/y.mp3", "/b.mp3", "/c.mp3"]
    );
    store
        .edit_playlists(PlaylistEdit::AddTracksAt {
            playlist: peak,
            position: usize::MAX,
            tracks: vec![file("/z.mp3")],
        })
        .unwrap();
    assert_eq!(
        track_names(&store.playlists().unwrap(), peak),
        ["/a.mp3", "/x.mp3", "/y.mp3", "/b.mp3", "/c.mp3", "/z.mp3"]
    );
}

#[test]
fn entries_reorder_among_their_siblings_and_the_order_survives_a_relaunch() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("dubplate.db");
    let mut store = SetStore::open(&path).unwrap();
    let gigs = create(&mut store, None, "Gigs", PlaylistKind::Folder);
    let warm_up = create(&mut store, Some(gigs), "Warm up", PlaylistKind::Playlist);
    let peak = create(&mut store, Some(gigs), "Peak", PlaylistKind::Playlist);
    let closing = create(&mut store, Some(gigs), "Closing", PlaylistKind::Playlist);
    store
        .edit_playlists(PlaylistEdit::Move {
            playlist: closing,
            parent: Some(gigs),
            position: 0,
        })
        .unwrap();
    store
        .edit_playlists(PlaylistEdit::Move {
            playlist: warm_up,
            parent: Some(gigs),
            position: 2,
        })
        .unwrap();
    store
        .edit_playlists(PlaylistEdit::Move {
            playlist: peak,
            parent: None,
            position: 0,
        })
        .unwrap();
    assert_eq!(
        names(&store.playlists().unwrap()),
        [
            ("Peak".to_owned(), 0),
            ("Gigs".to_owned(), 0),
            ("Closing".to_owned(), 1),
            ("Warm up".to_owned(), 1),
        ]
    );
    drop(store);

    let store = SetStore::open(&path).unwrap();
    assert_eq!(
        names(&store.playlists().unwrap()),
        [
            ("Peak".to_owned(), 0),
            ("Gigs".to_owned(), 0),
            ("Closing".to_owned(), 1),
            ("Warm up".to_owned(), 1),
        ]
    );
}

#[test]
fn tracks_move_and_leave_a_playlist() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = SetStore::open(&directory.path().join("dubplate.db")).unwrap();
    let peak = create(&mut store, None, "Peak", PlaylistKind::Playlist);
    store
        .edit_playlists(PlaylistEdit::AddTracks {
            playlist: peak,
            tracks: vec![file("/a.mp3"), file("/b.mp3"), file("/c.mp3")],
        })
        .unwrap();
    store
        .edit_playlists(PlaylistEdit::MoveTrack {
            playlist: peak,
            track: TrackIdentity::File(PathBuf::from("/c.mp3")),
            position: 0,
        })
        .unwrap();
    assert_eq!(
        track_names(&store.playlists().unwrap(), peak),
        ["/c.mp3", "/a.mp3", "/b.mp3"]
    );
    store
        .edit_playlists(PlaylistEdit::MoveTrack {
            playlist: peak,
            track: TrackIdentity::File(PathBuf::from("/c.mp3")),
            position: usize::MAX,
        })
        .unwrap();
    store
        .edit_playlists(PlaylistEdit::RemoveTracks {
            playlist: peak,
            tracks: vec![TrackIdentity::File(PathBuf::from("/a.mp3"))],
        })
        .unwrap();
    assert_eq!(
        track_names(&store.playlists().unwrap(), peak),
        ["/b.mp3", "/c.mp3"]
    );
}

#[test]
fn entries_move_between_folders_but_never_into_themselves() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = SetStore::open(&directory.path().join("dubplate.db")).unwrap();
    let gigs = create(&mut store, None, "Gigs", PlaylistKind::Folder);
    let club = create(&mut store, Some(gigs), "Club", PlaylistKind::Folder);
    let peak = create(&mut store, None, "Peak", PlaylistKind::Playlist);
    let closing = create(&mut store, None, "Closing", PlaylistKind::Playlist);

    store
        .edit_playlists(PlaylistEdit::Move {
            playlist: peak,
            parent: Some(club),
            position: 0,
        })
        .unwrap();
    store
        .edit_playlists(PlaylistEdit::Move {
            playlist: closing,
            parent: None,
            position: 0,
        })
        .unwrap();
    assert_eq!(
        names(&store.playlists().unwrap()),
        [
            ("Closing".to_owned(), 0),
            ("Gigs".to_owned(), 0),
            ("Club".to_owned(), 1),
            ("Peak".to_owned(), 2),
        ]
    );

    for target in [gigs, club] {
        assert!(matches!(
            store.edit_playlists(PlaylistEdit::Move {
                playlist: gigs,
                parent: Some(target),
                position: 0,
            }),
            Err(StoreError::PlaylistInsideItself(id)) if id == gigs
        ));
    }
    assert!(matches!(
        store.edit_playlists(PlaylistEdit::Move {
            playlist: closing,
            parent: Some(peak),
            position: 0,
        }),
        Err(StoreError::WrongPlaylistKind { .. })
    ));
    assert!(matches!(
        store.edit_playlists(PlaylistEdit::AddTracks {
            playlist: gigs,
            tracks: vec![file("/a.mp3")],
        }),
        Err(StoreError::WrongPlaylistKind { .. })
    ));
}

#[test]
fn deleting_a_folder_deletes_what_it_holds_and_renaming_keeps_the_place() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = SetStore::open(&directory.path().join("dubplate.db")).unwrap();
    let first = create(&mut store, None, "First", PlaylistKind::Playlist);
    let gigs = create(&mut store, None, "Gigs", PlaylistKind::Folder);
    let peak = create(&mut store, Some(gigs), "Peak", PlaylistKind::Playlist);
    let last = create(&mut store, None, "Last", PlaylistKind::Playlist);
    store
        .edit_playlists(PlaylistEdit::AddTracks {
            playlist: peak,
            tracks: vec![file("/a.mp3")],
        })
        .unwrap();
    store
        .edit_playlists(PlaylistEdit::Rename {
            playlist: first,
            name: "Opening".to_owned(),
        })
        .unwrap();
    store.edit_playlists(PlaylistEdit::Delete(gigs)).unwrap();
    let tree = store.playlists().unwrap();
    assert_eq!(
        names(&tree),
        [("Opening".to_owned(), 0), ("Last".to_owned(), 0)]
    );
    assert!(tree.get(peak).is_none());
    assert!(tree.get(last).is_some());
    assert!(matches!(
        store.edit_playlists(PlaylistEdit::Delete(peak)),
        Err(StoreError::PlaylistNotFound(id)) if id == peak
    ));
}

#[test]
fn a_folder_shows_every_track_of_its_playlists_once() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = SetStore::open(&directory.path().join("dubplate.db")).unwrap();
    let gigs = create(&mut store, None, "Gigs", PlaylistKind::Folder);
    for (name, tracks) in [
        ("Warm up", vec![file("/a.mp3"), file("/b.mp3")]),
        ("Peak", vec![file("/b.mp3"), file("/c.mp3")]),
    ] {
        store
            .edit_playlists(PlaylistEdit::Create {
                parent: Some(gigs),
                name: name.to_owned(),
                kind: PlaylistKind::Playlist,
                tracks,
            })
            .unwrap();
    }
    assert_eq!(
        track_names(&store.playlists().unwrap(), gigs),
        ["/a.mp3", "/b.mp3", "/c.mp3"]
    );
}

#[test]
fn a_moved_file_stays_in_its_playlists() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = SetStore::open(&directory.path().join("dubplate.db")).unwrap();
    let peak = create(&mut store, None, "Peak", PlaylistKind::Playlist);
    store
        .edit_playlists(PlaylistEdit::AddTracks {
            playlist: peak,
            tracks: vec![file("/Old/a.mp3"), file("/b.mp3")],
        })
        .unwrap();
    store
        .follow_moved_files(&HashMap::from([(
            PathBuf::from("/Old/a.mp3"),
            PathBuf::from("/New/a.mp3"),
        )]))
        .unwrap();
    let tree = store.playlists().unwrap();
    let moved = &tree.tracks(peak)[0];
    assert_eq!(
        moved.identity,
        TrackIdentity::File(PathBuf::from("/New/a.mp3"))
    );
    assert_eq!(moved.track_path, Some(PathBuf::from("/New/a.mp3")));
}
