//! What set preparation knows about each track: its key, tempo and energy for suggestions and
//! seams, and what its file and rekordbox's analysis say about whether it is ready to mix.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use dubplate_prep::{
    Key, PrepTrack, ProposedCues, ReadyReport, SetEntry, TrackReadiness, propose_mix_cues,
    ready_report,
};
use dubplate_rekordbox::TrackAnalysis;
use dubplate_rekordbox::analysis::{Beat, BeatGrid, SongStructure};

use crate::collection::{Collection, CollectionTrack, TrackOrigin};

/// The suggestion engine's view of every track that has an audio file, found by its file.
#[derive(Debug, Clone, Default)]
pub struct Catalogue {
    /// The tracks, in the collection's order.
    pub tracks: Vec<PrepTrack>,
    by_path: HashMap<PathBuf, usize>,
}

impl Catalogue {
    /// The catalogue of `collection`. Disk-free, but it touches every track: build it off the
    /// interface thread with the collection.
    pub fn build(collection: &Collection) -> Self {
        let tracks: Vec<PrepTrack> = collection
            .indexed_tracks()
            .filter_map(|(_, track)| prep_track(track))
            .collect();
        let by_path = tracks
            .iter()
            .enumerate()
            .map(|(position, track)| (track.track_path.clone(), position))
            .collect();
        Self { tracks, by_path }
    }

    /// The track whose audio file is `path`.
    pub fn get(&self, path: &Path) -> Option<&PrepTrack> {
        self.tracks.get(*self.by_path.get(path)?)
    }

    /// Gives the track whose audio file is `path` its energy, from 1 to 10.
    pub fn set_energy(&mut self, path: &Path, rating: u8) {
        if let Some(track) = self
            .by_path
            .get(path)
            .and_then(|position| self.tracks.get_mut(*position))
        {
            track.energy = Some(f32::from(rating));
        }
    }
}

/// A library track as the suggestion engine sees it, from what rekordbox or the file's tags
/// say. Tracks without an audio file have nothing to mix and are left out.
fn prep_track(entry: &CollectionTrack) -> Option<PrepTrack> {
    Some(PrepTrack {
        track_path: entry.track.file_path.clone()?,
        rekordbox_id: (entry.origin == TrackOrigin::Rekordbox).then(|| entry.track.id.clone()),
        title: entry.row.title.to_string(),
        artist: entry.row.artist.to_string(),
        key: entry.track.key.as_deref().and_then(Key::parse_file_key),
        bpm: entry.track.bpm.map(|bpm| bpm as f32),
        energy: None,
    })
}

/// Beats in a bar.
const BEATS_PER_BAR: i32 = 4;

/// The DJ's fix of rekordbox's beat grid for one track, kept in the set store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GridCorrection {
    /// How far every beat moves, in milliseconds, later when positive.
    pub offset_ms: i32,
    /// How many beats the bars move, later when positive.
    pub downbeat_shift: i32,
}

impl GridCorrection {
    /// The correction a set entry keeps.
    pub fn of(entry: &SetEntry) -> Self {
        Self {
            offset_ms: entry.grid_offset_ms,
            downbeat_shift: entry.downbeat_shift,
        }
    }

    /// rekordbox's grid with this correction: every beat moved by the offset, beats that would
    /// fall before the start left out, and the bars counted from the shifted downbeat.
    pub fn apply(self, grid: &BeatGrid) -> BeatGrid {
        let beats = grid
            .beats
            .iter()
            .filter_map(|beat| {
                let time_ms = i64::from(beat.time_ms) + i64::from(self.offset_ms);
                let in_bar = (i32::from(beat.beat_in_bar) - 1 - self.downbeat_shift)
                    .rem_euclid(BEATS_PER_BAR)
                    + 1;
                Some(Beat {
                    time_ms: u32::try_from(time_ms).ok()?,
                    beat_in_bar: u16::try_from(in_bar).ok()?,
                    ..*beat
                })
            })
            .collect();
        BeatGrid { beats }
    }
}

/// What a set track's analysis holds for checking it: its grid, its phrases and its
/// waveforms. Read off the interface thread.
#[derive(Debug, Clone)]
pub struct TrackDetail {
    /// rekordbox's grid, before the DJ's correction.
    pub grid: Option<BeatGrid>,
    /// rekordbox's phrases.
    pub structure: Option<SongStructure>,
    /// The whole track's waveform.
    pub overview: Option<crate::waveform::OverviewWaveform>,
    /// The waveform at the lanes' zoom, for judging the grid against the music.
    pub zoomed: Option<std::sync::Arc<crate::waveform::ZoomedWaveform>>,
}

/// Reads a track's analysis for the track check. Disk work: run it off the interface thread.
pub fn track_detail(entry: &CollectionTrack, share_directory: Option<&Path>) -> TrackDetail {
    let analysis = read_analysis(entry, share_directory);
    TrackDetail {
        grid: analysis
            .as_ref()
            .and_then(|analysis| analysis.beat_grid.clone()),
        structure: analysis
            .as_ref()
            .and_then(|analysis| analysis.song_structure.clone()),
        overview: analysis.as_ref().and_then(|analysis| {
            crate::waveform::OverviewWaveform::from_waveforms(&analysis.waveforms)
        }),
        zoomed: analysis
            .as_ref()
            .and_then(|analysis| {
                crate::waveform::ZoomedWaveform::from_waveforms(
                    &analysis.waveforms,
                    crate::waveform::Zoom::DEFAULT,
                )
            })
            .map(std::sync::Arc::new),
    }
}

/// rekordbox's analysis of a track from the collection, when it has one.
fn read_analysis(entry: &CollectionTrack, share_directory: Option<&Path>) -> Option<TrackAnalysis> {
    match (
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
    }
}

/// The downbeat nearest to `seconds` on `grid`: where I and O put a mix cue.
pub fn nearest_bar_ms(grid: &BeatGrid, seconds: f64) -> Option<u32> {
    let target = seconds * 1000.0;
    grid.beats
        .iter()
        .filter(|beat| beat.beat_in_bar == 1)
        .min_by(|first, second| {
            (f64::from(first.time_ms) - target)
                .abs()
                .total_cmp(&(f64::from(second.time_ms) - target).abs())
        })
        .map(|beat| beat.time_ms)
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

impl TrackCheck {
    /// A set track whose file the library no longer lists: only whether the file is there.
    pub fn not_in_library(path: &Path) -> Self {
        Self {
            file_found: path.is_file(),
            has_grid: false,
            proposal: ProposedCues {
                mix_in: None,
                mix_out: None,
            },
        }
    }
}

/// Looks at a track's file and rekordbox's analysis of it. Disk work: run it off the interface
/// thread.
pub fn check_track(
    entry: &CollectionTrack,
    share_directory: Option<&Path>,
    correction: GridCorrection,
) -> TrackCheck {
    let file_found = entry.track.file_path.as_deref().is_some_and(Path::is_file);
    let analysis = read_analysis(entry, share_directory);
    let grid = analysis
        .as_ref()
        .and_then(|analysis| analysis.beat_grid.as_ref())
        .filter(|grid| !grid.beats.is_empty())
        .map(|grid| correction.apply(grid));
    let proposal = match (&grid, &analysis) {
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

/// Whether a set track is ready to mix: its grid confirmed by the DJ, both mix cues, a known
/// key and its file. A track not checked yet is reported by what is known without the disk.
pub fn readiness(track: &PrepTrack, check: Option<&TrackCheck>, entry: &SetEntry) -> ReadyReport {
    ready_report(&TrackReadiness {
        file_found: check.is_none_or(|check| check.file_found),
        grid_confirmed: entry.grid_confirmed,
        has_mix_in_cue: entry.mix_in_ms.is_some(),
        has_mix_out_cue: entry.mix_out_ms.is_some(),
        key_known: track.key.is_some(),
    })
}

/// The mix-in and mix-out a proposal would leave the entry with, keeping any cue already set.
pub fn accept_proposal(entry: &SetEntry, proposal: ProposedCues) -> (Option<u32>, Option<u32>) {
    (
        entry.mix_in_ms.or(proposal.mix_in.map(|cue| cue.time_ms)),
        entry.mix_out_ms.or(proposal.mix_out.map(|cue| cue.time_ms)),
    )
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
            (Some(outgoing_key), Some(incoming_key)) => {
                Some(outgoing_key.relation_to(&incoming_key))
            }
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

    /// One line for the seam row, such as "One step on the wheel · +1.6%", leaving out the key
    /// or the tempo when a warning beside it already says so.
    pub fn line(&self, key_warned: bool, tempo_warned: bool) -> String {
        let key = match (self.key_move, self.key_clash) {
            (Some(key_move), _) => Some(key_move.to_owned()),
            (None, true) if key_warned => None,
            (None, true) => Some("Keys clash".to_owned()),
            (None, false) => Some("Key unknown".to_owned()),
        };
        let tempo = self
            .tempo_step_percent
            .filter(|_| !tempo_warned)
            .map(|step| format!("{step:+.1}%"));
        [key, tempo]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" \u{b7} ")
    }
}

#[cfg(test)]
mod tests {
    use dubplate_prep::{ProposalSource, ProposedCue};

    use super::*;

    fn track(key: Option<&str>, bpm: Option<f32>) -> PrepTrack {
        PrepTrack {
            track_path: PathBuf::from("/Music/Track.mp3"),
            rekordbox_id: None,
            title: "Title".to_owned(),
            artist: "Artist".to_owned(),
            key: key.and_then(Key::parse_file_key),
            bpm,
            energy: None,
        }
    }

    #[test]
    fn a_seam_names_the_key_move_and_the_tempo_step() {
        let seam = SeamSummary::between(
            &track(Some("8A"), Some(120.0)),
            &track(Some("9A"), Some(122.0)),
        );
        assert!(!seam.key_clash);
        assert!(seam.key_move.is_some());
        assert!(
            seam.line(false, false).ends_with("+1.7%"),
            "{}",
            seam.line(false, false)
        );
        let clash = SeamSummary::between(&track(Some("8A"), Some(120.0)), &track(Some("3B"), None));
        assert!(clash.key_clash);
        assert_eq!(clash.line(false, false), "Keys clash");
        let unknown =
            SeamSummary::between(&track(None, Some(120.0)), &track(Some("8A"), Some(120.0)));
        assert_eq!(unknown.line(false, false), "Key unknown \u{b7} +0.0%");
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
        let cues = SetEntry {
            mix_in_ms: Some(1),
            mix_out_ms: Some(2),
            grid_confirmed: true,
            ..SetEntry::default()
        };
        assert!(readiness(&known, Some(&check), &cues).is_ready());
        assert_eq!(
            readiness(&known, Some(&check), &SetEntry::default()).badge(),
            "Grid unconfirmed +2"
        );
        let unconfirmed = SetEntry {
            grid_confirmed: false,
            ..cues.clone()
        };
        assert_eq!(
            readiness(&known, Some(&check), &unconfirmed).badge(),
            "Grid unconfirmed",
            "a grid rekordbox analysed still waits for the DJ to confirm it"
        );
        let missing = TrackCheck {
            file_found: false,
            ..check
        };
        assert_eq!(
            readiness(&known, Some(&missing), &cues).badge(),
            "File missing"
        );
        assert_eq!(
            readiness(&track(None, None), None, &cues).badge(),
            "Key unknown"
        );
    }

    fn beat(time_ms: u32, beat_in_bar: u16) -> Beat {
        Beat {
            beat_in_bar,
            tempo_hundredths: 12_000,
            time_ms,
        }
    }

    #[test]
    fn a_correction_moves_the_beats_and_the_bars() {
        let grid = BeatGrid {
            beats: vec![
                beat(0, 1),
                beat(500, 2),
                beat(1000, 3),
                beat(1500, 4),
                beat(2000, 1),
            ],
        };
        let later = GridCorrection {
            offset_ms: 20,
            downbeat_shift: 1,
        }
        .apply(&grid);
        let shown: Vec<(u32, u16)> = later
            .beats
            .iter()
            .map(|beat| (beat.time_ms, beat.beat_in_bar))
            .collect();
        assert_eq!(shown, [(20, 4), (520, 1), (1020, 2), (1520, 3), (2020, 4)]);
        let earlier = GridCorrection {
            offset_ms: -10,
            downbeat_shift: -1,
        }
        .apply(&grid);
        assert_eq!(
            earlier.beats.len(),
            4,
            "a beat moved before the start is left out"
        );
        assert_eq!(
            (earlier.beats[0].time_ms, earlier.beats[0].beat_in_bar),
            (490, 3)
        );
        assert_eq!(nearest_bar_ms(&later, 1.9), Some(520));
        assert_eq!(nearest_bar_ms(&later, 0.1), Some(520));
        assert_eq!(nearest_bar_ms(&BeatGrid { beats: Vec::new() }, 1.0), None);
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
        let kept = SetEntry {
            mix_in_ms: Some(8_000),
            ..SetEntry::default()
        };
        assert_eq!(
            accept_proposal(&kept, proposal),
            (Some(8_000), Some(300_000))
        );
    }
}
