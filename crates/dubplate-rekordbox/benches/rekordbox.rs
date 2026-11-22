//! Reading rekordbox: a large track's analysis files and a 352-track library, both built in a
//! temporary folder so the bench never opens the DJ's own files.

use std::hint::black_box;
use std::path::Path;

use criterion::{Criterion, criterion_group, criterion_main};
use dubplate_rekordbox::analysis::{AnalysisFile, Section, TrackAnalysis};
use dubplate_rekordbox::{DatabaseKey, Library};
use rusqlite::Connection;

/// A ten-minute track: 90,000 detail columns at 150 a second, about 1,250 beats at 124 BPM.
const TRACK_SECONDS: u32 = 600;
const DETAIL_COLUMNS: u32 = TRACK_SECONDS * 150;
const BEAT_COUNT: u32 = TRACK_SECONDS * 124 / 60;
const LIBRARY_TRACKS: usize = 352;
const LIBRARY_PLAYLISTS: usize = 24;
const CUES_PER_TRACK: usize = 6;
const KEY: &str = "dubplate-bench-key";
const SCHEMA: &str = include_str!("../schema/rekordbox-7.2.19.sql");
const STAMP: &str = "2026-11-21 20:00:00.000 +00:00";
const ANALYSIS_PATH: &str = "/PIONEER/USBANLZ/bench/ANLZ0000.DAT";

fn section(fourcc: &[u8; 4], header_length: u32, body: &[u8]) -> Vec<u8> {
    let mut bytes = fourcc.to_vec();
    bytes.extend(header_length.to_be_bytes());
    bytes.extend((12 + body.len() as u32).to_be_bytes());
    bytes.extend(body);
    bytes
}

fn analysis_file(sections: &[Vec<u8>]) -> Vec<u8> {
    let content = sections.concat();
    let mut bytes = b"PMAI".to_vec();
    bytes.extend(0x1c_u32.to_be_bytes());
    bytes.extend((0x1c + content.len() as u32).to_be_bytes());
    bytes.extend([0, 0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
    bytes.extend(content);
    bytes
}

fn beat_grid() -> Vec<u8> {
    let mut body = Vec::new();
    body.extend(0_u32.to_be_bytes());
    body.extend(0x0008_0000_u32.to_be_bytes());
    body.extend(BEAT_COUNT.to_be_bytes());
    for beat in 0..BEAT_COUNT {
        body.extend(((beat % 4 + 1) as u16).to_be_bytes());
        body.extend(12_400_u16.to_be_bytes());
        body.extend((beat * 60_000 / 124 + 40).to_be_bytes());
    }
    section(b"PQTZ", 0x18, &body)
}

fn preview(fourcc: &[u8; 4], columns: usize) -> Vec<u8> {
    let data: Vec<u8> = (0..columns).map(|column| (column % 251) as u8).collect();
    let mut body = (data.len() as u32).to_be_bytes().to_vec();
    body.extend(0x0010_0000_u32.to_be_bytes());
    body.extend(data);
    section(fourcc, 0x14, &body)
}

fn sized_entries(fourcc: &[u8; 4], entry_size: u32, count: u32, with_unknown: bool) -> Vec<u8> {
    let data: Vec<u8> = (0..entry_size * count)
        .map(|byte| (byte.wrapping_mul(2_654_435_761) >> 24) as u8)
        .collect();
    let mut body = entry_size.to_be_bytes().to_vec();
    body.extend(count.to_be_bytes());
    let header_length = if with_unknown {
        body.extend(0_u32.to_be_bytes());
        0x18
    } else {
        0x14
    };
    body.extend(data);
    section(fourcc, header_length, &body)
}

/// The `.DAT` file: beat grid, the blue overview and the tiny overview.
fn dat_file() -> Vec<u8> {
    analysis_file(&[beat_grid(), preview(b"PWAV", 400), preview(b"PWV2", 100)])
}

/// The `.EXT` file: the scrolling blue and colour waveforms and the colour overview.
fn ext_file() -> Vec<u8> {
    analysis_file(&[
        sized_entries(b"PWV3", 1, DETAIL_COLUMNS, true),
        sized_entries(b"PWV4", 6, 1_200, true),
        sized_entries(b"PWV5", 2, DETAIL_COLUMNS, true),
    ])
}

/// The `.2EX` file: the 3-band overview and scrolling waveform.
fn two_ex_file() -> Vec<u8> {
    analysis_file(&[
        sized_entries(b"PWV6", 3, 1_200, false),
        sized_entries(b"PWV7", 3, DETAIL_COLUMNS, true),
    ])
}

/// Every height and colour of the scrolling waveforms, the way a renderer reads them.
fn decode_waveforms(analysis: &TrackAnalysis) -> u64 {
    let waveforms = &analysis.waveforms;
    let mut total = 0_u64;
    for column in waveforms.color_detail.iter().flatten() {
        total += u64::from(column.height())
            + u64::from(column.red())
            + u64::from(column.green())
            + u64::from(column.blue());
    }
    for column in waveforms.three_band_detail.iter().flatten() {
        total += u64::from(column.low) + u64::from(column.mid) + u64::from(column.high);
    }
    for column in waveforms.detail.iter().flatten() {
        total += u64::from(column.height()) + u64::from(column.whiteness());
    }
    total
}

fn write_analysis_files(share_directory: &Path) {
    let dat_path = share_directory.join(ANALYSIS_PATH.trim_start_matches('/'));
    std::fs::create_dir_all(dat_path.parent().expect("a parent folder")).expect("folders");
    std::fs::write(&dat_path, dat_file()).expect("write the DAT file");
    std::fs::write(dat_path.with_extension("EXT"), ext_file()).expect("write the EXT file");
    std::fs::write(dat_path.with_extension("2EX"), two_ex_file()).expect("write the 2EX file");
}

fn analysis(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("anlz");
    let ext = ext_file();
    group.bench_function("parse_ext_10_minutes", |bencher| {
        bencher.iter(|| AnalysisFile::parse(black_box(&ext)).expect("parse"));
    });
    let two_ex = two_ex_file();
    group.bench_function("parse_2ex_10_minutes", |bencher| {
        bencher.iter(|| AnalysisFile::parse(black_box(&two_ex)).expect("parse"));
    });
    let dat = dat_file();
    group.bench_function("parse_dat_1240_beats", |bencher| {
        bencher.iter(|| {
            let file = AnalysisFile::parse(black_box(&dat)).expect("parse");
            assert!(matches!(file.sections[0], Section::BeatGrid(_)));
            file
        });
    });

    let share = tempfile::tempdir().expect("a temporary folder");
    write_analysis_files(share.path());
    group.bench_function("read_track_analysis_from_disk", |bencher| {
        bencher.iter(|| TrackAnalysis::read(share.path(), ANALYSIS_PATH).expect("read"));
    });
    let track_analysis = TrackAnalysis::read(share.path(), ANALYSIS_PATH).expect("read");
    group.bench_function("decode_scrolling_waveforms", |bencher| {
        bencher.iter(|| decode_waveforms(black_box(&track_analysis)));
    });
    group.finish();
}

/// A library like a DJ's: 352 tracks by 60 artists in 24 keys, six cues each, and 24
/// playlists of 30 tracks in two folders.
fn build_library(directory: &Path) -> std::path::PathBuf {
    let path = directory.join("master.db");
    let connection = Connection::open(&path).expect("create the library");
    connection.pragma_update(None, "key", KEY).expect("key");
    connection.execute_batch(SCHEMA).expect("schema");
    connection
        .execute_batch("BEGIN")
        .expect("start a transaction");
    connection
        .execute(
            "INSERT INTO djmdProperty (DBID, DBVersion, created_at, updated_at)
             VALUES ('1', '6000', ?1, ?1)",
            [STAMP],
        )
        .expect("property");
    for artist in 0..60 {
        connection
            .execute(
                "INSERT INTO djmdArtist (ID, Name, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
                (format!("a{artist}"), format!("Artist {artist}"), STAMP),
            )
            .expect("artist");
    }
    for key in 0..24 {
        let name = format!("{}{}", key % 12 + 1, if key < 12 { 'A' } else { 'B' });
        connection
            .execute(
                "INSERT INTO djmdKey (ID, ScaleName, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
                (format!("k{key}"), name, STAMP),
            )
            .expect("key");
    }
    for track in 0..LIBRARY_TRACKS {
        connection
            .execute(
                "INSERT INTO djmdContent
                     (ID, Title, ArtistID, KeyID, BPM, Length, FolderPath, FileSize, Rating,
                      rb_local_deleted, AnalysisDataPath, StockDate, Commnt, created_at,
                      updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 420, ?6, 81600000, 3, 0, ?7, '2026-01-01',
                         'warm-up', ?8, ?8)",
                (
                    format!("t{track}"),
                    format!("Track {track:03}"),
                    format!("a{}", track % 60),
                    format!("k{}", track % 24),
                    11_800 + (track as i64 % 30) * 20,
                    format!("/Music/Artist {}/Track {track:03}.flac", track % 60),
                    ANALYSIS_PATH,
                    STAMP,
                ),
            )
            .expect("track");
        for cue in 0..CUES_PER_TRACK {
            connection
                .execute(
                    "INSERT INTO djmdCue
                         (ID, ContentID, Kind, InMsec, OutMsec, rb_local_deleted, created_at,
                          updated_at)
                     VALUES (?1, ?2, ?3, ?4, -1, 0, ?5, ?5)",
                    (
                        format!("c{track}-{cue}"),
                        format!("t{track}"),
                        cue as i64,
                        cue as i64 * 30_000,
                        STAMP,
                    ),
                )
                .expect("cue");
        }
    }
    for folder in 0..2 {
        connection
            .execute(
                "INSERT INTO djmdPlaylist (ID, ParentID, Name, Attribute, Seq, created_at, updated_at)
                 VALUES (?1, 'root', ?2, 1, ?3, ?4, ?4)",
                (format!("f{folder}"), format!("Folder {folder}"), folder, STAMP),
            )
            .expect("folder");
    }
    for playlist in 0..LIBRARY_PLAYLISTS {
        connection
            .execute(
                "INSERT INTO djmdPlaylist (ID, ParentID, Name, Attribute, Seq, created_at, updated_at)
                 VALUES (?1, ?2, ?3, 0, ?4, ?5, ?5)",
                (
                    format!("p{playlist}"),
                    format!("f{}", playlist % 2),
                    format!("Playlist {playlist}"),
                    playlist as i64,
                    STAMP,
                ),
            )
            .expect("playlist");
        for entry in 0..30 {
            connection
                .execute(
                    "INSERT INTO djmdSongPlaylist
                         (ID, PlaylistID, ContentID, TrackNo, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                    (
                        format!("e{playlist}-{entry}"),
                        format!("p{playlist}"),
                        format!("t{}", (playlist * 13 + entry * 7) % LIBRARY_TRACKS),
                        entry as i64,
                        STAMP,
                    ),
                )
                .expect("playlist entry");
        }
    }
    connection.execute_batch("COMMIT").expect("commit");
    path
}

fn library(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("library_352_tracks");
    group.sample_size(20);
    let directory = tempfile::tempdir().expect("a temporary folder");
    let master_db = build_library(directory.path());
    let key = DatabaseKey::new(KEY);
    group.bench_function("open_copy_and_read_everything", |bencher| {
        bencher.iter(|| {
            let library = Library::open_copy(&master_db, &key).expect("open");
            (
                library.tracks().expect("tracks"),
                library.cues().expect("cues"),
                library.playlists().expect("playlists"),
            )
        });
    });
    let library = Library::open_copy(&master_db, &key).expect("open");
    group.bench_function("read_tracks", |bencher| {
        bencher.iter(|| library.tracks().expect("tracks"));
    });
    group.bench_function("read_cues", |bencher| {
        bencher.iter(|| library.cues().expect("cues"));
    });
    group.bench_function("read_playlists", |bencher| {
        bencher.iter(|| library.playlists().expect("playlists"));
    });
    group.finish();
}

criterion_group!(benches, analysis, library);
criterion_main!(benches);
