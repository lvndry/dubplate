//! Writes a stick with Dubplate's own Device Library and analysis writers, then reads what a
//! player wrote back onto it: the histories the player recorded and the cues the player added
//! or moved on the CDJ. No audio in the repo; the tracks are rows and analysis files only.

use std::fs;
use std::path::Path;

use dubplate_rekordbox::analysis::{AnalysisCue, AnalysisExport, AnalysisFileKind, AnalysisFiles};
use dubplate_rekordbox::device_library::{
    DeviceLibrary, DeviceTrack, FileType, LibraryId, TrackId, analysis_path, export_ext_pdb,
};
use dubplate_rekordbox::stick_import::{
    StickImportError, stick_cue_changes, stick_histories, stick_track_cues,
};
use dubplate_rekordbox::{CueChange, HotCue, HotCuePad, TrackCues};

fn id(value: u32) -> TrackId {
    TrackId::new(value).expect("the test id is not 0")
}

fn track(number: u32, title: &str, file_path: &str) -> DeviceTrack {
    DeviceTrack {
        id: id(number),
        title: title.to_owned(),
        artist: None,
        composer: None,
        original_artist: None,
        remixer: None,
        album: None,
        genre: None,
        label: None,
        key: None,
        artwork: None,
        color: None,
        rating: 0,
        tempo_hundredths: 12_200,
        duration_seconds: 412,
        sample_rate: 44_100,
        sample_depth: 16,
        bitrate: 320,
        file_size: 9_876_543,
        file_type: FileType::Mp3,
        track_number: 0,
        disc_number: 0,
        year: 0,
        play_count: 0,
        checksum: number.wrapping_mul(0x9e37_79b9),
        file_path: file_path.to_owned(),
        analyze_path: analysis_path(file_path).to_string(),
        analyze_date: "2026-11-22".to_owned(),
        date_added: "2026-11-01".to_owned(),
        release_date: String::new(),
        comment: String::new(),
        mix_name: String::new(),
        isrc: String::new(),
        lyricist: String::new(),
        message: String::new(),
        kuvo_public: false,
        autoload_hot_cues: true,
    }
}

fn hot_cue(number: u32, time_ms: u32) -> AnalysisCue {
    AnalysisCue {
        hot_cue: Some(number),
        pad: Some(char::from_u32(u32::from('A') + number - 1).expect("pads A to H are letters")),
        time_ms,
        loop_end_ms: None,
        loop_length: None,
        name: None,
        memory_color: None,
        hot_cue_color: None,
    }
}

fn memory_cue(time_ms: u32) -> AnalysisCue {
    AnalysisCue {
        hot_cue: None,
        pad: None,
        time_ms,
        loop_end_ms: None,
        loop_length: None,
        name: None,
        memory_color: Some(2),
        hot_cue_color: None,
    }
}

fn named_memory_cue(time_ms: u32, name: &str) -> AnalysisCue {
    AnalysisCue {
        name: Some(name.to_owned()),
        ..memory_cue(time_ms)
    }
}

fn write_stick(directory: &Path, library: &DeviceLibrary) {
    let rekordbox = directory.join("PIONEER").join("rekordbox");
    fs::create_dir_all(&rekordbox).expect("the stick folders are written");
    fs::write(
        rekordbox.join("export.pdb"),
        library.to_export_pdb().expect("the library writes"),
    )
    .expect("export.pdb is written");
    fs::write(
        rekordbox.join("exportExt.pdb"),
        export_ext_pdb(library.library_id).expect("exportExt.pdb writes"),
    )
    .expect("exportExt.pdb is written");
}

fn write_analysis(directory: &Path, file_path: &str, cues: &[AnalysisCue]) {
    let files = AnalysisFiles::write(&AnalysisExport {
        audio_path: file_path,
        beat_grid: None,
        cues,
        waveforms: &Default::default(),
        copied: &[],
    })
    .expect("the analysis files write");
    for kind in [
        AnalysisFileKind::Dat,
        AnalysisFileKind::Ext,
        AnalysisFileKind::TwoEx,
    ] {
        let stored = analysis_path(file_path).file(kind);
        let file = directory.join(stored.trim_start_matches('/'));
        fs::create_dir_all(file.parent().expect("the analysis file has a folder"))
            .expect("the analysis folder is written");
        fs::write(&file, files.file(kind)).expect("the analysis file is written");
    }
}

/// A stick holding one track at `/Contents/AK1.mp3`, as Dubplate exported it with `exported`,
/// the track's id and those cues as Dubplate's `TrackCues`.
fn stick_with_track(directory: &Path, exported: Vec<AnalysisCue>) -> (TrackId, TrackCues) {
    let file_path = "/Contents/AK1.mp3";
    let mut library = DeviceLibrary::new(LibraryId(1), "2026-11-22");
    library.tracks.push(track(1, "Akufanele", file_path));
    write_stick(directory, &library);
    write_analysis(directory, file_path, &exported);
    (id(1), TrackCues::from_analysis(&exported))
}

#[test]
fn a_history_reads_back_in_the_order_the_player_played_it() {
    let stick = tempfile::tempdir().expect("a temporary stick");
    let mut library = DeviceLibrary::new(LibraryId(1), "2026-11-22");
    library
        .tracks
        .push(track(1, "Akufanele", "/Contents/AK1.mp3"));
    library
        .tracks
        .push(track(2, "Ngiyabonga", "/Contents/AK2.mp3"));
    let played = vec![id(2), id(1), TrackId::new(9).expect("the test id is not 0")];
    library
        .record_history("HISTORY 001", &played)
        .expect("the history is recorded");
    write_stick(stick.path(), &library);

    let histories = stick_histories(stick.path()).expect("the histories read back");
    assert_eq!(histories.len(), 1, "one history was recorded");
    assert_eq!(histories[0].name, "HISTORY 001");
    let titles: Vec<Option<&str>> = histories[0]
        .tracks
        .iter()
        .map(|track| track.track.as_ref().map(|track| track.title.as_str()))
        .collect();
    assert_eq!(
        titles,
        vec![Some("Ngiyabonga"), Some("Akufanele"), None],
        "the tracks stay in playing order, and the one the library does not hold reads back as missing"
    );
    let first = histories[0].tracks[0]
        .track
        .as_ref()
        .expect("the track is in the library");
    assert_eq!(first.file_path, "/Contents/AK2.mp3");
}

#[test]
fn each_history_keeps_its_own_order() {
    let stick = tempfile::tempdir().expect("a temporary stick");
    let mut library = DeviceLibrary::new(LibraryId(1), "2026-11-22");
    library
        .tracks
        .push(track(1, "Akufanele", "/Contents/AK1.mp3"));
    library
        .tracks
        .push(track(2, "Ngiyabonga", "/Contents/AK2.mp3"));
    library
        .record_history("HISTORY 001", &[id(2), id(1)])
        .expect("the first history is recorded");
    library
        .record_history("HISTORY 002", &[id(1)])
        .expect("the second history is recorded");
    write_stick(stick.path(), &library);

    let histories = stick_histories(stick.path()).expect("the histories read back");
    let names: Vec<&str> = histories
        .iter()
        .map(|history| history.name.as_str())
        .collect();
    assert_eq!(names, vec!["HISTORY 001", "HISTORY 002"]);
    let first_titles: Vec<&str> = histories[0]
        .tracks
        .iter()
        .map(|track| {
            track
                .track
                .as_ref()
                .expect("the track is in the library")
                .title
                .as_str()
        })
        .collect();
    assert_eq!(first_titles, vec!["Ngiyabonga", "Akufanele"]);
    assert_eq!(histories[1].tracks.len(), 1);
}

#[test]
fn a_hot_cue_the_player_added_reads_back_as_added() {
    let stick = tempfile::tempdir().expect("a temporary stick");
    let file_path = "/Contents/AK1.mp3";
    let (track_id, dubplate) = stick_with_track(stick.path(), vec![hot_cue(1, 1_000)]);

    let played = vec![hot_cue(1, 1_000), hot_cue(3, 8_000)];
    write_analysis(stick.path(), file_path, &played);

    let changes =
        stick_cue_changes(stick.path(), [(track_id, dubplate)]).expect("the changes read back");
    let track_changes = changes.get(track_id);
    assert_eq!(
        track_changes.len(),
        1,
        "only the cue the player added changed"
    );
    let added = match &track_changes[0] {
        CueChange::HotCueSet { pad, cue } => (*pad, cue.start_ms),
        other => panic!("the added hot cue is on pad C: {other:?}"),
    };
    assert_eq!(
        added,
        (HotCuePad::C, 8_000),
        "the added hot cue is on pad C where the player put it"
    );
    let changed: Vec<TrackId> = changes.changed().map(|(track_id, _)| track_id).collect();
    assert_eq!(changed, vec![track_id]);
}

#[test]
fn a_memory_cue_the_player_moved_reads_back_as_moved() {
    let stick = tempfile::tempdir().expect("a temporary stick");
    let file_path = "/Contents/AK1.mp3";
    let (track_id, dubplate) = stick_with_track(stick.path(), vec![memory_cue(5_000)]);

    write_analysis(stick.path(), file_path, &[memory_cue(6_500)]);

    let changes =
        stick_cue_changes(stick.path(), [(track_id, dubplate)]).expect("the changes read back");
    assert!(
        move_pair(changes.get(track_id), 5_000, 6_500),
        "a memory cue at a new position reads back as gone where it was and new where it is: {:?}",
        changes.get(track_id)
    );
}

/// The changes are exactly one memory cue gone at `from_ms` and one memory cue new at
/// `to_ms`, nothing else: how the format reads back a memory cue at a new position.
fn move_pair(changes: &[CueChange], from_ms: u32, to_ms: u32) -> bool {
    let removed = changes.iter().filter_map(|change| match change {
        CueChange::MemoryCueRemoved(cue) if cue.start_ms == from_ms => Some(true),
        _ => None,
    });
    let added = changes.iter().filter_map(|change| match change {
        CueChange::MemoryCueAdded(cue) if cue.start_ms == to_ms => Some(true),
        _ => None,
    });
    changes.len() == 2 && removed.count() == 1 && added.count() == 1
}

#[test]
fn a_recoloured_or_renamed_cue_reads_back_as_changed_in_place() {
    let stick = tempfile::tempdir().expect("a temporary stick");
    let file_path = "/Contents/AK1.mp3";
    let (track_id, dubplate) =
        stick_with_track(stick.path(), vec![named_memory_cue(5_000, "Drop")]);

    let played = AnalysisCue {
        memory_color: Some(5),
        ..named_memory_cue(5_000, "Intro")
    };
    write_analysis(stick.path(), file_path, &[played]);

    let changes =
        stick_cue_changes(stick.path(), [(track_id, dubplate)]).expect("the changes read back");
    assert!(
        matches!(
            changes.get(track_id),
            [CueChange::MemoryCueChanged { before, after }]
                if before.start_ms == after.start_ms
                    && before.name == "Drop"
                    && after.name == "Intro"
        ),
        "a cue recoloured and renamed where it stood is one change in place: {:?}",
        changes.get(track_id)
    );
}

#[test]
fn the_same_stick_gives_no_changes() {
    let stick = tempfile::tempdir().expect("a temporary stick");
    let exported = vec![
        hot_cue(1, 1_000),
        hot_cue(8, 90_000),
        named_memory_cue(5_000, "Drop"),
    ];
    let (track_id, dubplate) = stick_with_track(stick.path(), exported);

    let changes = stick_cue_changes(stick.path(), [(track_id, dubplate.clone())])
        .expect("the changes read back");
    assert!(changes.changed().next().is_none(), "nothing changed");

    let stick_cues = stick_track_cues(stick.path()).expect("the stick's cues read back");
    let stick_cues = stick_cues.get(track_id).expect("the track is on the stick");
    assert!(!stick_cues.missing);
    assert_eq!(
        stick_cues.cues, dubplate,
        "the stick holds what Dubplate wrote"
    );
}

#[test]
fn a_cue_within_one_millisecond_of_where_it_was_is_the_same_cue() {
    let stick = tempfile::tempdir().expect("a temporary stick");
    let file_path = "/Contents/AK1.mp3";
    let (track_id, dubplate) =
        stick_with_track(stick.path(), vec![memory_cue(5_000), hot_cue(1, 1_000)]);

    write_analysis(
        stick.path(),
        file_path,
        &[memory_cue(5_001), hot_cue(1, 999)],
    );
    let changes = stick_cue_changes(stick.path(), [(track_id, dubplate.clone())])
        .expect("the changes read back");
    assert!(
        changes.get(track_id).is_empty(),
        "one millisecond is not a move"
    );

    write_analysis(
        stick.path(),
        file_path,
        &[memory_cue(5_002), hot_cue(1, 1_000)],
    );
    let changes =
        stick_cue_changes(stick.path(), [(track_id, dubplate)]).expect("the changes read back");
    assert!(
        move_pair(changes.get(track_id), 5_000, 5_002),
        "two milliseconds is a move: {:?}",
        changes.get(track_id)
    );
}

#[test]
fn a_missing_export_reads_back_as_a_clean_error() {
    let stick = tempfile::tempdir().expect("a temporary stick");
    let error = stick_histories(stick.path()).expect_err("there is no export");
    assert!(
        matches!(error, StickImportError::NoExport(ref path) if path == stick.path()),
        "a stick without an export says so: {error}"
    );
}

#[test]
fn a_corrupt_export_reads_back_as_a_clean_error() {
    let stick = tempfile::tempdir().expect("a temporary stick");
    let rekordbox = stick.path().join("PIONEER").join("rekordbox");
    fs::create_dir_all(&rekordbox).expect("the stick folders are written");
    fs::write(
        rekordbox.join("export.pdb"),
        b"not a device database at all",
    )
    .expect("the corrupt file is written");

    let error = stick_histories(stick.path()).expect_err("the export is corrupt");
    assert!(
        matches!(error, StickImportError::Library { .. }),
        "histories: {error}"
    );

    let error = stick_track_cues(stick.path()).expect_err("the export is corrupt");
    assert!(
        matches!(error, StickImportError::Library { .. }),
        "cues: {error}"
    );
}

#[test]
fn a_track_without_analysis_files_reads_back_empty_and_marked_missing() {
    let stick = tempfile::tempdir().expect("a temporary stick");
    let mut library = DeviceLibrary::new(LibraryId(1), "2026-11-22");
    library
        .tracks
        .push(track(1, "Akufanele", "/Contents/AK1.mp3"));
    write_stick(stick.path(), &library);

    let stick_cues = stick_track_cues(stick.path()).expect("the stick's cues read back");
    let stick_cues = stick_cues.get(id(1)).expect("the track is on the stick");
    assert!(
        stick_cues.missing,
        "the stick holds no analysis file for the track"
    );
    assert_eq!(stick_cues.cues, TrackCues::default());
}

#[test]
fn a_track_not_on_the_stick_gets_no_changes_reported() {
    let stick = tempfile::tempdir().expect("a temporary stick");
    let (_, dubplate) = stick_with_track(stick.path(), vec![memory_cue(5_000)]);

    let changes =
        stick_cue_changes(stick.path(), [(id(2), dubplate)]).expect("the changes read back");
    assert!(
        changes.get(id(2)).is_empty(),
        "the stick cannot say what the player did to a track it does not hold"
    );
}

#[test]
fn a_track_cue_removed_by_the_player_reads_back_as_removed() {
    let stick = tempfile::tempdir().expect("a temporary stick");
    let file_path = "/Contents/AK1.mp3";
    let (track_id, dubplate) =
        stick_with_track(stick.path(), vec![hot_cue(1, 1_000), memory_cue(5_000)]);

    write_analysis(stick.path(), file_path, &[]);

    let changes =
        stick_cue_changes(stick.path(), [(track_id, dubplate)]).expect("the changes read back");
    assert!(
        changes.get(track_id).contains(&CueChange::HotCueCleared {
            pad: HotCuePad::A,
            cue: HotCue::at(1_000),
        }),
        "the pad the player cleared reads back as cleared: {:?}",
        changes.get(track_id)
    );
    assert!(
        changes.get(track_id).iter().any(
            |change| matches!(change, CueChange::MemoryCueRemoved(cue) if cue.start_ms == 5_000)
        ),
        "the memory cue the player deleted reads back as removed"
    );
}
