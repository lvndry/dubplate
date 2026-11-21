use dubplate_rekordbox::analysis::{BeatGrid, PhraseKind, SongStructure};

/// Where a proposed cue comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProposalSource {
    /// The cue rides on a phrase rekordbox analysed into the track.
    RekordboxPhrases,
    /// The cue rides on the beat grid alone, with no phrases to lean on.
    BeatGridOnly,
}

/// One proposed mix cue: where it fires and what it was built from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProposedCue {
    /// When the cue fires, in milliseconds from the start of the track.
    pub time_ms: u32,
    /// What the proposal was built from.
    pub source: ProposalSource,
}

/// The mix-in and mix-out Dubplate proposes for a track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProposedCues {
    /// Where the incoming track starts under the outgoing one, snapped to a
    /// downbeat. None when the grid is empty or nothing sensible can be found.
    pub mix_in: Option<ProposedCue>,
    /// Where the outgoing track starts handing over, snapped to a downbeat.
    /// None when the grid is empty or the handover would come before the
    /// mix-in.
    pub mix_out: Option<ProposedCue>,
}

/// The number of beats between bars.
const BEATS_PER_BAR: u32 = 4;

/// The handover in beats: 32 bars before the end when the structure has no
/// Outro, and the whole fallback for a track with no structure.
const HANDOVER_BEATS_BEFORE_END: u32 = BEATS_PER_BAR * 32;

/// Proposes where to mix a track in and out from its beat grid and, when
/// rekordbox analysed it, its phrases.
pub fn propose_mix_cues(grid: &BeatGrid, structure: Option<&SongStructure>) -> ProposedCues {
    let Some((mix_in_beat, mix_in_cue)) = proposed_mix_in(grid, structure) else {
        return ProposedCues {
            mix_in: None,
            mix_out: None,
        };
    };
    let mix_out = proposed_mix_out(grid, structure, mix_in_beat);
    ProposedCues {
        mix_in: Some(mix_in_cue),
        mix_out,
    }
}

/// The next beat at or after `beat_number` that starts a bar.
fn downbeat_at_or_after(grid: &BeatGrid, beat_number: u32) -> Option<u32> {
    let last_beat = u32::try_from(grid.beats.len()).ok()?;
    (beat_number.max(1)..=last_beat).find(|candidate| {
        grid.beat(*candidate)
            .is_some_and(|found| found.is_downbeat())
    })
}

/// The last beat at or before `beat_number` that starts a bar.
fn downbeat_at_or_before(grid: &BeatGrid, beat_number: u32) -> Option<u32> {
    let last_beat = u32::try_from(grid.beats.len()).ok()?;
    (1..=beat_number.min(last_beat)).rev().find(|candidate| {
        grid.beat(*candidate)
            .is_some_and(|found| found.is_downbeat())
    })
}

/// Where the incoming track starts under the outgoing one. With a song
/// structure it is the first phrase's start; without one, the grid's first
/// downbeat. Always snapped forward to a downbeat. Returns the snapped beat
/// number and the cue.
fn proposed_mix_in(
    grid: &BeatGrid,
    structure: Option<&SongStructure>,
) -> Option<(u32, ProposedCue)> {
    let (start_beat, source) = match structure {
        Some(analysis) => {
            let first_phrase = analysis.phrases.first()?;
            (
                u32::from(first_phrase.start_beat),
                ProposalSource::RekordboxPhrases,
            )
        }
        None => (1, ProposalSource::BeatGridOnly),
    };
    let snapped_beat = downbeat_at_or_after(grid, start_beat)?;
    let beat = grid.beat(snapped_beat)?;
    Some((
        snapped_beat,
        ProposedCue {
            time_ms: beat.time_ms,
            source,
        },
    ))
}

/// Where the outgoing track starts handing over. With a song structure it is
/// the last Outro's start, or 32 bars before the structure's end when there
/// is no Outro; without one, 128 beats before the grid's last beat. Snapped
/// back to a downbeat, and never earlier than the mix-in.
fn proposed_mix_out(
    grid: &BeatGrid,
    structure: Option<&SongStructure>,
    mix_in_beat: u32,
) -> Option<ProposedCue> {
    let (handover_beat, source) = match structure {
        Some(analysis) => {
            let last_outro_beat = analysis
                .phrases
                .iter()
                .rev()
                .find(|phrase| matches!(phrase.kind, PhraseKind::Outro(_)))
                .map(|phrase| u32::from(phrase.start_beat));
            let fallback_beat =
                u32::from(analysis.end_beat).saturating_sub(HANDOVER_BEATS_BEFORE_END);
            (
                last_outro_beat.unwrap_or(fallback_beat),
                ProposalSource::RekordboxPhrases,
            )
        }
        None => (
            u32::try_from(
                grid.beats
                    .len()
                    .checked_sub(usize::try_from(HANDOVER_BEATS_BEFORE_END).ok()?)?,
            )
            .ok()?,
            ProposalSource::BeatGridOnly,
        ),
    };
    let snapped_beat = downbeat_at_or_before(grid, handover_beat)?;
    if snapped_beat <= mix_in_beat {
        return None;
    }
    grid.beat(snapped_beat).map(|beat| ProposedCue {
        time_ms: beat.time_ms,
        source,
    })
}
