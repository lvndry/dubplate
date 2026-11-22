//! A set exported as rekordbox XML parses back with a real XML parser, keeps its text and paths
//! through escaping, and carries the grid corrections, mix cues and rekordbox cues in seconds.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use dubplate_library::{FolderScan, LibraryTrack};
use dubplate_prep::{
    ExportError, ExportLibrary, ExportTrack, RekordboxCollection, Set, SetEntry, SetExport, SetId,
    export_file_name, file_track, rekordbox_xml, tempo_markers,
};
use dubplate_rekordbox::analysis::{Beat, BeatGrid};
use dubplate_rekordbox::xml::TempoMarker;
use dubplate_rekordbox::{Cue, CueKind, HotCuePad, Track};
use quick_xml::XmlVersion;
use quick_xml::events::Event;
use quick_xml::reader::Reader;

/// One element of the parsed file: its name and its attributes, unescaped.
#[derive(Debug, Clone)]
struct Element {
    name: String,
    attributes: HashMap<String, String>,
    depth: usize,
}

impl Element {
    fn attribute(&self, name: &str) -> &str {
        self.attributes
            .get(name)
            .map(String::as_str)
            .unwrap_or_else(|| panic!("{} has no {name}: {:?}", self.name, self.attributes))
    }
}

/// Parses the XML back with a real parser, failing on anything malformed.
fn parse(xml: &str) -> Vec<Element> {
    let mut reader = Reader::from_str(xml);
    let mut elements = Vec::new();
    let mut depth = 0;
    loop {
        match reader.read_event().expect("well-formed XML") {
            Event::Start(start) => {
                elements.push(element(&start, depth));
                depth += 1;
            }
            Event::Empty(start) => elements.push(element(&start, depth)),
            Event::End(_) => depth -= 1,
            Event::Eof => break,
            _ => {}
        }
    }
    assert_eq!(depth, 0, "every element is closed");
    elements
}

fn element(start: &quick_xml::events::BytesStart<'_>, depth: usize) -> Element {
    Element {
        name: AsRef::<str>::as_ref(&start.name()).to_owned(),
        attributes: start
            .attributes()
            .map(|attribute| {
                let attribute = attribute.expect("a well-formed attribute");
                (
                    AsRef::<str>::as_ref(&attribute.key).to_owned(),
                    attribute
                        .normalized_value(XmlVersion::Explicit1_0)
                        .expect("an attribute value that unescapes")
                        .into_owned(),
                )
            })
            .collect(),
        depth,
    }
}

fn named<'a>(elements: &'a [Element], name: &str) -> Vec<&'a Element> {
    elements
        .iter()
        .filter(|element| element.name == name)
        .collect()
}

/// The path a `file://localhost` location points at.
fn decoded_location(location: &str) -> PathBuf {
    let encoded = location
        .strip_prefix("file://localhost")
        .expect("rekordbox's location prefix");
    let mut bytes = Vec::new();
    let mut rest = encoded.as_bytes();
    while let Some((&byte, tail)) = rest.split_first() {
        if byte == b'%' {
            let hex = std::str::from_utf8(&tail[..2]).expect("two hex digits after %");
            bytes.push(u8::from_str_radix(hex, 16).expect("two hex digits after %"));
            rest = &tail[2..];
        } else {
            assert!(
                byte.is_ascii() && byte != b' ',
                "only ASCII without spaces is left unescaped"
            );
            bytes.push(byte);
            rest = tail;
        }
    }
    PathBuf::from(String::from_utf8(bytes).expect("a UTF-8 path"))
}

fn beat(time_ms: u32, beat_in_bar: u16, tempo_hundredths: u16) -> Beat {
    Beat {
        beat_in_bar,
        tempo_hundredths,
        time_ms,
    }
}

/// A steady 120 BPM grid from 41 ms, its first beat the downbeat.
fn steady_grid(beats: u32) -> BeatGrid {
    BeatGrid {
        beats: (0..beats)
            .map(|index| beat(41 + index * 500, (index % 4 + 1) as u16, 12_000))
            .collect(),
    }
}

fn rekordbox_track(id: &str, path: &str) -> Track {
    Track {
        id: id.to_owned(),
        title: Some(format!("Title {id}")),
        artist: Some("Artist".to_owned()),
        bpm: Some(120.0),
        key: Some("8A".to_owned()),
        length_seconds: Some(300),
        ..file_track(Path::new(path))
    }
}

fn hot_cue(track_id: &str, pad: HotCuePad, start_ms: u32, name: &str) -> Cue {
    Cue {
        id: format!("{track_id}-{}", pad.letter()),
        track_id: track_id.to_owned(),
        kind: CueKind::Hot { pad },
        start_ms,
        loop_end_ms: None,
        color: None,
        comment: Some(name.to_owned()),
    }
}

fn memory_cue(track_id: &str, start_ms: u32) -> Cue {
    Cue {
        id: format!("{track_id}-memory-{start_ms}"),
        track_id: track_id.to_owned(),
        kind: CueKind::Memory,
        start_ms,
        loop_end_ms: None,
        color: None,
        comment: None,
    }
}

fn export_track(track: Track, entry: SetEntry) -> ExportTrack {
    ExportTrack {
        entry: SetEntry {
            track_path: track.file_path.clone().expect("a track with a file"),
            rekordbox_id: (!track.id.is_empty()).then(|| track.id.clone()),
            ..entry
        },
        track,
        in_rekordbox: true,
        beat_grid: None,
        cues: Vec::new(),
    }
}

#[test]
fn text_and_paths_survive_escaping() {
    let path = "/Users/dj/Music/Rønhöff & Miči/Café <Live> \"Dub\" 50% #1 B’utiza.m4a";
    let track = Track {
        title: Some("Are We Safe? <Extended> & \"Dub\" 'Mix'".to_owned()),
        artist: Some("Lemon & Herb, Miči".to_owned()),
        album: Some("Tab\tand\nline".to_owned()),
        comment: Some("https://example.com/?a=1&b=2".to_owned()),
        ..rekordbox_track("1", path)
    };
    let xml = rekordbox_xml(
        "Sunset & <Sunrise>",
        &[export_track(track, SetEntry::default())],
    );
    let elements = parse(&xml);
    let track = named(&elements, "TRACK")[0];
    assert_eq!(
        track.attribute("Name"),
        "Are We Safe? <Extended> & \"Dub\" 'Mix'"
    );
    assert_eq!(track.attribute("Artist"), "Lemon & Herb, Miči");
    assert_eq!(track.attribute("Album"), "Tab\tand\nline");
    assert_eq!(track.attribute("Comments"), "https://example.com/?a=1&b=2");
    assert_eq!(
        decoded_location(track.attribute("Location")),
        Path::new(path)
    );
    assert!(
        track
            .attribute("Location")
            .starts_with("file://localhost/Users/dj/Music/R%c3%b8nh%c3%b6ff%20%26%20Mi%c4%8di/"),
        "{}",
        track.attribute("Location")
    );
    let playlist = named(&elements, "NODE")[1];
    assert_eq!(playlist.attribute("Name"), "Sunset & <Sunrise>");
}

#[test]
fn the_file_has_rekordbox_structure() {
    let first = export_track(rekordbox_track("10", "/Music/One.mp3"), SetEntry::default());
    let second = export_track(rekordbox_track("20", "/Music/Two.mp3"), SetEntry::default());
    let backup = export_track(
        rekordbox_track("30", "/Music/Three.mp3"),
        SetEntry {
            is_backup: true,
            ..SetEntry::default()
        },
    );
    let xml = rekordbox_xml("Friday", &[second.clone(), first, second, backup]);
    assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"));
    let elements = parse(&xml);
    let root = &elements[0];
    assert_eq!(
        (root.name.as_str(), root.attribute("Version")),
        ("DJ_PLAYLISTS", "1.0.0")
    );
    assert_eq!(elements[1].name, "PRODUCT");
    let collection = named(&elements, "COLLECTION")[0];
    assert_eq!(
        collection.attribute("Entries"),
        "3",
        "a track twice in the set is listed once"
    );
    let collection_tracks: Vec<&Element> = named(&elements, "TRACK")
        .into_iter()
        .filter(|track| track.depth == 2)
        .collect();
    let ids: Vec<&str> = collection_tracks
        .iter()
        .map(|track| track.attribute("TrackID"))
        .collect();
    assert_eq!(ids, ["1", "2", "3"]);
    let nodes = named(&elements, "NODE");
    assert_eq!(
        (
            nodes[0].attribute("Type"),
            nodes[0].attribute("Name"),
            nodes[0].attribute("Count")
        ),
        ("0", "ROOT", "2")
    );
    assert_eq!(
        (
            nodes[1].attribute("Name"),
            nodes[1].attribute("Type"),
            nodes[1].attribute("KeyType"),
            nodes[1].attribute("Entries")
        ),
        ("Friday", "1", "0", "3")
    );
    assert_eq!(nodes[2].attribute("Name"), "Friday backups");
    let keys: Vec<&str> = named(&elements, "TRACK")
        .into_iter()
        .filter(|track| track.depth == 4)
        .map(|track| track.attribute("Key"))
        .collect();
    assert_eq!(
        keys,
        ["1", "2", "1", "3"],
        "the playlists keep the set's order"
    );
    let first_track = collection_tracks[0];
    assert_eq!(first_track.attribute("AverageBpm"), "120.00");
    assert_eq!(first_track.attribute("Tonality"), "8A");
    assert_eq!(first_track.attribute("TotalTime"), "300");
}

#[test]
fn the_grid_carries_the_djs_corrections_in_seconds() {
    let mut track = export_track(
        rekordbox_track("1", "/Music/One.mp3"),
        SetEntry {
            grid_offset_ms: 20,
            downbeat_shift: 1,
            ..SetEntry::default()
        },
    );
    track.beat_grid = Some(steady_grid(64));
    let elements = parse(&rekordbox_xml("Set", &[track.clone()]));
    let tempos = named(&elements, "TEMPO");
    assert_eq!(tempos.len(), 1, "a steady grid is one marker");
    assert_eq!(tempos[0].attribute("Inizio"), "0.061");
    assert_eq!(tempos[0].attribute("Bpm"), "120.00");
    assert_eq!(tempos[0].attribute("Metro"), "4/4");
    assert_eq!(
        tempos[0].attribute("Battito"),
        "4",
        "the downbeat moved one beat later, so the first beat is the last of its bar"
    );

    track.entry.grid_offset_ms = -50;
    track.entry.downbeat_shift = 0;
    let elements = parse(&rekordbox_xml("Set", &[track]));
    let tempo = named(&elements, "TEMPO")[0];
    assert_eq!(
        (tempo.attribute("Inizio"), tempo.attribute("Battito")),
        ("0.491", "2"),
        "a beat moved before the start is left out"
    );
}

#[test]
fn mix_cues_are_named_memory_cues_beside_rekordbox_cues() {
    let mut track = export_track(
        rekordbox_track("1", "/Music/One.mp3"),
        SetEntry {
            mix_in_ms: Some(32_123),
            mix_out_ms: Some(291_005),
            ..SetEntry::default()
        },
    );
    let mut saved_loop = hot_cue("1", HotCuePad::B, 64_000, "Loop");
    saved_loop.loop_end_ms = Some(72_000);
    track.cues = vec![
        hot_cue("1", HotCuePad::C, 1_259, "1.1Bars"),
        saved_loop,
        memory_cue("1", 32_123),
        memory_cue("1", 100_500),
    ];
    let elements = parse(&rekordbox_xml("Set", &[track]));
    let marks: Vec<(&str, &str, &str, &str, Option<&str>)> = named(&elements, "POSITION_MARK")
        .into_iter()
        .map(|mark| {
            (
                mark.attribute("Name"),
                mark.attribute("Type"),
                mark.attribute("Start"),
                mark.attribute("Num"),
                mark.attributes.get("End").map(String::as_str),
            )
        })
        .collect();
    assert_eq!(
        marks,
        [
            ("Mix in", "0", "32.123", "-1", None),
            ("", "0", "100.500", "-1", None),
            ("Mix out", "0", "291.005", "-1", None),
            ("Loop", "4", "64.000", "1", Some("72.000")),
            ("1.1Bars", "0", "1.259", "2", None),
        ],
        "rekordbox's memory cue at the mix-in gives way to it, and the hot cues keep their pads"
    );
}

#[test]
fn tempo_markers_follow_tempo_changes_bar_breaks_and_drift() {
    assert_eq!(
        tempo_markers(&steady_grid(400)),
        [TempoMarker {
            start_ms: 41,
            tempo_hundredths: 12_000,
            beat_in_bar: 1
        }]
    );
    let mut changing = steady_grid(8);
    changing
        .beats
        .extend((0..8).map(|index| beat(4_041 + index * 480, (index % 4 + 1) as u16, 12_500)));
    let starts: Vec<(u32, u16)> = tempo_markers(&changing)
        .iter()
        .map(|marker| (marker.start_ms, marker.tempo_hundredths))
        .collect();
    assert_eq!(starts, [(41, 12_000), (4_041, 12_500)]);

    let mut broken_bar = steady_grid(8);
    broken_bar.beats[5].beat_in_bar = 1;
    broken_bar.beats[6].beat_in_bar = 2;
    broken_bar.beats[7].beat_in_bar = 3;
    let markers = tempo_markers(&broken_bar);
    assert_eq!(markers.len(), 2);
    assert_eq!((markers[1].start_ms, markers[1].beat_in_bar), (2_541, 1));

    let mut drifting = steady_grid(8);
    for beat in &mut drifting.beats[4..] {
        beat.time_ms += 3;
    }
    assert_eq!(tempo_markers(&drifting).len(), 2);
    let mut rounded = steady_grid(8);
    rounded.beats[3].time_ms += 1;
    assert_eq!(
        tempo_markers(&rounded).len(),
        1,
        "a beat rounded to the next millisecond stays on the marker"
    );
}

#[test]
fn the_library_finds_each_set_track_where_the_dj_keeps_it() {
    let directory = tempfile::tempdir().unwrap();
    let relinked_to = directory.path().join("New Place").join("Moved.m4a");
    let folder_file = directory.path().join("Crate").join("Folder Track.mp3");
    std::fs::create_dir_all(relinked_to.parent().unwrap()).unwrap();
    std::fs::create_dir_all(folder_file.parent().unwrap()).unwrap();
    std::fs::write(&relinked_to, b"audio").unwrap();
    std::fs::write(&folder_file, b"audio").unwrap();
    let rekordbox_tracks = vec![rekordbox_track("7", "/Old Place/Moved.m4a")];
    let cues = vec![hot_cue("7", HotCuePad::A, 1_000, "Drop")];
    let scans = vec![FolderScan {
        folder: directory.path().join("Crate"),
        tracks: vec![LibraryTrack {
            path: folder_file.clone(),
            title: Some("Folder Title".to_owned()),
            artist: Some("Folder Artist".to_owned()),
            album: None,
            genre: None,
            comment: None,
            bpm: Some(124.0),
            key: Some("Am".to_owned()),
            duration_seconds: Some(301.6),
            size: 5,
            modified_ms: 0,
        }],
        problems: Vec::new(),
        scanned_at_ms: 0,
    }];
    let relinks = HashMap::from([(PathBuf::from("/Old Place/Moved.m4a"), relinked_to.clone())]);
    let library = ExportLibrary::new(
        Some(RekordboxCollection {
            tracks: &rekordbox_tracks,
            cues: &cues,
            share_directory: directory.path(),
        }),
        &scans,
        &relinks,
    );
    let gone = directory.path().join("Gone.wav");
    let set = Set {
        id: SetId(1),
        name: "Saturday".to_owned(),
        target_minutes: 60,
        created_at: 0,
        updated_at: 0,
        entries: vec![
            SetEntry {
                track_path: PathBuf::from("/Old Place/Moved.m4a"),
                rekordbox_id: Some("7".to_owned()),
                ..SetEntry::default()
            },
            SetEntry {
                track_path: folder_file.clone(),
                ..SetEntry::default()
            },
            SetEntry {
                track_path: gone.clone(),
                ..SetEntry::default()
            },
        ],
        seams: Vec::new(),
    };
    let export = library.export(&set);
    assert_eq!(export.track_count, 3);
    assert_eq!(export.new_to_rekordbox, 2);
    assert_eq!(export.problems.len(), 1, "{:?}", export.problems);
    assert!(
        export.problems[0].contains("Gone.wav"),
        "{:?}",
        export.problems
    );

    let elements = parse(&export.xml);
    let tracks = named(&elements, "TRACK");
    assert_eq!(
        decoded_location(tracks[0].attribute("Location")),
        relinked_to,
        "a relinked file exports at its new place"
    );
    assert_eq!(tracks[0].attribute("Name"), "Title 7");
    assert_eq!(
        named(&elements, "POSITION_MARK")[0].attribute("Name"),
        "Drop"
    );
    assert_eq!(
        decoded_location(tracks[1].attribute("Location")),
        folder_file
    );
    assert_eq!(tracks[1].attribute("Name"), "Folder Title");
    assert_eq!(tracks[1].attribute("Tonality"), "Am");
    assert_eq!(tracks[1].attribute("TotalTime"), "302");
    assert_eq!(decoded_location(tracks[2].attribute("Location")), gone);
    assert!(!tracks[2].attributes.contains_key("Name"));
}

#[test]
fn an_export_is_written_anywhere_but_inside_rekordbox() {
    let directory = tempfile::tempdir().unwrap();
    let export = SetExport {
        xml: rekordbox_xml("Set", &[]),
        track_count: 0,
        new_to_rekordbox: 0,
        problems: Vec::new(),
    };
    let path = directory.path().join("Dubplate Exports").join("Set.xml");
    export.write_to(&path).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), export.xml);
    assert!(!path.with_extension("xml.partial").exists());

    let inside = directory.path().join("Library/Pioneer/rekordbox/Set.xml");
    assert!(matches!(
        export.write_to(&inside),
        Err(ExportError::InsideRekordbox(_))
    ));
    assert!(
        !inside.parent().unwrap().exists(),
        "nothing is created there"
    );
}

#[test]
fn a_set_name_becomes_a_file_name() {
    assert_eq!(
        export_file_name("Friday: Warm-up / Peak"),
        "Friday- Warm-up - Peak.xml"
    );
    assert_eq!(export_file_name("Café & Co"), "Café & Co.xml");
    assert_eq!(export_file_name(" .hidden"), "hidden.xml");
    assert_eq!(export_file_name("  "), "Set.xml");
}

#[test]
fn a_set_track_follows_its_file_by_rekordbox_id_and_the_relinks() {
    let rekordbox_tracks = vec![rekordbox_track("7", "/Old Place/Moved.m4a")];
    let relinks = HashMap::from([(
        PathBuf::from("/Old Place/Moved.m4a"),
        PathBuf::from("/New Place/Moved.m4a"),
    )]);
    let library = ExportLibrary::new(
        Some(RekordboxCollection {
            tracks: &rekordbox_tracks,
            cues: &[],
            share_directory: Path::new("/PIONEER"),
        }),
        &[],
        &relinks,
    );
    let set = Set {
        id: SetId(1),
        name: "Saturday".to_owned(),
        target_minutes: 60,
        created_at: 0,
        updated_at: 0,
        entries: vec![
            SetEntry {
                track_path: PathBuf::from("/Older Place/Moved.m4a"),
                rekordbox_id: Some("7".to_owned()),
                ..SetEntry::default()
            },
            SetEntry {
                track_path: PathBuf::from("/Crate/Stays.mp3"),
                ..SetEntry::default()
            },
        ],
        seams: Vec::new(),
    };
    assert_eq!(
        library.current_path(&set.entries[0]),
        PathBuf::from("/New Place/Moved.m4a"),
        "the rekordbox id finds the track wherever the set last saw it"
    );
    assert_eq!(
        library.moved_files(&set),
        HashMap::from([(
            PathBuf::from("/Older Place/Moved.m4a"),
            PathBuf::from("/New Place/Moved.m4a"),
        )]),
        "a file that has not moved is left alone"
    );
}
