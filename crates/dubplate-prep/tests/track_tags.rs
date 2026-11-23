//! The metadata the DJ edits on a track: the rating, colour label and comment, and the My Tag
//! system of categories of tags, kept in Dubplate's database, and kept across a relaunch.

use std::path::PathBuf;

use dubplate_prep::{
    SetStore, StoreError, TagCategoryId, TagId, TrackColour, TrackIdentity, TrackMetadata,
    TrackTagEdit,
};
use dubplate_rekordbox::Track;

fn file(path: &str) -> TrackIdentity {
    TrackIdentity::File(PathBuf::from(path))
}

fn rekordbox(id: &str) -> TrackIdentity {
    TrackIdentity::Rekordbox(id.to_owned())
}

fn track_without_metadata() -> Track {
    Track {
        id: String::new(),
        title: None,
        artist: None,
        album: None,
        genre: None,
        label: None,
        key: None,
        bpm: None,
        length_seconds: None,
        file_path: None,
        file_size: None,
        rating: None,
        comment: None,
        analysis_path: None,
        date_added: None,
        auto_gain_db: None,
    }
}

#[allow(clippy::unwrap_used)]
fn open() -> (tempfile::TempDir, SetStore) {
    let directory = tempfile::tempdir().unwrap();
    let store = SetStore::open(&directory.path().join("dubplate.db")).unwrap();
    (directory, store)
}

#[allow(clippy::unwrap_used)]
fn created_category(store: &mut SetStore, name: &str, position: usize) -> TagCategoryId {
    store
        .edit_track_tags(TrackTagEdit::CreateCategory {
            name: name.to_owned(),
            position,
        })
        .unwrap()
        .created_category
        .unwrap()
}

#[allow(clippy::unwrap_used)]
fn created_tag(
    store: &mut SetStore,
    category: TagCategoryId,
    name: &str,
    position: usize,
) -> TagId {
    store
        .edit_track_tags(TrackTagEdit::CreateTag {
            category,
            name: name.to_owned(),
            position,
        })
        .unwrap()
        .created_tag
        .unwrap()
}

fn tag_names(metadata: &TrackMetadata) -> Vec<String> {
    metadata.tags.iter().map(|tag| tag.name.clone()).collect()
}

#[test]
fn a_fresh_database_has_the_four_default_categories() {
    let (_directory, store) = open();
    let categories = store.tag_categories().unwrap();
    assert_eq!(
        categories
            .iter()
            .map(|category| (category.name.clone(), category.tags.len()))
            .collect::<Vec<_>>(),
        [
            ("Genre".to_owned(), 0),
            ("Components".to_owned(), 0),
            ("Situation".to_owned(), 0),
            ("Other".to_owned(), 0),
        ]
    );
}

#[test]
fn a_track_gets_a_rating_a_colour_and_a_comment() {
    let (_directory, mut store) = open();
    let track = file("/a.mp3");
    store
        .edit_track_tags(TrackTagEdit::SetRating {
            track: track.clone(),
            track_path: Some(PathBuf::from("/a.mp3")),
            rating: 5,
        })
        .unwrap();
    store
        .edit_track_tags(TrackTagEdit::SetColour {
            track: track.clone(),
            track_path: None,
            colour: Some(TrackColour::Purple),
        })
        .unwrap();
    store
        .edit_track_tags(TrackTagEdit::SetComment {
            track: track.clone(),
            track_path: None,
            comment: "play the second drop".to_owned(),
        })
        .unwrap();

    let stored = store.track_metadata(&track).unwrap().unwrap();
    assert_eq!(stored.rating, 5);
    assert_eq!(stored.colour, Some(TrackColour::Purple));
    assert_eq!(stored.comment, "play the second drop");
    assert_eq!(stored.track_path, Some(PathBuf::from("/a.mp3")));
    assert!(stored.tags.is_empty());

    store
        .edit_track_tags(TrackTagEdit::SetColour {
            track: track.clone(),
            track_path: None,
            colour: None,
        })
        .unwrap();
    let cleared = store.track_metadata(&track).unwrap().unwrap();
    assert_eq!(cleared.colour, None);
    assert_eq!(cleared.rating, 5, "clearing the colour keeps the rating");

    store
        .edit_track_tags(TrackTagEdit::SetRating {
            track: track.clone(),
            track_path: None,
            rating: 0,
        })
        .unwrap();
    assert_eq!(
        store.track_metadata(&track).unwrap().unwrap().rating,
        0,
        "rating 0 is unrated, not an error"
    );
    assert!(store.track_metadata(&rekordbox("9")).unwrap().is_none());
}

#[test]
fn a_rating_outside_five_stars_is_refused() {
    let (_directory, mut store) = open();
    assert!(matches!(
        store.edit_track_tags(TrackTagEdit::SetRating {
            track: file("/a.mp3"),
            track_path: None,
            rating: 6,
        }),
        Err(StoreError::RatingOutOfRange(6))
    ));
    assert!(store.track_metadata(&file("/a.mp3")).unwrap().is_none());
}

#[test]
fn tags_go_on_tracks_and_come_off_again() {
    let (_directory, mut store) = open();
    let category = store.tag_categories().unwrap()[0].id;
    let peak = created_tag(&mut store, category, "Peak", 0);
    let warm = created_tag(&mut store, category, "Warm", 1);
    let first = store
        .edit_track_tags(TrackTagEdit::AddTag {
            tracks: vec![file("/a.mp3"), file("/b.mp3")],
            tag: peak,
        })
        .unwrap();
    assert_eq!(first.added, 2);
    let again = store
        .edit_track_tags(TrackTagEdit::AddTag {
            tracks: vec![file("/a.mp3"), file("/a.mp3"), rekordbox("7")],
            tag: peak,
        })
        .unwrap();
    assert_eq!(again.added, 1, "a track keeps its tag once");
    store
        .edit_track_tags(TrackTagEdit::AddTag {
            tracks: vec![file("/a.mp3")],
            tag: warm,
        })
        .unwrap();
    store
        .edit_track_tags(TrackTagEdit::RemoveTag {
            tracks: vec![file("/a.mp3")],
            tag: peak,
        })
        .unwrap();

    let a = store.track_metadata(&file("/a.mp3")).unwrap().unwrap();
    assert_eq!(tag_names(&a), ["Warm"]);
    let b = store.track_metadata(&file("/b.mp3")).unwrap().unwrap();
    assert_eq!(tag_names(&b), ["Peak"]);
    let c = store.track_metadata(&rekordbox("7")).unwrap().unwrap();
    assert_eq!(tag_names(&c), ["Peak"]);
    assert!(
        store
            .track_metadata(&file("/no-tag.mp3"))
            .unwrap()
            .is_none()
    );

    assert!(matches!(
        store.edit_track_tags(TrackTagEdit::AddTag {
            tracks: vec![file("/x.mp3")],
            tag: TagId(99),
        }),
        Err(StoreError::TagNotFound(id)) if id == TagId(99)
    ));
}

#[test]
fn deleting_a_tag_takes_it_off_every_track() {
    let (_directory, mut store) = open();
    let category = store.tag_categories().unwrap()[0].id;
    let peak = created_tag(&mut store, category, "Peak", 0);
    store
        .edit_track_tags(TrackTagEdit::AddTag {
            tracks: vec![file("/a.mp3"), file("/b.mp3"), rekordbox("7")],
            tag: peak,
        })
        .unwrap();
    store
        .edit_track_tags(TrackTagEdit::DeleteTag(peak))
        .unwrap();

    for track in [file("/a.mp3"), file("/b.mp3"), rekordbox("7")] {
        assert!(
            store
                .track_metadata(&track)
                .unwrap()
                .map(|metadata| metadata.tags)
                .as_ref()
                .map(Vec::is_empty)
                .unwrap_or(true),
            "the tag is off the track"
        );
    }
    let categories = store.tag_categories().unwrap();
    assert!(categories[0].tags.is_empty());
    assert!(matches!(
        store.edit_track_tags(TrackTagEdit::DeleteTag(peak)),
        Err(StoreError::TagNotFound(id)) if id == peak
    ));
}

#[test]
fn categories_and_tags_can_be_added_renamed_and_reordered() {
    let (_directory, mut store) = open();
    let mood = created_category(&mut store, "Moods", usize::MAX);
    let deep = created_tag(&mut store, mood, "Deep", 0);
    let bright = created_tag(&mut store, mood, "Bright", 0);
    let dark = created_tag(&mut store, mood, "Dark", 1);
    let categories = store.tag_categories().unwrap();
    let made = &categories[4];
    assert_eq!((made.id, made.name.clone()), (mood, "Moods".to_owned()));
    assert_eq!(
        made.tags
            .iter()
            .map(|tag| (tag.id, tag.name.clone()))
            .collect::<Vec<_>>(),
        [
            (bright, "Bright".to_owned()),
            (dark, "Dark".to_owned()),
            (deep, "Deep".to_owned())
        ],
        "position 0 pushes the earlier tags back"
    );

    store
        .edit_track_tags(TrackTagEdit::RenameTag {
            tag: deep,
            name: "Deep and warm".to_owned(),
        })
        .unwrap();
    store
        .edit_track_tags(TrackTagEdit::MoveTag {
            tag: deep,
            position: 2,
        })
        .unwrap();
    store
        .edit_track_tags(TrackTagEdit::RenameCategory {
            category: mood,
            name: "Mood".to_owned(),
        })
        .unwrap();
    store
        .edit_track_tags(TrackTagEdit::MoveCategory {
            category: mood,
            position: 0,
        })
        .unwrap();
    let categories = store.tag_categories().unwrap();
    assert_eq!(
        (categories[0].id, categories[0].name.clone()),
        (mood, "Mood".to_owned())
    );
    assert_eq!(
        categories[0]
            .tags
            .iter()
            .map(|tag| (tag.id, tag.name.clone()))
            .collect::<Vec<_>>(),
        [
            (bright, "Bright".to_owned()),
            (dark, "Dark".to_owned()),
            (deep, "Deep and warm".to_owned()),
        ]
    );

    let unknown_category = TagCategoryId(99);
    assert!(matches!(
        store.edit_track_tags(TrackTagEdit::RenameCategory {
            category: unknown_category,
            name: "Nowhere".to_owned(),
        }),
        Err(StoreError::TagCategoryNotFound(id)) if id == unknown_category
    ));
    assert!(matches!(
        store.edit_track_tags(TrackTagEdit::CreateTag {
            category: unknown_category,
            name: "Nowhere".to_owned(),
            position: 0,
        }),
        Err(StoreError::TagCategoryNotFound(id)) if id == unknown_category
    ));
    assert!(matches!(
        store.edit_track_tags(TrackTagEdit::RenameTag {
            tag: TagId(99),
            name: "Nowhere".to_owned(),
        }),
        Err(StoreError::TagNotFound(id)) if id == TagId(99)
    ));
    assert!(matches!(
        store.edit_track_tags(TrackTagEdit::DeleteCategory(unknown_category)),
        Err(StoreError::TagCategoryNotFound(id)) if id == unknown_category
    ));
}

#[test]
fn deleting_a_category_deletes_its_tags_and_their_tracks() {
    let (_directory, mut store) = open();
    let category = created_category(&mut store, "Moods", usize::MAX);
    let first = created_tag(&mut store, category, "First", 0);
    let second = created_tag(&mut store, category, "Second", 0);
    store
        .edit_track_tags(TrackTagEdit::AddTag {
            tracks: vec![file("/a.mp3")],
            tag: first,
        })
        .unwrap();
    store
        .edit_track_tags(TrackTagEdit::AddTag {
            tracks: vec![file("/a.mp3"), file("/b.mp3")],
            tag: second,
        })
        .unwrap();
    store
        .edit_track_tags(TrackTagEdit::DeleteCategory(category))
        .unwrap();

    let categories = store.tag_categories().unwrap();
    assert!(categories.iter().all(|made| made.id != category));
    for track in [file("/a.mp3"), file("/b.mp3")] {
        let metadata = store.track_metadata(&track).unwrap();
        assert!(
            metadata
                .as_ref()
                .map(|metadata| metadata.tags.is_empty())
                .unwrap_or(true),
            "the category's tags are off its tracks"
        );
    }
    assert!(matches!(
        store.edit_track_tags(TrackTagEdit::DeleteCategory(category)),
        Err(StoreError::TagCategoryNotFound(id)) if id == category
    ));
}

#[test]
fn the_tracks_with_all_the_tags_are_found() {
    let (_directory, mut store) = open();
    let category = store.tag_categories().unwrap()[0].id;
    let peak = created_tag(&mut store, category, "Peak", 0);
    let big = created_tag(&mut store, category, "Big", 1);
    store
        .edit_track_tags(TrackTagEdit::AddTag {
            tracks: vec![file("/a.mp3"), file("/b.mp3"), rekordbox("7")],
            tag: peak,
        })
        .unwrap();
    store
        .edit_track_tags(TrackTagEdit::AddTag {
            tracks: vec![file("/a.mp3"), rekordbox("7")],
            tag: big,
        })
        .unwrap();
    store
        .edit_track_tags(TrackTagEdit::AddTag {
            tracks: vec![file("/c.mp3")],
            tag: big,
        })
        .unwrap();

    let both = store.tracks_with_all_tags(&[peak, big]).unwrap();
    assert_eq!(
        both,
        vec![file("/a.mp3"), rekordbox("7")],
        "the tracks that carry every one"
    );
    let peak_alone = store.tracks_with_all_tags(&[peak]).unwrap();
    assert_eq!(
        peak_alone,
        vec![file("/a.mp3"), file("/b.mp3"), rekordbox("7")]
    );
    assert!(store.tracks_with_all_tags(&[]).unwrap().is_empty());
    assert!(store.tracks_with_all_tags(&[TagId(99)]).unwrap().is_empty());
}

#[test]
fn the_metadata_of_many_tracks_comes_back_in_one_read() {
    let (_directory, mut store) = open();
    let category = store.tag_categories().unwrap()[0].id;
    let peak = created_tag(&mut store, category, "Peak", 0);
    store
        .edit_track_tags(TrackTagEdit::SetRating {
            track: file("/a.mp3"),
            track_path: Some(PathBuf::from("/a.mp3")),
            rating: 4,
        })
        .unwrap();
    store
        .edit_track_tags(TrackTagEdit::SetComment {
            track: file("/a.mp3"),
            track_path: None,
            comment: "a".to_owned(),
        })
        .unwrap();
    store
        .edit_track_tags(TrackTagEdit::AddTag {
            tracks: vec![file("/b.mp3")],
            tag: peak,
        })
        .unwrap();

    let stored = store
        .track_metadata_many(&[
            file("/a.mp3"),
            file("/b.mp3"),
            file("/unstored.mp3"),
            rekordbox("7"),
            file("/a.mp3"),
        ])
        .unwrap();
    assert_eq!(stored.len(), 2);
    let a = &stored[&file("/a.mp3")];
    assert_eq!(a.rating, 4);
    assert_eq!(a.comment, "a");
    assert_eq!(a.track_path, Some(PathBuf::from("/a.mp3")));
    assert!(a.tags.is_empty());
    let b = &stored[&file("/b.mp3")];
    assert_eq!(tag_names(b), ["Peak"]);
    assert!(!stored.contains_key(&file("/unstored.mp3")));
    assert!(!stored.contains_key(&rekordbox("7")));
    assert!(store.track_metadata_many(&[]).unwrap().is_empty());
}

#[test]
fn a_rekordbox_track_seeds_a_track_that_has_nothing_stored_yet() {
    let track = Track {
        id: "7".to_owned(),
        rating: Some(4),
        comment: Some("killer drop".to_owned()),
        ..track_without_metadata()
    };
    let seed = TrackMetadata::from_rekordbox(&track);
    assert_eq!(seed.rating, 4);
    assert_eq!(seed.comment, "killer drop");
    assert_eq!(seed.colour, None);
    assert!(seed.tags.is_empty());

    let unrated = Track {
        id: "8".to_owned(),
        rating: None,
        comment: None,
        ..track_without_metadata()
    };
    let seed = TrackMetadata::from_rekordbox(&unrated);
    assert_eq!(seed.rating, 0);
    assert_eq!(seed.comment, "");
}

#[test]
fn metadata_and_tags_survive_a_relaunch() {
    let (directory, mut store) = open();
    let category = store.tag_categories().unwrap()[0].id;
    let mood = created_category(&mut store, "Moods", usize::MAX);
    let peak = created_tag(&mut store, category, "Peak", 0);
    let late = created_tag(&mut store, mood, "Late", 0);
    store
        .edit_track_tags(TrackTagEdit::SetRating {
            track: file("/a.mp3"),
            track_path: Some(PathBuf::from("/a.mp3")),
            rating: 5,
        })
        .unwrap();
    store
        .edit_track_tags(TrackTagEdit::SetColour {
            track: file("/a.mp3"),
            track_path: None,
            colour: Some(TrackColour::Green),
        })
        .unwrap();
    store
        .edit_track_tags(TrackTagEdit::SetComment {
            track: file("/a.mp3"),
            track_path: None,
            comment: "closing".to_owned(),
        })
        .unwrap();
    store
        .edit_track_tags(TrackTagEdit::AddTag {
            tracks: vec![file("/a.mp3"), rekordbox("7")],
            tag: peak,
        })
        .unwrap();
    store
        .edit_track_tags(TrackTagEdit::AddTag {
            tracks: vec![rekordbox("7")],
            tag: late,
        })
        .unwrap();
    drop(store);

    let store = SetStore::open(&directory.path().join("dubplate.db")).unwrap();
    let a = store.track_metadata(&file("/a.mp3")).unwrap().unwrap();
    assert_eq!((a.rating, a.colour), (5, Some(TrackColour::Green)));
    assert_eq!(a.comment, "closing");
    assert_eq!(tag_names(&a), ["Peak"]);
    let b = store.track_metadata(&rekordbox("7")).unwrap().unwrap();
    assert_eq!(
        b.tags
            .iter()
            .map(|tag| (tag.id, tag.name.clone()))
            .collect::<Vec<_>>(),
        [(peak, "Peak".to_owned()), (late, "Late".to_owned())],
        "in the order of the categories and their tags"
    );
    let categories = store.tag_categories().unwrap();
    assert_eq!(
        categories
            .iter()
            .map(|made| (made.name.clone(), made.tags.len()))
            .collect::<Vec<_>>(),
        [
            ("Genre".to_owned(), 1),
            ("Components".to_owned(), 0),
            ("Situation".to_owned(), 0),
            ("Other".to_owned(), 0),
            ("Moods".to_owned(), 1),
        ]
    );
    assert_eq!(
        store.tracks_with_all_tags(&[peak]).unwrap(),
        vec![file("/a.mp3"), rekordbox("7")]
    );
}

#[test]
fn a_tag_asked_for_twice_still_finds_its_tracks() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = SetStore::open(&directory.path().join("dubplate.db")).unwrap();
    let category = store.tag_categories().unwrap()[0].id;
    let peak = store
        .edit_track_tags(TrackTagEdit::CreateTag {
            category,
            name: "Peak".to_owned(),
            position: 0,
        })
        .unwrap()
        .created_tag
        .unwrap();
    let track = TrackIdentity::File(PathBuf::from("/Music/a.mp3"));
    store
        .edit_track_tags(TrackTagEdit::AddTag {
            tracks: vec![track.clone()],
            tag: peak,
        })
        .unwrap();
    assert_eq!(store.tracks_with_all_tags(&[peak, peak]).unwrap(), [track]);
}

#[test]
fn metadata_for_a_large_library_reads_in_one_call() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = SetStore::open(&directory.path().join("dubplate.db")).unwrap();
    let tracks: Vec<TrackIdentity> = (0..20_000)
        .map(|index| TrackIdentity::File(PathBuf::from(format!("/Music/{index}.mp3"))))
        .collect();
    store
        .edit_track_tags(TrackTagEdit::SetRating {
            track: tracks[19_999].clone(),
            track_path: None,
            rating: 4,
        })
        .unwrap();
    let found = store.track_metadata_many(&tracks).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[&tracks[19_999]].rating, 4);
}
