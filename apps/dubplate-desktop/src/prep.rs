//! What set preparation knows about each track: its key, tempo and energy for suggestions and
//! seams, and what its file and its analysis (rekordbox's, or Dubplate's for a track rekordbox
//! never analysed) say about whether it is ready to mix.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use dubplate_prep::{
    AudioAnalysis, GridCorrection, Key, PrepTrack, ProposedCues, ReadyReport, SetEntry,
    TrackReadiness, propose_mix_cues, ready_report,
};
use dubplate_rekordbox::TrackAnalysis;
use dubplate_rekordbox::analysis::{BeatGrid, SongStructure};

use crate::analysis::preferred;
use crate::collection::{Collection, CollectionTrack, TrackOrigin};
use crate::deck::GridSource;

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

    /// Gives the track whose audio file is `path` what Dubplate's analysis found: its energy,
    /// and its tempo and key where they are the ones the track shows (see [`preferred`]).
    pub fn set_analysis(&mut self, path: &Path, analysis: &AudioAnalysis) {
        let Some(track) = self
            .by_path
            .get(path)
            .and_then(|position| self.tracks.get_mut(*position))
        else {
            return;
        };
        let origin = match track.rekordbox_id {
            Some(_) => TrackOrigin::Rekordbox,
            None => TrackOrigin::Folder,
        };
        track.bpm = preferred(origin, track.bpm, analysis.bpm);
        track.key = preferred(origin, track.key, analysis.key);
        if let Some(energy) = &analysis.energy {
            track.energy = Some(f32::from(energy.rating));
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

/// What a set track's analysis holds for checking it: its grid, its phrases and its
/// waveforms. Read off the interface thread.
#[derive(Debug, Clone)]
pub struct TrackDetail {
    /// The track's grid, rekordbox's or Dubplate's, before the DJ's correction.
    pub grid: Option<BeatGrid>,
    /// rekordbox's phrases.
    pub structure: Option<SongStructure>,
    /// The whole track's waveform.
    pub overview: Option<crate::waveform::OverviewWaveform>,
    /// The waveform at the lanes' zoom, for judging the grid against the music.
    pub zoomed: Option<std::sync::Arc<crate::waveform::ZoomedWaveform>>,
}

/// Reads a track's analysis for the track check, taking the grid of Dubplate's `analyses` when
/// rekordbox has none. Disk work: run it off the interface thread.
pub fn track_detail(
    entry: &CollectionTrack,
    share_directory: Option<&Path>,
    analysed: Option<&AudioAnalysis>,
) -> TrackDetail {
    let analysis = read_analysis(entry, share_directory);
    TrackDetail {
        grid: chosen_grid(analysis.as_ref(), analysed).map(|(grid, _)| grid),
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

/// The beat grid a track is mixed on, with where it comes from: rekordbox's when its analysis
/// has one, otherwise the grid of Dubplate's analysis. A grid without beats counts as none.
pub fn chosen_grid(
    rekordbox: Option<&TrackAnalysis>,
    analysed: Option<&AudioAnalysis>,
) -> Option<(BeatGrid, GridSource)> {
    let has_beats = |grid: &BeatGrid| !grid.beats.is_empty();
    let from_rekordbox = rekordbox
        .and_then(|analysis| analysis.beat_grid.clone())
        .filter(has_beats)
        .map(|grid| (grid, GridSource::Rekordbox));
    from_rekordbox.or_else(|| {
        analysed
            .and_then(AudioAnalysis::rekordbox_grid)
            .filter(has_beats)
            .map(|grid| (grid, GridSource::Analysis))
    })
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
    /// It has a beat grid, rekordbox's or Dubplate's.
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

/// Looks at a track's file and its analysis: rekordbox's, or for its grid Dubplate's
/// `analysed` one when rekordbox has none. Disk work: run it off the interface thread.
pub fn check_track(
    entry: &CollectionTrack,
    share_directory: Option<&Path>,
    correction: GridCorrection,
    analysed: Option<&AudioAnalysis>,
) -> TrackCheck {
    let file_found = entry.track.file_path.as_deref().is_some_and(Path::is_file);
    let analysis = read_analysis(entry, share_directory);
    let grid = chosen_grid(analysis.as_ref(), analysed).map(|(grid, _)| correction.apply(&grid));
    let structure = analysis
        .as_ref()
        .and_then(|analysis| analysis.song_structure.as_ref());
    let proposal = match &grid {
        Some(grid) => propose_mix_cues(grid, structure),
        None => ProposedCues {
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
    use dubplate_rekordbox::analysis::Beat;

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

    fn analysed(bpm: f32) -> AudioAnalysis {
        AudioAnalysis {
            duration_seconds: 10.0,
            bpm: Some(bpm),
            beat_grid: Some(dubplate_prep::AnalysedGrid {
                bpm: f64::from(bpm),
                first_downbeat_seconds: 0.25,
            }),
            key: Key::parse_file_key("4A"),
            energy: Some(dubplate_prep::TrackEnergy {
                rating: 6,
                score: 0.5,
                curve: Vec::new(),
            }),
        }
    }

    fn rekordbox_analysis(beats: Vec<Beat>) -> TrackAnalysis {
        let mut analysis = TrackAnalysis::from_files(Vec::new());
        analysis.beat_grid = Some(BeatGrid { beats });
        analysis
    }

    #[test]
    fn rekordbox_grid_comes_first_and_the_analysis_fills_in() {
        let beat = Beat {
            beat_in_bar: 1,
            tempo_hundredths: 12_400,
            time_ms: 100,
        };
        let rekordbox = rekordbox_analysis(vec![beat]);
        let analysis = analysed(120.0);
        let (grid, source) = chosen_grid(Some(&rekordbox), Some(&analysis)).unwrap();
        assert_eq!((grid.beats, source), (vec![beat], GridSource::Rekordbox));
        let (grid, source) = chosen_grid(None, Some(&analysis)).unwrap();
        assert_eq!(source, GridSource::Analysis);
        assert_eq!(
            grid.beats.len(),
            20,
            "every beat of the 10 seconds at 120 BPM"
        );
        assert_eq!(grid.first_downbeat().map(|beat| beat.time_ms), Some(250));
        let (_, source) = chosen_grid(Some(&rekordbox_analysis(Vec::new())), Some(&analysis))
            .expect("an empty rekordbox grid counts as none");
        assert_eq!(source, GridSource::Analysis);
        assert!(chosen_grid(None, None).is_none());
    }

    #[test]
    fn a_folder_track_takes_the_analysed_tempo_and_key_and_a_rekordbox_track_keeps_its_own() {
        let mut folder = track(Some("8A"), Some(118.0));
        folder.track_path = PathBuf::from("/Music/folder.mp3");
        let mut rekordbox = track(Some("8A"), Some(118.0));
        rekordbox.track_path = PathBuf::from("/Music/rekordbox.mp3");
        rekordbox.rekordbox_id = Some("1".to_owned());
        let mut catalogue = Catalogue {
            by_path: [
                (folder.track_path.clone(), 0),
                (rekordbox.track_path.clone(), 1),
            ]
            .into_iter()
            .collect(),
            tracks: vec![folder.clone(), rekordbox.clone()],
        };
        let analysis = analysed(113.0);
        catalogue.set_analysis(&folder.track_path, &analysis);
        catalogue.set_analysis(&rekordbox.track_path, &analysis);
        let folder = catalogue.get(&folder.track_path).unwrap();
        assert_eq!(
            (folder.bpm, folder.key, folder.energy),
            (Some(113.0), Key::parse_file_key("4A"), Some(6.0))
        );
        let rekordbox = catalogue.get(&rekordbox.track_path).unwrap();
        assert_eq!(
            (rekordbox.bpm, rekordbox.key, rekordbox.energy),
            (Some(118.0), Key::parse_file_key("8A"), Some(6.0))
        );
    }

    #[test]
    fn mix_cues_land_on_the_nearest_downbeat() {
        let beat = |time_ms, beat_in_bar| Beat {
            beat_in_bar,
            tempo_hundredths: 12_000,
            time_ms,
        };
        let grid = BeatGrid {
            beats: vec![
                beat(20, 4),
                beat(520, 1),
                beat(1020, 2),
                beat(1520, 3),
                beat(2020, 4),
            ],
        };
        assert_eq!(nearest_bar_ms(&grid, 1.9), Some(520));
        assert_eq!(nearest_bar_ms(&grid, 0.1), Some(520));
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
