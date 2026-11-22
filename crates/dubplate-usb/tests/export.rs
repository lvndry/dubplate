//! Exports small WAV tracks to scratch folders and reads every file back: the library with
//! Dubplate's reader, the analysis files with Dubplate's analysis reader. Never a real stick.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use dubplate_rekordbox::TrackAnalysis;
use dubplate_rekordbox::analysis::{AnalysisCue, Beat, BeatGrid, HotCueColor};
use dubplate_rekordbox::device_library::{DeviceLibrary, parse_export_ext_tags};
use dubplate_usb::{TrackTags, UsbExportError, UsbPlaylist, UsbTrack, export_to_usb};
use jiff::civil::date;

const SAMPLE_RATE: u32 = 44_100;

/// A mono 16-bit WAV file of `seconds` of a 60 Hz kick-like pulse every half second.
fn write_wav(path: &Path, seconds: u32) {
    let frames = SAMPLE_RATE * seconds;
    let data_length = frames * 2;
    let mut bytes = Vec::with_capacity(44 + data_length as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_length).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    bytes.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_length.to_le_bytes());
    for frame in 0..frames {
        let in_beat = frame % (SAMPLE_RATE / 2);
        let envelope = if in_beat < SAMPLE_RATE / 20 {
            0.8
        } else {
            0.05
        };
        let phase = std::f32::consts::TAU * 60.0 * frame as f32 / SAMPLE_RATE as f32;
        let sample = (phase.sin() * envelope * f32::from(i16::MAX)) as i16;
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("music folder");
    std::fs::write(path, bytes).expect("the WAV is written");
}

fn grid() -> BeatGrid {
    BeatGrid {
        beats: (0..8)
            .map(|index| Beat {
                beat_in_bar: (index % 4) as u16 + 1,
                tempo_hundredths: 12_000,
                time_ms: index * 500,
            })
            .collect(),
    }
}

fn track(source: PathBuf, title: &str, artist: &str) -> UsbTrack {
    UsbTrack {
        source,
        tags: TrackTags {
            title: Some(title.to_owned()),
            artist: Some(artist.to_owned()),
            genre: Some("Amapiano".to_owned()),
            key: Some("8A".to_owned()),
            rating: 4,
            ..TrackTags::default()
        },
        bpm: Some(120.0),
        duration_seconds: None,
        beat_grid: Some(grid()),
        grid_is_rekordbox_s: false,
        cues: vec![
            AnalysisCue {
                hot_cue: Some(1),
                pad: Some('A'),
                time_ms: 1_000,
                loop_end_ms: None,
                loop_length: None,
                name: Some("Drop".to_owned()),
                memory_color: None,
                hot_cue_color: Some(HotCueColor {
                    code: 42,
                    display: None,
                    led: None,
                }),
            },
            AnalysisCue {
                hot_cue: None,
                pad: None,
                time_ms: 0,
                loop_end_ms: None,
                loop_length: None,
                name: Some("Mix in".to_owned()),
                memory_color: Some(5),
                hot_cue_color: None,
            },
        ],
        rekordbox_analysis: None,
    }
}

struct Scratch {
    _folder: tempfile::TempDir,
    music: PathBuf,
    stick: PathBuf,
}

fn scratch() -> Scratch {
    let folder = tempfile::tempdir().expect("a scratch folder");
    let music = folder.path().join("music");
    let stick = folder.path().join("STICK");
    std::fs::create_dir_all(&stick).expect("the stick folder");
    write_wav(&music.join("uthando.wav"), 4);
    write_wav(&music.join("ngiyabonga.wav"), 3);
    Scratch {
        _folder: folder,
        music,
        stick,
    }
}

fn set(scratch: &Scratch) -> UsbPlaylist {
    UsbPlaylist {
        name: "Saturday".to_owned(),
        tracks: vec![
            track(scratch.music.join("uthando.wav"), "Uthando", "Shimza"),
            track(scratch.music.join("ngiyabonga.wav"), "Ngiyabonga", "Mörda"),
        ],
    }
}

fn read_library(stick: &Path) -> DeviceLibrary {
    let bytes = std::fs::read(stick.join("PIONEER/rekordbox/export.pdb")).expect("export.pdb");
    DeviceLibrary::parse_export_pdb(&bytes).expect("the library reads")
}

/// Every file under `folder`, with its bytes.
fn snapshot(folder: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut pending = vec![folder.to_path_buf()];
    while let Some(current) = pending.pop() {
        for entry in std::fs::read_dir(&current).expect("readable").flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.insert(path.clone(), std::fs::read(&path).expect("readable"));
            }
        }
    }
    files
}

#[test]
fn a_set_lands_on_a_stick_as_rekordbox_lays_it_out() {
    let scratch = scratch();
    let mut steps = Vec::new();
    let report = export_to_usb(
        &scratch.stick,
        &[set(&scratch)],
        date(2026, 11, 22),
        &mut |progress| steps.push(progress),
    )
    .expect("the export succeeds");
    assert_eq!(
        (report.tracks, report.copied, report.already_there),
        (2, 2, 0)
    );
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(steps.last().map(|step| step.done), Some(3));

    let audio = scratch
        .stick
        .join("Contents/Shimza/UnknownAlbum/uthando.wav");
    assert_eq!(
        std::fs::read(&audio).unwrap(),
        std::fs::read(scratch.music.join("uthando.wav")).unwrap()
    );
    assert!(
        scratch
            .stick
            .join("PIONEER/rekordbox/exportExt.pdb")
            .is_file()
    );
    assert_eq!(
        parse_export_ext_tags(
            &std::fs::read(scratch.stick.join("PIONEER/rekordbox/exportExt.pdb")).unwrap()
        )
        .unwrap()
        .len(),
        28
    );

    let library = read_library(&scratch.stick);
    assert_eq!(library.tracks.len(), 2);
    let uthando = library
        .tracks
        .iter()
        .find(|track| track.title == "Uthando")
        .unwrap();
    assert_eq!(
        uthando.file_path,
        "/Contents/Shimza/UnknownAlbum/uthando.wav"
    );
    assert_eq!(uthando.tempo_hundredths, 12_000);
    assert_eq!(uthando.duration_seconds, 4);
    assert_eq!(uthando.sample_rate, SAMPLE_RATE);
    assert_eq!(uthando.rating, 4);
    assert_eq!(uthando.analyze_date, "2026-11-22");
    let key = library
        .keys
        .iter()
        .find(|key| Some(key.id) == uthando.key)
        .unwrap();
    assert_eq!(key.name, "Am");
    let playlist = &library.playlists[0];
    assert_eq!(playlist.name, "Saturday");
    let order: Vec<_> = library
        .playlist_entries
        .iter()
        .map(|entry| entry.track)
        .collect();
    assert_eq!(order.len(), 2);
    assert_eq!(order[0], uthando.id);

    let analysis =
        TrackAnalysis::read(&scratch.stick, &uthando.analyze_path).expect("the analysis reads");
    assert_eq!(analysis.sources.len(), 3);
    assert_eq!(
        analysis.audio_path.as_deref(),
        Some(uthando.file_path.as_str())
    );
    assert_eq!(analysis.beat_grid, Some(grid()));
    assert_eq!(analysis.cues.len(), 2);
    let hot = analysis
        .cues
        .iter()
        .find(|cue| cue.hot_cue == Some(1))
        .unwrap();
    assert_eq!(hot.name.as_deref(), Some("Drop"));
    assert_eq!(hot.hot_cue_color.map(|color| color.code), Some(42));
    let memory = analysis
        .cues
        .iter()
        .find(|cue| cue.hot_cue.is_none())
        .unwrap();
    assert_eq!(memory.memory_color, Some(5));
    let waveforms = &analysis.waveforms;
    assert_eq!(
        waveforms.three_band_detail.as_ref().map(Vec::len),
        Some(600)
    );
    assert_eq!(waveforms.color_preview.as_ref().map(Vec::len), Some(1200));
    assert_eq!(waveforms.preview.as_ref().map(Vec::len), Some(400));
}

#[test]
fn a_second_export_adds_to_dubplate_s_library_and_copies_nothing_twice() {
    let scratch = scratch();
    export_to_usb(
        &scratch.stick,
        &[set(&scratch)],
        date(2026, 11, 22),
        &mut |_| {},
    )
    .unwrap();
    let first = read_library(&scratch.stick);

    let mut sunday = set(&scratch);
    sunday.name = "Sunday".to_owned();
    sunday.tracks.reverse();
    let mut saturday = set(&scratch);
    saturday.tracks.truncate(1);
    let report = export_to_usb(
        &scratch.stick,
        &[saturday, sunday],
        date(2026, 11, 23),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!((report.copied, report.already_there), (0, 2));

    let second = read_library(&scratch.stick);
    assert_eq!(second.library_id, first.library_id);
    assert_eq!(second.tracks.len(), 2);
    for track in &first.tracks {
        let again = second
            .tracks
            .iter()
            .find(|other| other.id == track.id)
            .unwrap();
        assert_eq!(again.file_path, track.file_path);
    }
    let names: Vec<&str> = second
        .playlists
        .iter()
        .map(|node| node.name.as_str())
        .collect();
    assert_eq!(names, ["Saturday", "Sunday"]);
    let saturday_entries = second
        .playlist_entries
        .iter()
        .filter(|entry| entry.playlist == second.playlists[0].id)
        .count();
    assert_eq!(
        saturday_entries, 1,
        "a playlist of the same name is replaced"
    );
}

#[test]
fn a_library_rekordbox_wrote_is_left_exactly_as_it_was() {
    let scratch = scratch();
    let rekordbox = scratch.stick.join("PIONEER/rekordbox");
    std::fs::create_dir_all(&rekordbox).unwrap();
    std::fs::write(rekordbox.join("export.pdb"), b"rekordbox's library").unwrap();
    std::fs::create_dir_all(scratch.stick.join("Contents")).unwrap();
    std::fs::write(scratch.stick.join("Contents/theirs.mp3"), b"audio").unwrap();
    let before = snapshot(&scratch.stick);
    let result = export_to_usb(
        &scratch.stick,
        &[set(&scratch)],
        date(2026, 11, 22),
        &mut |_| {},
    );
    assert!(
        matches!(result, Err(UsbExportError::ForeignLibrary { .. })),
        "{result:?}"
    );
    assert_eq!(snapshot(&scratch.stick), before);
}

#[test]
fn a_file_dubplate_did_not_write_is_never_replaced() {
    let scratch = scratch();
    let theirs = scratch
        .stick
        .join("Contents/Shimza/UnknownAlbum/uthando.wav");
    std::fs::create_dir_all(theirs.parent().unwrap()).unwrap();
    std::fs::write(&theirs, b"someone else's file").unwrap();
    export_to_usb(
        &scratch.stick,
        &[set(&scratch)],
        date(2026, 11, 22),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(std::fs::read(&theirs).unwrap(), b"someone else's file");
    let library = read_library(&scratch.stick);
    assert!(
        library
            .tracks
            .iter()
            .any(|track| track.file_path == "/Contents/Shimza/UnknownAlbum/uthando (2).wav")
    );
}

#[test]
fn rekordbox_s_folders_are_refused() {
    let scratch = scratch();
    let inside = scratch.stick.join("Pioneer").join("rekordbox");
    std::fs::create_dir_all(&inside).unwrap();
    let result = export_to_usb(&inside, &[set(&scratch)], date(2026, 11, 22), &mut |_| {});
    assert!(matches!(result, Err(UsbExportError::InsideRekordbox(_))));
    assert_eq!(std::fs::read_dir(&inside).unwrap().count(), 0);
}

#[test]
fn players_setting_files_alone_do_not_make_a_library() {
    let scratch = scratch();
    std::fs::create_dir_all(scratch.stick.join("PIONEER")).unwrap();
    std::fs::write(scratch.stick.join("PIONEER/MYSETTING.DAT"), b"settings").unwrap();
    export_to_usb(
        &scratch.stick,
        &[set(&scratch)],
        date(2026, 11, 22),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(
        std::fs::read(scratch.stick.join("PIONEER/MYSETTING.DAT")).unwrap(),
        b"settings"
    );
}
