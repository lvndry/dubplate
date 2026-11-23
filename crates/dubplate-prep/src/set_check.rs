use std::path::PathBuf;

use crate::warnings::warnings_for_slots;
use crate::{
    PrepTrack, ReadyProblem, ReadyReport, SeamStatus, Set, SetEntry, SetTrack, SetWarning,
    TrackReadiness, ready_report,
};

/// The available metadata and file status for a saved set entry.
#[derive(Debug, Clone, Copy, Default)]
pub struct SetCheckTrack<'track> {
    /// Mixing metadata, when the track can be resolved.
    pub track: Option<&'track PrepTrack>,
    /// The audio file is available at the resolved path.
    pub file_found: bool,
    /// Vocals are audible near the start of the track.
    pub vocals_at_start: bool,
    /// Vocals are audible near the end of the track.
    pub vocals_at_end: bool,
}

impl SetCheckTrack<'_> {
    /// Checks the saved cues and confirmation against the available facts.
    fn readiness(self, entry: &SetEntry) -> ReadyReport {
        if self.track.is_none() && !self.file_found {
            return ReadyReport {
                problems: vec![ReadyProblem::FileMissing],
            };
        }
        ready_report(&TrackReadiness {
            file_found: self.file_found,
            grid_confirmed: entry.grid_confirmed,
            has_mix_in_cue: entry.mix_in_ms.is_some(),
            has_mix_out_cue: entry.mix_out_ms.is_some(),
            key_known: self.track.is_some_and(|track| track.key.is_some()),
        })
    }
}

/// One track or backup in a set check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetCheckRow {
    /// The audio file path recorded in the set.
    pub track_path: PathBuf,
    /// The title, falling back to the file stem when metadata is unavailable.
    pub title: String,
    /// Each issue affecting the track's readiness.
    pub ready: ReadyReport,
}

/// The readiness and transition warnings for a prepared set.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SetCheckReport {
    /// The main tracks in playing order.
    pub tracks: Vec<SetCheckRow>,
    /// The backups in their own saved order.
    pub backups: Vec<SetCheckRow>,
    /// Transition warnings followed by track warnings, with positions indexing `tracks`.
    pub warnings: Vec<SetWarning>,
}

impl SetCheckReport {
    /// Whether the main set has tracks and every one is ready to play.
    pub fn is_ready(&self) -> bool {
        !self.tracks.is_empty() && self.tracks.iter().all(|row| row.ready.is_ready())
    }
}

/// Checks a saved set using metadata provided by `lookup`, without performing any I/O.
pub fn check_set<'track>(
    set: &Set,
    mut lookup: impl FnMut(&SetEntry) -> SetCheckTrack<'track>,
) -> SetCheckReport {
    let mut report = SetCheckReport::default();
    let mut warning_tracks = Vec::new();
    for entry in &set.entries {
        let facts = lookup(entry);
        let row = SetCheckRow {
            track_path: entry.track_path.clone(),
            title: facts.track.map_or_else(
                || {
                    entry
                        .track_path
                        .file_stem()
                        .map(|stem| stem.to_string_lossy().into_owned())
                        .unwrap_or_default()
                },
                |track| track.title.clone(),
            ),
            ready: facts.readiness(entry),
        };
        if entry.is_backup {
            report.backups.push(row);
        } else {
            warning_tracks.push(facts.track.map(|track| {
                (
                    SetTrack {
                        track,
                        vocals_at_start: facts.vocals_at_start,
                        vocals_at_end: facts.vocals_at_end,
                        free_into_next: set
                            .seam_after(report.tracks.len())
                            .is_some_and(|seam| seam.status == SeamStatus::Free),
                    },
                    entry.energy_override.or(track.energy),
                )
            }));
            report.tracks.push(row);
        }
    }
    report.warnings = warnings_for_slots(warning_tracks.len(), |index| warning_tracks[index]);
    report
}
