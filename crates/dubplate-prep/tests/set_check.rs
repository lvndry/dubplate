//! Readiness and transition warnings for saved sets.

use std::path::PathBuf;

use dubplate_prep::{
    EqIntent, Key, PrepTrack, ReadyProblem, Seam, SeamStatus, Set, SetCheckTrack, SetEntry, SetId,
    WarningKind, WarningPosition, check_set,
};

fn entry(name: &str) -> SetEntry {
    SetEntry {
        track_path: PathBuf::from(format!("{name}.mp3")),
        grid_confirmed: true,
        mix_in_ms: Some(0),
        mix_out_ms: Some(120_000),
        ..SetEntry::default()
    }
}

fn track(entry: &SetEntry, title: &str, key: &str, bpm: f32, energy: f32) -> PrepTrack {
    PrepTrack {
        track_path: entry.track_path.clone(),
        rekordbox_id: None,
        title: title.to_owned(),
        artist: "Artist".to_owned(),
        key: Key::parse_file_key(key),
        bpm: Some(bpm),
        energy: Some(energy),
    }
}

fn set(entries: Vec<SetEntry>) -> Set {
    Set {
        id: SetId(1),
        name: "Set".to_owned(),
        target_minutes: 30,
        created_at: 0,
        updated_at: 0,
        entries,
        seams: Vec::new(),
    }
}

#[test]
fn transitions_use_actual_neighbours_and_duplicate_positions_include_unresolved_tracks() {
    let a = entry("a");
    let b = entry("b");
    let mut incoming = a.clone();
    incoming.energy_override = Some(4.0);
    let mut set = set(vec![
        entry("unresolved first"),
        a.clone(),
        entry("unresolved middle"),
        b.clone(),
        incoming,
    ]);
    let tracks = [
        track(&a, "Song (Radio Edit)", "8A", 120.0, 9.0),
        track(&b, "Song (Extended Mix)", "3B", 132.0, 8.0),
    ];
    let lookup = |entry: &SetEntry| {
        let track = tracks
            .iter()
            .find(|track| track.track_path == entry.track_path);
        SetCheckTrack {
            track,
            file_found: track.is_some(),
            vocals_at_start: entry.track_path == a.track_path,
            vocals_at_end: entry.track_path == b.track_path,
        }
    };

    let report = check_set(&set, lookup);
    assert_eq!(report.warnings.len(), 6);
    let (seams, tracks) = report.warnings.split_at(4);
    assert!(
        seams
            .iter()
            .all(|warning| warning.position == WarningPosition::Seam { after: 3 })
    );
    assert!(matches!(seams[0].kind, WarningKind::KeyClash { .. }));
    assert!(matches!(seams[1].kind, WarningKind::TempoGap { .. }));
    assert_eq!(seams[2].kind, WarningKind::VocalsOverlap);
    assert_eq!(seams[3].kind, WarningKind::EnergyDrop { by: 4.0 });
    assert_eq!(tracks[0].position, WarningPosition::Track { index: 3 });
    assert_eq!(
        tracks[0].kind,
        WarningKind::SimilarVersion { first_position: 1 }
    );
    assert_eq!(tracks[1].position, WarningPosition::Track { index: 4 });
    assert_eq!(tracks[1].kind, WarningKind::Duplicate { first_position: 1 });

    set.seams.push(Seam {
        from_track_path: b.track_path.clone(),
        to_track_path: a.track_path.clone(),
        overlap_bars: 16,
        eq_intent: EqIntent::None,
        status: SeamStatus::Free,
        note: String::new(),
    });
    assert_eq!(check_set(&set, lookup).warnings, tracks);
}

#[test]
fn main_readiness_uses_saved_grid_confirmation_and_keeps_backups_separate() {
    let main = entry("main");
    let mut missing = entry("missing backup");
    missing.is_backup = true;
    let mut existing = entry("existing backup");
    existing.is_backup = true;
    let set = set(vec![main.clone(), missing.clone(), existing.clone()]);
    let track = track(&main, "Main", "8A", 120.0, 6.0);
    let lookup = |entry: &SetEntry| SetCheckTrack {
        track: (entry.track_path == main.track_path).then_some(&track),
        file_found: entry.track_path != missing.track_path,
        ..SetCheckTrack::default()
    };
    let report = check_set(&set, lookup);
    assert!(report.is_ready());
    assert_eq!(report.tracks[0].title, "Main");
    assert_eq!(report.backups[0].title, "missing backup");
    assert_eq!(
        report.backups[0].ready.problems,
        [ReadyProblem::FileMissing]
    );
    assert_eq!(report.backups[1].title, "existing backup");
    assert_eq!(report.backups[1].ready.problems, [ReadyProblem::KeyUnknown]);
    assert!(report.warnings.is_empty());

    let mut unconfirmed_set = set.clone();
    unconfirmed_set.entries[0].grid_confirmed = false;
    unconfirmed_set.entries[0].mix_out_ms = None;
    let report = check_set(&unconfirmed_set, lookup);
    assert!(!report.is_ready());
    assert_eq!(
        report.tracks[0].ready.problems,
        [ReadyProblem::GridUnconfirmed, ReadyProblem::NoMixOutCue]
    );
}
