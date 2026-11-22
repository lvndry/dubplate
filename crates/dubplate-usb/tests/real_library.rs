//! Exports tracks of the DJ's own rekordbox library to a scratch folder, reading a copy of
//! master.db and rekordbox's analysis files in place, never writing them.

use std::collections::HashMap;
use std::path::PathBuf;

use dubplate_prep::{ExportLibrary, RekordboxCollection, Set, SetEntry, SetId};
use dubplate_rekordbox::device_library::DeviceLibrary;
use dubplate_rekordbox::{DatabaseKey, Library, TrackAnalysis, default_master_db};
use dubplate_usb::{export_to_usb, local_today, playlists_from_set};

/// Exports `DUBPLATE_REAL_EXPORT_TRACKS` (8 by default) full-length tracks rekordbox analysed
/// and whose files are there, those with cues first, as one playlist, to the empty folder
/// `DUBPLATE_REAL_EXPORT_TO`, and reads every file back.
#[test]
#[ignore = "reads the DJ's own rekordbox library; set DUBPLATE_REAL_EXPORT_TO"]
fn exports_tracks_of_the_rekordbox_library() {
    let Some(destination) = std::env::var_os("DUBPLATE_REAL_EXPORT_TO").map(PathBuf::from) else {
        return;
    };
    let count: usize = std::env::var("DUBPLATE_REAL_EXPORT_TRACKS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(8);
    let key = DatabaseKey::from_user_configuration().expect("the rekordbox key");
    let library = Library::open_copy(&default_master_db().unwrap(), &key).expect("master.db");
    let tracks = library.tracks().unwrap();
    let cues = library.cues().unwrap();
    let with_cues: std::collections::HashSet<&str> =
        cues.iter().map(|cue| cue.track_id.as_str()).collect();
    let mut chosen: Vec<_> = tracks
        .iter()
        .filter(|track| {
            track.analysis_path.is_some()
                && track.bpm.is_some()
                && track.length_seconds.is_some_and(|seconds| seconds > 120)
                && track.file_path.as_ref().is_some_and(|path| path.is_file())
        })
        .collect();
    chosen.sort_by_key(|track| !with_cues.contains(track.id.as_str()));
    chosen.truncate(count);
    let set = Set {
        id: SetId(1),
        name: "Dubplate real export".to_owned(),
        target_minutes: 60,
        created_at: 0,
        updated_at: 0,
        entries: chosen
            .iter()
            .map(|track| SetEntry {
                track_path: track.file_path.clone().unwrap(),
                rekordbox_id: Some(track.id.clone()),
                mix_in_ms: Some(1_000),
                ..SetEntry::default()
            })
            .collect(),
        seams: Vec::new(),
    };
    let relinks = HashMap::new();
    let export_library = ExportLibrary::new(
        Some(RekordboxCollection {
            tracks: &tracks,
            cues: &cues,
            share_directory: library.share_directory(),
        }),
        &[],
        &relinks,
    );
    let (playlists, problems) = playlists_from_set(&export_library, &set, &mut |_| None);
    assert!(problems.is_empty(), "{problems:?}");
    let report = export_to_usb(&destination, &playlists, local_today(), &mut |_| {})
        .expect("the export succeeds");
    eprintln!("{report:#?}");

    let bytes = std::fs::read(destination.join("PIONEER/rekordbox/export.pdb")).unwrap();
    let device = DeviceLibrary::parse_export_pdb(&bytes).unwrap();
    assert_eq!(device.tracks.len(), chosen.len());
    let mut written_cues = 0;
    for track in &device.tracks {
        let analysis = TrackAnalysis::read(&destination, &track.analyze_path).unwrap();
        assert_eq!(
            analysis.audio_path.as_deref(),
            Some(track.file_path.as_str())
        );
        assert!(analysis.beat_grid.is_some(), "{} has no grid", track.title);
        assert!(
            analysis.waveforms.three_band_detail.is_some()
                || analysis.waveforms.color_detail.is_some()
        );
        written_cues += analysis.cues.len();
        assert!(
            destination
                .join(track.file_path.trim_start_matches('/'))
                .is_file()
        );
    }
    eprintln!(
        "{} tracks, {written_cues} cues written",
        device.tracks.len()
    );
}
