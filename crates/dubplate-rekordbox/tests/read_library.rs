//! Reads an encrypted library built from the rekordbox 7.2.19 schema.

use std::path::{Path, PathBuf};

use dubplate_rekordbox::{CueKind, DatabaseKey, Error, HotCuePad, Library, PlaylistKind};
use rusqlite::Connection;

const TEST_KEY: &str = "dubplate-test-key";
const SCHEMA: &str = include_str!("../schema/rekordbox-7.2.19.sql");
const STAMP: &str = "2026-11-21 20:00:00.000 +00:00";

fn build_library(directory: &Path) -> PathBuf {
    let path = directory.join("master.db");
    let connection = Connection::open(&path).expect("create fixture");
    connection
        .pragma_update(None, "key", TEST_KEY)
        .expect("set key");
    connection
        .execute_batch(SCHEMA)
        .expect("create rekordbox 7.2.19 tables");
    let rows = format!(
        "INSERT INTO djmdProperty (DBID, DBVersion, created_at, updated_at)
             VALUES ('1', '6000', '{STAMP}', '{STAMP}');
         INSERT INTO djmdArtist (ID, Name, created_at, updated_at) VALUES
             ('a1', 'Mara Okonjo', '{STAMP}', '{STAMP}'),
             ('a2', 'Teodor Ruse', '{STAMP}', '{STAMP}');
         INSERT INTO djmdKey (ID, ScaleName, created_at, updated_at) VALUES
             ('k1', '8A', '{STAMP}', '{STAMP}'),
             ('k2', '9A', '{STAMP}', '{STAMP}');
         INSERT INTO djmdContent
             (ID, Title, ArtistID, KeyID, BPM, Length, FolderPath, Rating, rb_local_deleted,
              created_at, updated_at) VALUES
             ('t1', 'Saltwater Hymn', 'a1', 'k1', 12400, 408, '/Music/saltwater.flac', 4, 0,
              '{STAMP}', '{STAMP}'),
             ('t2', 'Low Orbit', 'a2', 'k2', 12550, 424, '/Music/low-orbit.mp3', 0, 0,
              '{STAMP}', '{STAMP}'),
             ('t3', 'Deleted Track', 'a1', 'k1', 12000, 300, '/Music/gone.wav', 0, 1,
              '{STAMP}', '{STAMP}');
         INSERT INTO djmdCue
             (ID, ContentID, Kind, InMsec, OutMsec, Color, Comment, rb_local_deleted,
              created_at, updated_at) VALUES
             ('c1', 't1', 0, 1000, -1, NULL, NULL, 0, '{STAMP}', '{STAMP}'),
             ('c2', 't1', 1, 45000, -1, 255, 'Drop', 0, '{STAMP}', '{STAMP}'),
             ('c3', 't2', 2, 30000, 60000, NULL, NULL, 0, '{STAMP}', '{STAMP}'),
             ('c4', 't1', 3, 90000, -1, NULL, NULL, 1, '{STAMP}', '{STAMP}'),
             ('c5', 't2', 5, 40000, -1, -1, NULL, 0, '{STAMP}', '{STAMP}'),
             ('c6', 't2', 9, 50000, -1, -1, NULL, 0, '{STAMP}', '{STAMP}'),
             ('c7', 't2', 4, 70000, -1, -1, NULL, 0, '{STAMP}', '{STAMP}');
         INSERT INTO djmdPlaylist (ID, ParentID, Name, Attribute, Seq, created_at, updated_at)
             VALUES
             ('f1', 'root', 'Sets', 1, 1, '{STAMP}', '{STAMP}'),
             ('p1', 'f1', 'Warm-up', 0, 1, '{STAMP}', '{STAMP}'),
             ('s1', 'root', 'Peak time', 4, 2, '{STAMP}', '{STAMP}');
         INSERT INTO djmdSongPlaylist (ID, PlaylistID, ContentID, TrackNo, created_at, updated_at)
             VALUES
             ('e1', 'p1', 't2', 1, '{STAMP}', '{STAMP}'),
             ('e2', 'p1', 't1', 2, '{STAMP}', '{STAMP}'),
             ('e3', 'p1', 't3', 3, '{STAMP}', '{STAMP}');"
    );
    connection
        .execute_batch(&rows)
        .expect("insert fixture rows");
    path
}

fn open_fixture() -> (tempfile::TempDir, Library) {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = build_library(directory.path());
    let library = Library::open_copy(&path, &DatabaseKey::new(TEST_KEY)).expect("open fixture");
    (directory, library)
}

#[test]
fn reads_tracks_with_their_names_key_and_tempo() {
    let (_directory, library) = open_fixture();
    let tracks = library.tracks().unwrap();

    let titles: Vec<_> = tracks
        .iter()
        .map(|track| track.title.as_deref().unwrap())
        .collect();
    assert_eq!(titles, ["Low Orbit", "Saltwater Hymn"]);

    let saltwater = &tracks[1];
    assert_eq!(saltwater.artist.as_deref(), Some("Mara Okonjo"));
    assert_eq!(saltwater.key.as_deref(), Some("8A"));
    assert_eq!(saltwater.bpm, Some(124.0));
    assert_eq!(saltwater.length_seconds, Some(408));
    assert_eq!(saltwater.rating, Some(4));
    assert_eq!(
        saltwater.file_path.as_deref(),
        Some(Path::new("/Music/saltwater.flac"))
    );
    assert_eq!(tracks[0].bpm, Some(125.5));
}

#[test]
fn reads_memory_cues_hot_cues_and_loops_but_not_deleted_cues() {
    let (_directory, library) = open_fixture();
    let cues = library.cues().unwrap();

    let ids: Vec<_> = cues.iter().map(|cue| cue.id.as_str()).collect();
    assert_eq!(ids, ["c1", "c2", "c3", "c5", "c6", "c7"]);
    assert_eq!(cues[0].kind, CueKind::Memory);
    assert_eq!(cues[0].loop_end_ms, None);
    assert_eq!(cues[1].kind, CueKind::Hot { pad: HotCuePad::A });
    assert_eq!(cues[1].start_ms, 45_000);
    assert_eq!(cues[1].comment.as_deref(), Some("Drop"));
    assert_eq!(cues[2].kind, CueKind::Hot { pad: HotCuePad::B });
    assert_eq!(cues[2].loop_end_ms, Some(60_000));
}

#[test]
fn maps_rekordbox_kinds_to_pads_skipping_four() {
    let (_directory, library) = open_fixture();
    let cues = library.cues().unwrap();
    let kind_of = |id: &str| cues.iter().find(|cue| cue.id == id).unwrap().kind;

    assert_eq!(kind_of("c5"), CueKind::Hot { pad: HotCuePad::D });
    assert_eq!(kind_of("c6"), CueKind::Hot { pad: HotCuePad::H });
    assert_eq!(kind_of("c7"), CueKind::Unknown { kind: 4 });
}

#[test]
fn every_pad_round_trips_through_its_rekordbox_kind() {
    let kinds: Vec<i64> = ['A', 'B', 'C', 'D', 'E', 'F', 'G', 'H']
        .iter()
        .map(|letter| {
            let pad = (1..=9)
                .filter_map(HotCuePad::from_rekordbox_kind)
                .find(|pad| pad.letter() == *letter)
                .unwrap();
            assert_eq!(
                HotCuePad::from_rekordbox_kind(pad.rekordbox_kind()),
                Some(pad)
            );
            pad.rekordbox_kind()
        })
        .collect();

    assert_eq!(kinds, [1, 2, 3, 5, 6, 7, 8, 9]);
    assert_eq!(HotCuePad::from_rekordbox_kind(4), None);
}

#[test]
fn reads_the_playlist_tree_in_order_without_deleted_tracks() {
    let (_directory, library) = open_fixture();
    let playlists = library.playlists().unwrap();

    let folder = playlists
        .iter()
        .find(|playlist| playlist.id == "f1")
        .unwrap();
    assert_eq!(folder.kind, PlaylistKind::Folder);
    assert_eq!(folder.parent_id, None);
    assert!(folder.track_ids.is_empty());

    let warm_up = playlists
        .iter()
        .find(|playlist| playlist.id == "p1")
        .unwrap();
    assert_eq!(warm_up.parent_id.as_deref(), Some("f1"));
    assert_eq!(warm_up.track_ids, ["t2", "t1"]);

    let smart = playlists
        .iter()
        .find(|playlist| playlist.id == "s1")
        .unwrap();
    assert_eq!(smart.kind, PlaylistKind::Smart);
}

#[test]
fn summarises_the_library() {
    let (_directory, library) = open_fixture();
    let summary = library.summary().unwrap();

    assert_eq!(summary.database_version.as_deref(), Some("6000"));
    assert_eq!(summary.tracks, 2);
    assert_eq!(summary.playlists, 3);
    assert_eq!(summary.cues, 6);
}

#[test]
fn refuses_a_wrong_key() {
    let directory = tempfile::tempdir().unwrap();
    let path = build_library(directory.path());

    let result = Library::open_copy(&path, &DatabaseKey::new("not-the-key"));

    assert!(matches!(result, Err(Error::WrongKey)));
}

#[test]
fn reports_a_missing_library() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("master.db");

    let result = Library::open_copy(&path, &DatabaseKey::new(TEST_KEY));

    assert!(matches!(result, Err(Error::NotFound(missing)) if missing == path));
}

#[test]
fn leaves_the_original_file_untouched() {
    let directory = tempfile::tempdir().unwrap();
    let path = build_library(directory.path());
    let before = std::fs::read(&path).unwrap();

    let library = Library::open_copy(&path, &DatabaseKey::new(TEST_KEY)).unwrap();
    library.tracks().unwrap();
    library.playlists().unwrap();
    drop(library);

    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn never_prints_the_key() {
    let key = DatabaseKey::new("secret-value");

    assert_eq!(format!("{key:?}"), "DatabaseKey(..)");
}
