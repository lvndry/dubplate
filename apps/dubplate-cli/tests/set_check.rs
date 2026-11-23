//! Exercise the public command against an isolated, saved set.
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use dubplate_library::{DataFolder, FolderScan, LibraryCache, LibraryTrack};
use dubplate_prep::{SetEntry, SetList, SetStore};
use serde_json::Value;

fn check(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dubplate"))
        .args(["set", "check"])
        .args(args)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home)
        .env("APPDATA", home)
        .output()
        .expect("run set check")
}

#[test]
fn checks_a_saved_running_order_and_returns_machine_readable_results() {
    let home = tempfile::tempdir().unwrap();
    let config = if cfg!(target_os = "macos") {
        home.path().join("Library/Application Support")
    } else {
        home.path().to_path_buf()
    };
    let data = DataFolder::at(config.join("dubplate"));
    let mut store = SetStore::open(&data.sets()).unwrap();
    let set = store.create_set("Friday", 60).unwrap();
    let music = home.path().join("house");
    fs::create_dir(&music).unwrap();
    let music = music.canonicalize().unwrap();
    fs::write(
        data.settings(),
        format!("music_folders = [{music:?}]\nimport_rekordbox = false\n"),
    )
    .unwrap();

    let mut tracks = Vec::new();
    for (index, (title, bpm)) in [("Opening", 115.0), ("Peak", 128.0)]
        .into_iter()
        .enumerate()
    {
        let path = music.join(format!("{title}.wav"));
        fs::write(&path, []).unwrap();
        store
            .insert_entry(
                set,
                SetList::Main,
                index,
                SetEntry {
                    track_path: path.clone(),
                    grid_confirmed: true,
                    mix_in_ms: Some(0),
                    mix_out_ms: Some(8000),
                    ..Default::default()
                },
            )
            .unwrap();
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        let modified_ms = modified
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        tracks.push(LibraryTrack {
            path,
            title: Some(title.into()),
            artist: None,
            album: None,
            genre: None,
            comment: None,
            bpm: Some(bpm),
            key: Some("8A".into()),
            duration_seconds: Some(10.0),
            size: 0,
            modified_ms: u64::try_from(modified_ms).unwrap(),
        });
    }
    LibraryCache::new(vec![FolderScan {
        folder: music.clone(),
        tracks,
        problems: Vec::new(),
        scanned_at_ms: 0,
    }])
    .save(&data.library_cache())
    .unwrap();
    store
        .insert_entry(
            set,
            SetList::Backups,
            0,
            SetEntry {
                track_path: music.join("missing-backup.wav"),
                ..Default::default()
            },
        )
        .unwrap();

    // Another process (the desktop) can keep the library open while preflight runs.
    let ready = check(home.path(), &["friday", "--json"]);
    assert_eq!(
        ready.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&ready.stderr)
    );
    let report: Value = serde_json::from_slice(&ready.stdout).unwrap();
    assert_eq!(report["set"], "Friday");
    assert_eq!(report["ready"], true);
    assert_eq!(report["tracks"][0]["position"], 1);
    assert_eq!(report["backups"][0]["ready"], false);
    assert_eq!(report["warnings"][0]["kind"], "tempo_gap");
    assert_eq!(report["warnings"][0]["from"], 1);
    assert_eq!(report["warnings"][0]["to"], 2);

    // A replacement with unreadable tags must not inherit the old scan's key and tempo.
    fs::write(music.join("Peak.wav"), b"unreadable replacement").unwrap();
    let changed = check(home.path(), &["Friday", "--json"]);
    assert_eq!(changed.status.code(), Some(2));
    let report: Value = serde_json::from_slice(&changed.stdout).unwrap();
    assert_eq!(report["tracks"][1]["ready"], false);
    assert_eq!(report["warnings"], serde_json::json!([]));

    fs::remove_file(music.join("Peak.wav")).unwrap();
    let missing = check(home.path(), &["Friday"]);
    assert_eq!(missing.status.code(), Some(2));
    let text = String::from_utf8(missing.stdout).unwrap();
    assert!(text.contains("1/2 main tracks ready"));
    assert!(text.contains("Peak — File missing"));
    assert!(!data.analysis_cache().exists());

    let empty = store.create_set("Empty", 10).unwrap();
    assert!(store.load_set(empty).unwrap().entries.is_empty());
    assert_eq!(check(home.path(), &["Empty"]).status.code(), Some(2));
    let unknown = check(home.path(), &["No such set", "--json"]);
    assert_eq!(unknown.status.code(), Some(1));
    assert!(unknown.stdout.is_empty());
}
