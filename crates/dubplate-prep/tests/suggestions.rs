//! Next-track suggestions over a small afro house and amapiano crate.

use std::path::PathBuf;

use dubplate_prep::{EnergyDirection, Key, KeyFit, PrepTrack, suggest_next};

fn track(id: &str, key: &str, bpm: f32, energy: f32) -> PrepTrack {
    PrepTrack {
        track_path: PathBuf::from(format!("/music/{id}.mp3")),
        rekordbox_id: None,
        title: id.to_owned(),
        artist: String::new(),
        key: Key::parse_file_key(key),
        bpm: Some(bpm),
        energy: Some(energy),
    }
}

fn crate_of_tracks() -> Vec<PrepTrack> {
    vec![
        track("same-key", "8A", 122.0, 6.0),
        track("adjacent-up", "9A", 123.0, 7.0),
        track("relative", "8B", 121.0, 6.0),
        track("clash", "3B", 122.0, 6.0),
        track("amapiano", "8A", 113.0, 5.0),
        track("already-in-set", "8A", 122.0, 6.0),
    ]
}

#[test]
fn reads_camelot_and_musical_keys_the_way_rekordbox_writes_them() {
    assert_eq!(Key::parse_file_key("8A"), Key::parse_file_key("Am"));
    assert_eq!(
        Key::parse_file_key("8A").map(|key| key.camelot()),
        Some("8A".to_owned())
    );
}

#[test]
fn suggests_compatible_tracks_and_leaves_out_clashes_far_tempos_and_the_set() {
    let reference = track("playing", "8A", 122.0, 6.0);
    let candidates = crate_of_tracks();

    let suggestions = suggest_next(
        &reference,
        &candidates,
        EnergyDirection::Same,
        |candidate| candidate.title == "already-in-set",
        10,
    );
    let ids: Vec<&str> = suggestions
        .iter()
        .map(|suggestion| suggestion.track.title.as_str())
        .collect();

    assert_eq!(ids.first(), Some(&"same-key"));
    assert!(ids.contains(&"adjacent-up"));
    assert!(ids.contains(&"relative"));
    assert!(!ids.contains(&"clash"));
    assert!(
        !ids.contains(&"amapiano"),
        "a 7% tempo gap is past the default window"
    );
    assert!(!ids.contains(&"already-in-set"));
    assert!(!ids.contains(&"playing"));
}

#[test]
fn lifting_the_energy_puts_the_more_energetic_track_first() {
    let reference = track("playing", "8A", 122.0, 6.0);
    let candidates = vec![
        track("same-energy", "8A", 122.0, 6.0),
        track("lift", "9A", 122.5, 8.0),
    ];

    let suggestions = suggest_next(&reference, &candidates, EnergyDirection::Up, |_| false, 2);

    assert_eq!(suggestions[0].track.title, "lift");
    assert_eq!(
        suggestions[0].reason(),
        "One step on the wheel · +0.4% · energy +2"
    );
}

#[test]
fn the_key_fit_accepts_the_moves_the_suggestions_make() {
    let key = |text: &str| Key::parse_file_key(text).unwrap();
    let fit = KeyFit::new(key("8A"));
    assert_eq!(fit.reference(), key("8A"));
    assert_eq!(fit.key_move(&key("8A")), Some("Same key"));
    assert_eq!(fit.key_move(&key("9A")), Some("One step on the wheel"));
    assert_eq!(fit.key_move(&key("7A")), Some("One step on the wheel"));
    assert_eq!(fit.key_move(&key("C")), Some("Relative"));
    assert_eq!(
        fit.key_move(&key("10A")),
        None,
        "an energy boost is not a suggestion"
    );
    assert_eq!(fit.key_move(&key("3B")), None);
}
