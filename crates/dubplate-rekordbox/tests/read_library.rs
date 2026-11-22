//! Reads an encrypted library built from the rekordbox 7.2.19 schema.

mod common;

use std::path::Path;

use common::{TEST_KEY, build_library, open_fixture};
use dubplate_rekordbox::{
    CueKind, DatabaseKey, Error, HotCuePad, Library, MemoryCueColor, PaletteColor, PlaylistKind,
};

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
    assert_eq!(saltwater.file_size, Some(81_600_000));
    assert_eq!(tracks[0].file_size, None, "a NULL FileSize is unknown");
    assert_eq!(tracks[0].bpm, Some(125.5));
}

#[test]
fn reads_the_auto_gain_rekordbox_measured() {
    let (_directory, library) = open_fixture();
    let tracks = library.tracks().unwrap();
    let gain_of = |title: &str| {
        tracks
            .iter()
            .find(|track| track.title.as_deref() == Some(title))
            .unwrap()
            .auto_gain_db
    };
    // GainHigh 0x3F8C and GainLow 0xCCCD are the bits of the single 1.1: +0.83 dB.
    let saltwater = gain_of("Saltwater Hymn").unwrap();
    assert!((saltwater - 0.827_853_7).abs() < 1e-4, "{saltwater}");
    assert_eq!(gain_of("Low Orbit"), None, "its only mixer row is deleted");
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
fn reads_hot_cue_colours_from_the_colour_table_and_memory_colours_from_color() {
    let (_directory, library) = open_fixture();
    let cues = library.cues().unwrap();
    let cue = |id: &str| cues.iter().find(|cue| cue.id == id).unwrap();

    assert_eq!(cue("c1").memory_color, Some(MemoryCueColor::Blue));
    assert_eq!(cue("c1").hot_cue_color, None);
    let drop = cue("c2");
    assert_eq!(drop.hot_cue_color.map(PaletteColor::index), Some(42));
    assert_eq!(
        drop.memory_color, None,
        "a hot cue's Color is not its colour"
    );
    assert_eq!(
        cue("c3").hot_cue_color,
        None,
        "no index means the pad's default"
    );
    assert_eq!(
        cue("c5").hot_cue_color,
        None,
        "index 0 means the pad's default"
    );
}

#[test]
fn maps_rekordbox_kinds_to_pads_skipping_four() {
    let (_directory, library) = open_fixture();
    let cues = library.cues().unwrap();
    let kind_of = |id: &str| cues.iter().find(|cue| cue.id == id).unwrap().kind;

    assert_eq!(kind_of("c5"), CueKind::Hot { pad: HotCuePad::D });
    assert_eq!(kind_of("c6"), CueKind::Hot { pad: HotCuePad::H });
    assert_eq!(kind_of("c7"), CueKind::Unknown { kind: 4 });
    assert_eq!(kind_of("c5").pad(), Some(HotCuePad::D));
    assert_eq!(CueKind::Memory.pad(), None);
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
