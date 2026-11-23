//! Traktor's collection file is read into tracks with their grids, cues and ratings, and into
//! the playlist tree, without the file being changed.

#![allow(clippy::unwrap_used)]
use std::path::{Path, PathBuf};

use dubplate_library::{TraktorCue, TraktorError, TraktorPlaylistNode, read_traktor_collection};

const COLLECTION: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<NML VERSION="19">
  <COLLECTION ENTRIES="2">
    <ENTRY TITLE="Peak Time" ARTIST="Some DJ" CREATOR="" CREATED="2024-01-01 12:00:00" MODIFIED="2024-01-02 12:00:00">
      <LOCATION DIR="/:Users/:me/:Music/:" FILE="Peak Time.mp3" VOLUME="Macintosh HD" VOLUMEID="ABCD-1234" MOUNTPOINT="/"/>
      <ALBUM TITLE="Selected"/>
      <INFO GENRE="Techno" KEY="" PLAYTIME="312" RANKING="204" COMMENT="warm it up" BITRATE="320" TRACKNUMBER="1" COMPOSER="" LANGUAGE="" LABEL=""/>
      <TEMPO BPM="122.000061" BPM_QUALITY="100"/>
      <MUSICAL_KEY VALUE="17"/>
      <CUE_V2 NAME="Drop" DISPL_ORDER="0" TYPE="0" START="12345.678" LEN="0" REPEATS="-1" HOTCUE="0"/>
      <CUE_V2 NAME="" DISPL_ORDER="1" TYPE="0" START="5000" LEN="0" REPEATS="-1" HOTCUE="3"/>
      <CUE_V2 NAME="" DISPL_ORDER="2" TYPE="5" START="64000" LEN="8000" REPEATS="-1" HOTCUE="-1"/>
      <CUE_V2 NAME="Intro" DISPL_ORDER="3" TYPE="0" START="100" LEN="0" REPEATS="-1" HOTCUE="-1"/>
      <CUE_V2 NAME="" DISPL_ORDER="4" TYPE="4" START="4096" LEN="0" REPEATS="-1" HOTCUE="-1"/>
      <CUE_V2 NAME="" DISPL_ORDER="5" TYPE="2" START="290000" LEN="4000" REPEATS="-1" HOTCUE="-1"/>
    </ENTRY>
    <ENTRY TITLE="Loose" ARTIST="">
      <LOCATION DIR="/:Volumes/:USB/:Crate/:" FILE="Loose.wav" VOLUME="USB" VOLUMEID="EF01-5678" MOUNTPOINT="/Volumes/USB"/>
      <ALBUM TITLE=""/>
      <INFO GENRE="" PLAYTIME="not a number" RANKING="garbled"/>
      <TEMPO BPM=""/>
      <MUSICAL_KEY VALUE="oops"/>
      <CUE_V2 NAME="NoStart" DISPL_ORDER="0" TYPE="0" START="???" LEN="0" REPEATS="-1" HOTCUE="-1"/>
    </ENTRY>
  </COLLECTION>
  <PLAYLISTS>
    <NODE TYPE="FOLDER" NAME="$ROOT">
      <SUBNODES COUNT="3">
        <NODE TYPE="PLAYLIST" NAME="Peak">
          <PLAYLIST ENTRIES="2" TYPE="LIST" UUID="aaaa-1111" TIMESTAMP="0">
            <ENTRY><PRIMARYKEY TYPE="TRACK" KEY="Macintosh HD/:Users/:me/:Music/:Peak Time.mp3"/></ENTRY>
            <ENTRY><PRIMARYKEY TYPE="TRACK" KEY="USB/:Volumes/:USB/:Crate/:Loose.wav"/></ENTRY>
          </PLAYLIST>
        </NODE>
        <NODE TYPE="FOLDER" NAME="Gigs">
          <SUBNODES COUNT="2">
            <NODE TYPE="PLAYLIST" NAME="Saint Ouen">
              <PLAYLIST ENTRIES="1" TYPE="LIST" UUID="bbbb-2222" TIMESTAMP="0">
                <ENTRY><PRIMARYKEY TYPE="TRACK" KEY="Macintosh HD/:Users/:me/:Music/:Peak Time.mp3"/></ENTRY>
              </PLAYLIST>
            </NODE>
            <NODE TYPE="FOLDER" NAME="Deep">
              <SUBNODES COUNT="1">
                <NODE TYPE="PLAYLIST" NAME="Warmup">
                  <PLAYLIST ENTRIES="1" TYPE="LIST" UUID="cccc-3333" TIMESTAMP="0">
                    <ENTRY><PRIMARYKEY TYPE="TRACK" KEY="USB/:Volumes/:USB/:Crate/:Loose.wav"/></ENTRY>
                  </PLAYLIST>
                </NODE>
              </SUBNODES>
            </NODE>
          </SUBNODES>
        </NODE>
        <NODE TYPE="PLAYLIST" NAME="Empty">
          <PLAYLIST ENTRIES="0" TYPE="LIST" UUID="dddd-4444" TIMESTAMP="0"/>
        </NODE>
      </SUBNODES>
    </NODE>
  </PLAYLISTS>
</NML>
"#;

fn collection_file(directory: &Path, contents: &str) -> PathBuf {
    let path = directory.join("collection.nml");
    std::fs::write(&path, contents).unwrap();
    path
}

#[test]
fn tracks_carry_their_paths_metadata_grids_and_cues() {
    let directory = tempfile::tempdir().unwrap();
    let collection =
        read_traktor_collection(&collection_file(directory.path(), COLLECTION)).unwrap();
    assert_eq!(collection.tracks.len(), 2);

    let track = &collection.tracks[0];
    assert_eq!(track.path, PathBuf::from("/Users/me/Music/Peak Time.mp3"));
    assert_eq!(track.title.as_deref(), Some("Peak Time"));
    assert_eq!(track.artist.as_deref(), Some("Some DJ"));
    assert_eq!(track.album.as_deref(), Some("Selected"));
    assert_eq!(track.genre.as_deref(), Some("Techno"));
    assert_eq!(track.comment.as_deref(), Some("warm it up"));
    assert!((track.bpm.unwrap() - 122.000061).abs() < 1e-4);
    // 17 = F minor: pitch class 5, minor.
    assert_eq!(track.key, Some((5, true)));
    // 204 = 4 stars.
    assert_eq!(track.rating, 4);
    assert_eq!(track.duration_seconds, Some(312.0));
    assert_eq!(track.grid_anchor_ms, Some(4096.0));

    assert_eq!(
        track.hot_cues,
        vec![
            (
                0,
                TraktorCue {
                    start_ms: 12345.678,
                    name: "Drop".to_owned(),
                    loop_length_ms: None
                }
            ),
            (
                3,
                TraktorCue {
                    start_ms: 5000.0,
                    name: String::new(),
                    loop_length_ms: None
                }
            ),
        ],
        "the loop and the memory cue are not hot cues"
    );
    assert_eq!(
        track.memory_cues,
        vec![
            TraktorCue {
                start_ms: 64000.0,
                name: String::new(),
                loop_length_ms: Some(8000.0)
            },
            TraktorCue {
                start_ms: 100.0,
                name: "Intro".to_owned(),
                loop_length_ms: None
            },
            // The fade-out stays in the memory cues, without becoming a loop.
            TraktorCue {
                start_ms: 290000.0,
                name: String::new(),
                loop_length_ms: None
            },
        ],
        "the grid cue is the anchor, not a cue"
    );
}

#[test]
fn a_malformed_number_skips_only_its_field() {
    let directory = tempfile::tempdir().unwrap();
    let collection =
        read_traktor_collection(&collection_file(directory.path(), COLLECTION)).unwrap();
    let track = &collection.tracks[1];
    assert_eq!(track.path, PathBuf::from("/Volumes/USB/Crate/Loose.wav"));
    assert_eq!(track.title.as_deref(), Some("Loose"));
    assert_eq!(track.artist.as_deref(), Some(""));
    assert_eq!(track.genre.as_deref(), Some(""));
    assert_eq!(track.duration_seconds, None, "PLAYTIME is not a number");
    assert_eq!(track.rating, 0, "RANKING is not a number");
    assert_eq!(track.bpm, None, "BPM is empty");
    assert_eq!(track.key, None, "MUSICAL_KEY is not a number");
    assert!(track.hot_cues.is_empty());
    assert!(
        track.memory_cues.is_empty(),
        "a cue without a start is dropped"
    );
    assert_eq!(track.grid_anchor_ms, None);
}

#[test]
fn the_playlist_tree_maps_its_tracks_in_order() {
    let directory = tempfile::tempdir().unwrap();
    let collection =
        read_traktor_collection(&collection_file(directory.path(), COLLECTION)).unwrap();
    assert_eq!(
        collection.playlists,
        vec![
            TraktorPlaylistNode::Playlist {
                name: "Peak".to_owned(),
                track_paths: vec![
                    PathBuf::from("/Users/me/Music/Peak Time.mp3"),
                    PathBuf::from("/Volumes/USB/Crate/Loose.wav"),
                ],
            },
            TraktorPlaylistNode::Folder {
                name: "Gigs".to_owned(),
                children: vec![
                    TraktorPlaylistNode::Playlist {
                        name: "Saint Ouen".to_owned(),
                        track_paths: vec![PathBuf::from("/Users/me/Music/Peak Time.mp3")],
                    },
                    TraktorPlaylistNode::Folder {
                        name: "Deep".to_owned(),
                        children: vec![TraktorPlaylistNode::Playlist {
                            name: "Warmup".to_owned(),
                            track_paths: vec![PathBuf::from("/Volumes/USB/Crate/Loose.wav")],
                        }],
                    },
                ],
            },
            TraktorPlaylistNode::Playlist {
                name: "Empty".to_owned(),
                track_paths: Vec::new(),
            },
        ],
        "$ROOT is Traktor's own wrapper, not part of the tree"
    );
}

#[test]
fn unknown_attributes_and_elements_are_ignored() {
    let directory = tempfile::tempdir().unwrap();
    let path = collection_file(
        directory.path(),
        r#"<NML VERSION="20"><COLLECTION ENTRIES="1">
  <ENTRY TITLE="T" ARTIST="A" UNKNOWN_ATTR="x" ANOTHER="y">
    <LOCATION DIR="/:a/:b/" FILE="c.mp3" VOLUME="V" VOLUMEID="z"/>
    <SOMETHING NEW="yes"><NESTED DEEPER="1"/></SOMETHING>
    <INFO GENRE="House" RANKING="51"/>
  </ENTRY>
</COLLECTION></NML>"#,
    );
    let collection = read_traktor_collection(&path).unwrap();
    assert_eq!(collection.tracks[0].path, PathBuf::from("/a/b/c.mp3"));
    assert_eq!(collection.tracks[0].rating, 1);
    assert_eq!(collection.tracks[0].genre.as_deref(), Some("House"));
    assert!(collection.playlists.is_empty());
}

#[test]
fn the_ratings_and_keys_map_as_traktor_writes_them() {
    let entries: Vec<String> = (0..=5)
        .map(|stars| {
            format!(
                "<ENTRY TITLE=\"{stars} stars\"><LOCATION DIR=\"/:\" FILE=\"sample\"/><INFO RANKING=\"{value}\"/></ENTRY>",
                value = stars * 51
            )
        })
        .collect();
    let directory = tempfile::tempdir().unwrap();
    let path = collection_file(
        directory.path(),
        &format!(
            "<NML VERSION=\"19\"><COLLECTION ENTRIES=\"{}\">{}</COLLECTION></NML>",
            entries.len(),
            entries.concat()
        ),
    );
    let collection = read_traktor_collection(&path).unwrap();
    let ratings: Vec<u8> = collection.tracks.iter().map(|track| track.rating).collect();
    assert_eq!(ratings, [0, 1, 2, 3, 4, 5]);

    let keys: Vec<String> = (0..24)
        .map(|value| {
            format!("<ENTRY TITLE=\"key {value}\"><LOCATION DIR=\"/:\" FILE=\"sample\"/><MUSICAL_KEY VALUE=\"{value}\"/></ENTRY>")
        })
        .collect();
    let directory = tempfile::tempdir().unwrap();
    let path = collection_file(
        directory.path(),
        &format!(
            "<NML VERSION=\"19\"><COLLECTION ENTRIES=\"{}\">{}</COLLECTION></NML>",
            keys.len(),
            keys.concat()
        ),
    );
    let collection = read_traktor_collection(&path).unwrap();
    let parsed: Vec<(u8, bool)> = collection
        .tracks
        .iter()
        .map(|track| track.key.unwrap())
        .collect();
    assert_eq!(
        parsed,
        (0..24)
            .map(|value| (value % 12, value >= 12))
            .collect::<Vec<_>>()
    );
    assert_eq!(parsed[0], (0, false), "0 is C major");
    assert_eq!(parsed[9], (9, false), "9 is A major");
    assert_eq!(parsed[12], (0, true), "12 is C minor");
}

#[test]
fn a_missing_file_is_a_read_error() {
    let error = read_traktor_collection(Path::new("/nowhere/collection.nml"));
    assert!(matches!(error, Err(TraktorError::Read { .. })), "{error:?}");
}

#[test]
fn a_broken_file_is_a_malformed_error() {
    for contents in [
        "<NML VERSION=\"19\"><COLLECTION ENTRIES=\"1\"><ENTRY TITLE=\"open\"/>",
        "<NML VERSION=\"19\"><COLLECTION><ENTRY TITLE=\"a\"/></COLLECTION></WRONG>",
        "",
        "not xml at all",
        "<ROOT></ROOT>",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let error = read_traktor_collection(&collection_file(directory.path(), contents));
        assert!(
            matches!(
                error,
                Err(TraktorError::Malformed { .. } | TraktorError::WrongRoot { .. })
            ),
            "{contents:?} was not malformed: {error:?}"
        );
    }
}

#[test]
fn a_file_traktor_kept_open_is_never_changed_by_reading_it() {
    let directory = tempfile::tempdir().unwrap();
    let path = collection_file(directory.path(), COLLECTION);
    let before = std::fs::read(&path).unwrap();
    read_traktor_collection(&path).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn playlist_keys_distinguish_volumes_and_decode_xml_entities() {
    let directory = tempfile::tempdir().unwrap();
    let xml = r#"<NML><COLLECTION>
    <ENTRY><LOCATION DIR="/:music/:" FILE="A&amp;B.mp3" VOLUME="USB" MOUNTPOINT="/Volumes/USB"/></ENTRY>
    <ENTRY><LOCATION DIR="/:music/:" FILE="A&amp;B.mp3" VOLUME="Mac HD" MOUNTPOINT="/"/></ENTRY>
    </COLLECTION><PLAYLISTS><NODE TYPE="PLAYLIST" NAME="Set"><PLAYLIST>
    <ENTRY><PRIMARYKEY TYPE="TRACK" KEY="Mac HD/:music/:A&amp;B.mp3"/></ENTRY>
    <ENTRY><PRIMARYKEY TYPE="TRACK" KEY="USB/:music/:A&amp;B.mp3"/></ENTRY>
    <ENTRY><PRIMARYKEY TYPE="TRACK" KEY="unknown"/></ENTRY>
    </PLAYLIST></NODE></PLAYLISTS></NML>"#;
    let collection = read_traktor_collection(&collection_file(directory.path(), xml)).unwrap();
    assert_eq!(
        collection.playlists,
        vec![TraktorPlaylistNode::Playlist {
            name: "Set".into(),
            track_paths: vec![
                PathBuf::from("/music/A&B.mp3"),
                PathBuf::from("/Volumes/USB/music/A&B.mp3")
            ]
        }]
    );
}
