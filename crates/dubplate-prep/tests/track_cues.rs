//! The cues a DJ makes in Dubplate are kept per track as its whole list: they come back as
//! they were saved, a deletion stays deleted, and the cues of a file follow it when it moves.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use dubplate_prep::{SetEditor, SetStore, TrackIdentity};
use dubplate_rekordbox::{HotCue, HotCuePad, MemoryCue, MemoryCueColor, PaletteColor, TrackCues};

fn cues() -> TrackCues {
    let mut cues = TrackCues::default();
    cues.set_hot_cue(
        HotCuePad::A,
        HotCue {
            start_ms: 32_123,
            loop_end_ms: Some(36_123),
            color: PaletteColor::from_index(42),
            name: "Drop roll".to_owned(),
        },
    );
    cues.set_hot_cue(HotCuePad::H, HotCue::at(301_000));
    cues.insert_memory_cue(MemoryCue {
        color: Some(MemoryCueColor::Aqua),
        name: "Break".to_owned(),
        ..MemoryCue::at(64_000)
    });
    cues.insert_memory_cue(MemoryCue {
        loop_end_ms: Some(130_000),
        ..MemoryCue::at(128_000)
    });
    cues
}

#[test]
fn cues_come_back_after_a_relaunch_exactly_as_saved() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("dubplate.db");
    let rekordbox = TrackIdentity::Rekordbox("258436394".to_owned());
    let file = TrackIdentity::File(PathBuf::from("/Music/Loose/Café.mp3"));
    {
        let mut store = SetStore::open(&path).unwrap();
        store
            .save_track_cues(&rekordbox, Some(Path::new("/Music/a.mp3")), &cues())
            .unwrap();
        store
            .save_track_cues(&file, Some(Path::new("/Music/Loose/Café.mp3")), &cues())
            .unwrap();
    }
    let reopened = SetStore::open(&path).unwrap();
    let saved = reopened.track_cues().unwrap();
    assert_eq!(saved.len(), 2);
    assert_eq!(saved.get(&rekordbox), Some(&cues()));
    assert_eq!(saved.get(&file), Some(&cues()));
}

#[test]
fn the_whole_list_is_kept_so_a_deleted_cue_stays_deleted() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = SetStore::open(&directory.path().join("dubplate.db")).unwrap();
    let identity = TrackIdentity::Rekordbox("7".to_owned());
    store.save_track_cues(&identity, None, &cues()).unwrap();

    let mut fewer = cues();
    fewer.delete_hot_cue(HotCuePad::A);
    fewer.remove_memory_cues(|cue| cue.loop_end_ms.is_none());
    store.save_track_cues(&identity, None, &fewer).unwrap();
    assert_eq!(store.track_cues().unwrap().get(&identity), Some(&fewer));

    store
        .save_track_cues(&identity, None, &TrackCues::default())
        .unwrap();
    assert_eq!(
        store.track_cues().unwrap().get(&identity),
        Some(&TrackCues::default()),
        "a track whose cues were all deleted keeps an empty list, which hides rekordbox's"
    );
}

#[test]
fn the_cues_of_a_moved_file_follow_it() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("dubplate.db");
    let old_place = PathBuf::from("/Music/Old/track.mp3");
    let new_place = PathBuf::from("/Music/New/track.mp3");
    let rekordbox = TrackIdentity::Rekordbox("9".to_owned());
    {
        let mut store = SetStore::open(&path).unwrap();
        store
            .save_track_cues(&TrackIdentity::File(old_place.clone()), None, &cues())
            .unwrap();
        store
            .save_track_cues(
                &TrackIdentity::File(new_place.clone()),
                None,
                &TrackCues::default(),
            )
            .unwrap();
        store
            .save_track_cues(&rekordbox, Some(old_place.as_path()), &cues())
            .unwrap();
    }
    let mut editor = SetEditor::open(&path).unwrap();
    editor
        .follow_files(&HashMap::from([(old_place.clone(), new_place.clone())]))
        .unwrap();
    let saved = editor.track_cues().unwrap();
    assert_eq!(saved.get(&TrackIdentity::File(old_place)), None);
    assert_eq!(
        saved.get(&TrackIdentity::File(new_place)),
        Some(&cues()),
        "the moved file's cues take the place of any kept for the new place"
    );
    assert_eq!(
        saved.get(&rekordbox),
        Some(&cues()),
        "a rekordbox track keeps its cues by its id"
    );
}
