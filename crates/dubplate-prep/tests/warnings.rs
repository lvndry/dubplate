//! Set warnings: what the set does not like, on its seams and its tracks.

use dubplate_prep::{
    Key, PrepTrack, SetEntry, SetWarning, WarningKind, WarningPosition, set_warnings,
};

fn track(rekordbox_id: &str, title: &str, artist: &str, key: Option<&str>) -> PrepTrack {
    track_with_facts(rekordbox_id, title, artist, key, None, None)
}

fn track_with_facts(
    rekordbox_id: &str,
    title: &str,
    artist: &str,
    key: Option<&str>,
    bpm: Option<f32>,
    energy: Option<f32>,
) -> PrepTrack {
    PrepTrack {
        rekordbox_id: rekordbox_id.to_owned(),
        title: title.to_owned(),
        artist: artist.to_owned(),
        key: key.and_then(Key::parse_file_key),
        bpm,
        energy,
    }
}

fn entry(track: &PrepTrack) -> SetEntry<'_> {
    SetEntry {
        track,
        vocals_at_start: false,
        vocals_at_end: false,
        free_into_next: false,
    }
}

fn entry_with_flags(track: &PrepTrack, vocals_at_start: bool, vocals_at_end: bool) -> SetEntry<'_> {
    SetEntry {
        track,
        vocals_at_start,
        vocals_at_end,
        free_into_next: false,
    }
}

fn free_entry(track: &PrepTrack) -> SetEntry<'_> {
    SetEntry {
        track,
        vocals_at_start: false,
        vocals_at_end: false,
        free_into_next: true,
    }
}

#[test]
fn a_clean_afro_house_set_gives_no_warnings() {
    let first_track = track("id-1", "Opening groove", "Sunset Collective", Some("8A"));
    let second_track = track("id-2", "Deep groove", "Sunset Collective", Some("9A"));
    let third_track = track("id-3", "Peak groove", "Sunset Collective", Some("9B"));
    let first_entry = entry(&first_track);
    let second_entry = entry(&second_track);
    let third_entry = entry(&third_track);

    let warnings = set_warnings(&[first_entry, second_entry, third_entry]);

    assert_eq!(warnings, Vec::<SetWarning>::new());
}

#[test]
fn clashing_keys_warn_with_the_camelot_codes() {
    let outgoing_track = track("id-1", "Opening groove", "Sunset Collective", Some("8A"));
    let incoming_track = track("id-2", "Hard groove", "Peak Crew", Some("3B"));
    let outgoing_entry = entry(&outgoing_track);
    let incoming_entry = entry(&incoming_track);

    let warnings = set_warnings(&[outgoing_entry, incoming_entry]);

    assert_eq!(
        warnings,
        vec![SetWarning {
            position: WarningPosition::Seam { after: 0 },
            kind: WarningKind::KeyClash {
                outgoing: Key::parse_file_key("8A").unwrap(),
                incoming: Key::parse_file_key("3B").unwrap(),
            },
        }]
    );
    assert_eq!(warnings[0].kind.label(), "Key clash: 8A to 3B");
}

#[test]
fn a_tempo_gap_beyond_six_percent_warns_and_within_it_does_not() {
    let slow_track = track_with_facts(
        "id-1",
        "Slow groove",
        "Sunset Collective",
        None,
        Some(113.0),
        None,
    );
    let fast_track = track_with_facts("id-2", "Fast groove", "Peak Crew", None, Some(122.0), None);
    let close_track =
        track_with_facts("id-3", "Close groove", "Peak Crew", None, Some(124.0), None);

    let gapped = set_warnings(&[entry(&slow_track), entry(&fast_track)]);
    assert_eq!(
        gapped,
        vec![SetWarning {
            position: WarningPosition::Seam { after: 0 },
            kind: WarningKind::TempoGap {
                percent: 9.0 / 113.0 * 100.0
            },
        }]
    );
    assert_eq!(
        gapped[0].kind.label(),
        "Tempo gap +8.0%, needs a long blend"
    );

    let close = set_warnings(&[entry(&fast_track), entry(&close_track)]);
    assert!(
        close
            .iter()
            .all(|warning| !matches!(warning.kind, WarningKind::TempoGap { .. }))
    );
}

#[test]
fn vocals_overlap_only_when_both_tags_meet() {
    let cases = [
        (true, true, true),
        (true, false, false),
        (false, true, false),
        (false, false, false),
    ];
    for (outgoing_vocals_at_end, incoming_vocals_at_start, should_warn) in cases {
        let outgoing_track = track("id-1", "Out", "Sunset Collective", None);
        let incoming_track = track("id-2", "In", "Peak Crew", None);
        let outgoing_entry = entry_with_flags(&outgoing_track, false, outgoing_vocals_at_end);
        let incoming_entry = entry_with_flags(&incoming_track, incoming_vocals_at_start, false);

        let warnings = set_warnings(&[outgoing_entry, incoming_entry]);
        let warned = warnings
            .iter()
            .any(|warning| matches!(warning.kind, WarningKind::VocalsOverlap));
        assert_eq!(
            warned, should_warn,
            "{outgoing_vocals_at_end} to {incoming_vocals_at_start}"
        );
    }
    assert_eq!(WarningKind::VocalsOverlap.label(), "Vocals overlap");
}

#[test]
fn an_energy_drop_of_three_or_more_warns_and_less_does_not() {
    let high_track = track_with_facts("id-1", "High", "Peak Crew", None, None, Some(8.0));
    let low_track = track_with_facts("id-2", "Low", "Closer Crew", None, None, Some(5.0));
    let near_track = track_with_facts("id-3", "Near", "Closer Crew", None, None, Some(6.0));

    let dropped = set_warnings(&[entry(&high_track), entry(&low_track)]);
    assert_eq!(
        dropped,
        vec![SetWarning {
            position: WarningPosition::Seam { after: 0 },
            kind: WarningKind::EnergyDrop { by: 3.0 },
        }]
    );
    assert_eq!(dropped[0].kind.label(), "Energy drops by 3");

    let near = set_warnings(&[entry(&high_track), entry(&near_track)]);
    assert!(
        near.iter()
            .all(|warning| !matches!(warning.kind, WarningKind::EnergyDrop { .. }))
    );
}

#[test]
fn a_free_seam_silences_every_seam_warning() {
    let outgoing_track = track_with_facts(
        "id-1",
        "Out",
        "Sunset Collective",
        Some("8A"),
        Some(113.0),
        Some(8.0),
    );
    let incoming_track = track_with_facts(
        "id-2",
        "In",
        "Peak Crew",
        Some("3B"),
        Some(122.0),
        Some(5.0),
    );
    let free_outgoing_entry = free_entry(&outgoing_track);
    let incoming_entry = entry_with_flags(&incoming_track, true, false);
    let outgoing_entry = entry_with_flags(&outgoing_track, false, true);

    let free_seam = set_warnings(&[free_outgoing_entry, incoming_entry]);
    assert_eq!(free_seam, Vec::<SetWarning>::new());

    let warned_seam = set_warnings(&[outgoing_entry, incoming_entry]);
    assert_eq!(
        warned_seam
            .iter()
            .map(|warning| warning.kind.label())
            .collect::<Vec<_>>(),
        vec![
            "Key clash: 8A to 3B",
            "Tempo gap +8.0%, needs a long blend",
            "Vocals overlap",
            "Energy drops by 3",
        ]
    );
}

#[test]
fn a_duplicate_track_warns_at_the_earlier_position() {
    let first_track = track("id-1", "Jerusalema", "Nomadic Trio", Some("8A"));
    let later_track = track("id-1", "Jerusalema", "Nomadic Trio", Some("8A"));
    let middle_track = track("id-2", "Between", "Peak Crew", Some("9A"));
    let first_entry = entry(&first_track);
    let middle_entry = entry(&middle_track);
    let later_entry = entry(&later_track);

    let warnings = set_warnings(&[first_entry, middle_entry, later_entry]);

    assert_eq!(
        warnings,
        vec![SetWarning {
            position: WarningPosition::Track { index: 2 },
            kind: WarningKind::Duplicate { first_position: 0 },
        }]
    );
    assert_eq!(warnings[0].kind.label(), "Already in the set at 1");
}

#[test]
fn a_similar_version_warns_at_the_earlier_position() {
    let extended_track = track("id-1", "Jerusalema (Extended Mix)", "Nomadic Trio", None);
    let radio_track = track("id-2", "Jerusalema [Radio Edit]", "Nomadic Trio", None);
    let extended_entry = entry(&extended_track);
    let radio_entry = entry(&radio_track);

    let warnings = set_warnings(&[extended_entry, radio_entry]);

    assert_eq!(
        warnings,
        vec![SetWarning {
            position: WarningPosition::Track { index: 1 },
            kind: WarningKind::SimilarVersion { first_position: 0 },
        }]
    );
    assert_eq!(warnings[0].kind.label(), "Another version is at 1");
}

#[test]
fn a_similar_version_needs_the_same_artist_and_base_title() {
    let reference_track = track("id-1", "Jerusalema (Extended Mix)", "Nomadic Trio", None);
    let cases = [
        ("Jerusalema", "Nomadic Trio", true),
        ("jerusalema [radio edit]", "nomadic trio", true),
        ("Jerusalema II", "Nomadic Trio", false),
        ("Jerusalema (Extended Mix)", "Another Trio", false),
        ("(Extended Mix)", "Nomadic Trio", false),
    ];
    for (candidate_title, candidate_artist, should_warn) in cases {
        let candidate_track = track("id-2", candidate_title, candidate_artist, None);
        let reference_entry = entry(&reference_track);
        let candidate_entry = entry(&candidate_track);

        let warnings = set_warnings(&[reference_entry, candidate_entry]);
        let warned = warnings
            .iter()
            .any(|warning| matches!(warning.kind, WarningKind::SimilarVersion { .. }));
        assert_eq!(
            warned, should_warn,
            "{candidate_title} by {candidate_artist}"
        );
    }
}

#[test]
fn a_duplicate_is_not_also_a_similar_version() {
    let first_track = track("id-1", "Jerusalema (Extended Mix)", "Nomadic Trio", None);
    let later_track = track("id-1", "Jerusalema [Radio Edit]", "Nomadic Trio", None);
    let first_entry = entry(&first_track);
    let later_entry = entry(&later_track);

    let warnings = set_warnings(&[first_entry, later_entry]);

    assert_eq!(
        warnings,
        vec![SetWarning {
            position: WarningPosition::Track { index: 1 },
            kind: WarningKind::Duplicate { first_position: 0 },
        }]
    );
}

#[test]
fn unknown_keys_bpms_and_energies_never_warn() {
    let no_key_outgoing = track("id-1", "Out", "Sunset Collective", None);
    let no_key_incoming = track("id-2", "In", "Peak Crew", None);
    assert_eq!(
        set_warnings(&[entry(&no_key_outgoing), entry(&no_key_incoming)]),
        Vec::<SetWarning>::new()
    );

    let no_bpm_outgoing = track_with_facts("id-1", "Out", "Sunset Collective", None, None, None);
    let bpm_incoming = track_with_facts("id-2", "In", "Peak Crew", None, Some(122.0), None);
    assert_eq!(
        set_warnings(&[entry(&no_bpm_outgoing), entry(&bpm_incoming)]),
        Vec::<SetWarning>::new()
    );

    let no_energy_outgoing = track_with_facts("id-1", "Out", "Sunset Collective", None, None, None);
    let energy_incoming = track_with_facts("id-2", "In", "Peak Crew", None, None, Some(1.0));
    assert_eq!(
        set_warnings(&[entry(&no_energy_outgoing), entry(&energy_incoming)]),
        Vec::<SetWarning>::new()
    );
}

#[test]
fn a_single_entry_set_gives_no_warnings() {
    let only_track = track_with_facts(
        "id-1",
        "Only",
        "Sunset Collective",
        Some("8A"),
        Some(113.0),
        Some(1.0),
    );

    let warnings = set_warnings(&[entry(&only_track)]);

    assert_eq!(warnings, Vec::<SetWarning>::new());
}
