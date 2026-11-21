//! What set preparation knows about each track: its key, tempo and energy for suggestions and
//! seams, and what its file and rekordbox's analysis say about whether it is ready to mix.

use std::path::Path;

use dubplate_prep::{
    Key, PrepTrack, ProposedCues, ReadyReport, TrackReadiness, propose_mix_cues, ready_report,
};
use dubplate_rekordbox::TrackAnalysis;

use crate::collection::{Collection, CollectionTrack, TrackOrigin};
use crate::set::MixCues;

/// The suggestion engine's view of every track, in the collection's order. A track is named by
/// its identity, so tracks from music folders, which have no rekordbox id, stay apart.
pub fn catalogue(collection: &Collection) -> Vec<PrepTrack> {
    collection
        .indexed_tracks()
        .map(|(_, track)| prep_track(track))
        .collect()
}

fn prep_track(entry: &CollectionTrack) -> PrepTrack {
    PrepTrack {
        rekordbox_id: entry.identity(),
        title: entry.row.title.to_string(),
        artist: entry.row.artist.to_string(),
        key: entry.track.key.as_deref().and_then(Key::parse_file_key),
        bpm: entry.track.bpm.map(|bpm| bpm as f32),
        energy: None,
    }
}

/// What reading a set track's file and analysis found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrackCheck {
    /// The audio file is where the library says.
    pub file_found: bool,
    /// rekordbox analysed a beat grid for it.
    pub has_grid: bool,
    /// The mix-in and mix-out Dubplate proposes from the grid and phrases.
    pub proposal: ProposedCues,
}

/// Looks at a track's file and rekordbox's analysis of it. Disk work: run it off the interface
/// thread.
pub fn check_track(entry: &CollectionTrack, share_directory: Option<&Path>) -> TrackCheck {
    let file_found = entry
        .track
        .file_path
        .as_deref()
        .is_some_and(Path::is_file);
    let analysis = match (
        entry.origin,
        entry.track.analysis_path.as_deref(),
        share_directory,
    ) {
        (TrackOrigin::Rekordbox, Some(analysis_path), Some(share_directory))
            if !analysis_path.is_empty() =>
        {
            TrackAnalysis::read(share_directory, analysis_path).ok()
        }
        _ => None,
    };
    let grid = analysis
        .as_ref()
        .and_then(|analysis| analysis.beat_grid.as_ref())
        .filter(|grid| !grid.beats.is_empty());
    let proposal = match (grid, &analysis) {
        (Some(grid), Some(analysis)) => propose_mix_cues(grid, analysis.song_structure.as_ref()),
        _ => ProposedCues {
            mix_in: None,
            mix_out: None,
        },
    };
    TrackCheck {
        file_found,
        has_grid: grid.is_some(),
        proposal,
    }
}

/// Whether a set track is ready to mix. Until the DJ can confirm a grid by ear, a grid
/// rekordbox analysed counts as confirmed. A track not checked yet is reported by what is known
/// without the disk.
pub fn readiness(track: &PrepTrack, check: Option<&TrackCheck>, cues: MixCues) -> ReadyReport {
    ready_report(&TrackReadiness {
        file_found: check.is_none_or(|check| check.file_found),
        grid_confirmed: check.is_some_and(|check| check.has_grid),
        has_mix_in_cue: cues.mix_in_ms.is_some(),
        has_mix_out_cue: cues.mix_out_ms.is_some(),
        key_known: track.key.is_some(),
    })
}

/// The cues a proposal would set, keeping any the DJ already set.
pub fn accept_proposal(current: MixCues, proposal: ProposedCues) -> MixCues {
    MixCues {
        mix_in_ms: current
            .mix_in_ms
            .or(proposal.mix_in.map(|cue| cue.time_ms)),
        mix_out_ms: current
            .mix_out_ms
            .or(proposal.mix_out.map(|cue| cue.time_ms)),
    }
}

/// How one track hands over to the next, in words: the key move as Euphoria names it, and the
/// tempo step.
#[derive(Debug, Clone, PartialEq)]
pub struct SeamSummary {
    /// The key move, such as "One step on the wheel", or `None` when a key is unknown.
    pub key_move: Option<&'static str>,
    /// Whether the keys clash.
    pub key_clash: bool,
    /// The incoming track's tempo against the outgoing one, in percent.
    pub tempo_step_percent: Option<f32>,
}

impl SeamSummary {
    /// The seam between `outgoing` and `incoming`.
    pub fn between(outgoing: &PrepTrack, incoming: &PrepTrack) -> Self {
        let relation = match (outgoing.key, incoming.key) {
            (Some(outgoing_key), Some(incoming_key)) => Some(outgoing_key.relation_to(&incoming_key)),
            _ => None,
        };
        let tempo_step_percent = match (outgoing.bpm, incoming.bpm) {
            (Some(outgoing_bpm), Some(incoming_bpm)) if outgoing_bpm > 0.0 => {
                Some((incoming_bpm - outgoing_bpm) / outgoing_bpm * 100.0)
            }
            _ => None,
        };
        Self {
            key_move: relation.flatten().map(|relation| relation.label()),
            key_clash: matches!(relation, Some(None)),
            tempo_step_percent,
        }
    }

    /// One line for the seam row, such as "One step on the wheel · +1.6%".
    pub fn line(&self) -> String {
        let key = match (self.key_move, self.key_clash) {
            (Some(key_move), _) => key_move.to_owned(),
            (None, true) => "Keys clash".to_owned(),
            (None, false) => "Key unknown".to_owned(),
        };
        match self.tempo_step_percent {
            Some(step) => format!("{key} \u{b7} {step:+.1}%"),
            None => key,
        }
    }
}

#[cfg(test)]
mod tests {
    use dubplate_prep::{ProposalSource, ProposedCue};

    use super::*;

    fn track(key: Option<&str>, bpm: Option<f32>) -> PrepTrack {
        PrepTrack {
            rekordbox_id: "id".to_owned(),
            title: "Title".to_owned(),
            artist: "Artist".to_owned(),
            key: key.and_then(Key::parse_file_key),
            bpm,
            energy: None,
        }
    }

    #[test]
    fn a_seam_names_the_key_move_and_the_tempo_step() {
        let seam = SeamSummary::between(&track(Some("8A"), Some(120.0)), &track(Some("9A"), Some(122.0)));
        assert!(!seam.key_clash);
        assert!(seam.key_move.is_some());
        assert!(seam.line().ends_with("+1.7%"), "{}", seam.line());
        let clash = SeamSummary::between(&track(Some("8A"), Some(120.0)), &track(Some("3B"), None));
        assert!(clash.key_clash);
        assert_eq!(clash.line(), "Keys clash");
        let unknown = SeamSummary::between(&track(None, Some(120.0)), &track(Some("8A"), Some(120.0)));
        assert_eq!(unknown.line(), "Key unknown \u{b7} +0.0%");
    }

    #[test]
    fn a_track_is_ready_with_a_file_a_grid_both_cues_and_a_key() {
        let known = track(Some("8A"), Some(120.0));
        let check = TrackCheck {
            file_found: true,
            has_grid: true,
            proposal: ProposedCues {
                mix_in: None,
                mix_out: None,
            },
        };
        let cues = MixCues {
            mix_in_ms: Some(1),
            mix_out_ms: Some(2),
        };
        assert!(readiness(&known, Some(&check), cues).is_ready());
        assert_eq!(
            readiness(&known, Some(&check), MixCues::default()).badge(),
            "No mix-in cue +1"
        );
        let missing = TrackCheck {
            file_found: false,
            ..check
        };
        assert_eq!(readiness(&known, Some(&missing), cues).badge(), "File missing");
        assert_eq!(
            readiness(&track(None, None), None, cues).badge(),
            "Grid unconfirmed +1"
        );
    }

    #[test]
    fn accepting_a_proposal_fills_only_the_missing_cues() {
        let cue = |time_ms| ProposedCue {
            time_ms,
            source: ProposalSource::RekordboxPhrases,
        };
        let proposal = ProposedCues {
            mix_in: Some(cue(16_000)),
            mix_out: Some(cue(300_000)),
        };
        let kept = MixCues {
            mix_in_ms: Some(8_000),
            mix_out_ms: None,
        };
        assert_eq!(
            accept_proposal(kept, proposal),
            MixCues {
                mix_in_ms: Some(8_000),
                mix_out_ms: Some(300_000)
            }
        );
    }
}
