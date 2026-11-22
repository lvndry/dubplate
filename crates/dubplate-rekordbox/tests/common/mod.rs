//! A small encrypted library built from the rekordbox 7.2.19 schema, shared by the tests.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use dubplate_rekordbox::{DatabaseKey, Library};
use rusqlite::Connection;

/// The analysis path of "Saltwater Hymn", relative to the share directory.
pub const SALTWATER_ANALYSIS: &str = "/PIONEER/USBANLZ/5a1/7e0c-1d2f/ANLZ0000.DAT";
pub const TEST_KEY: &str = "dubplate-test-key";
const SCHEMA: &str = include_str!("../../schema/rekordbox-7.2.19.sql");
const STAMP: &str = "2026-11-21 20:00:00.000 +00:00";

pub fn build_library(directory: &Path) -> PathBuf {
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
             (ID, Title, ArtistID, KeyID, BPM, Length, FolderPath, FileSize, Rating, rb_local_deleted,
              AnalysisDataPath, created_at, updated_at) VALUES
             ('t1', 'Saltwater Hymn', 'a1', 'k1', 12400, 408, '/Music/saltwater.flac', 81600000, 4, 0,
              '{SALTWATER_ANALYSIS}', '{STAMP}', '{STAMP}'),
             ('t2', 'Low Orbit', 'a2', 'k2', 12550, 424, '/Music/low-orbit.mp3', NULL, 0, 0,
              NULL, '{STAMP}', '{STAMP}'),
             ('t3', 'Deleted Track', 'a1', 'k1', 12000, 300, '/Music/gone.wav', -1, 0, 1,
              NULL, '{STAMP}', '{STAMP}');
         INSERT INTO djmdMixerParam
             (ID, ContentID, GainHigh, GainLow, PeakHigh, PeakLow, rb_local_deleted,
              created_at, updated_at) VALUES
             ('m1', 't1', 16268, 52429, 16256, 0, 0, '{STAMP}', '{STAMP}'),
             ('m2', 't2', 16128, 0, 16256, 0, 1, '{STAMP}', '{STAMP}');
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

pub fn open_fixture() -> (tempfile::TempDir, Library) {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = build_library(directory.path());
    let library = Library::open_copy(&path, &DatabaseKey::new(TEST_KEY)).expect("open fixture");
    (directory, library)
}
