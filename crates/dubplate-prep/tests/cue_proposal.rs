//! Proposed mix cues: where a track comes in under the outgoing one and where it hands over.

use dubplate_prep::{ProposalSource, ProposedCue, ProposedCues, propose_mix_cues};
use dubplate_rekordbox::analysis::{
    Beat, BeatGrid, LightingBank, Mood, Phrase, PhraseKind, SongStructure,
};

/// The milliseconds one beat takes at 122 BPM.
const BEAT_SPACING_MS: u32 = 492;

/// A 122 BPM grid of `beat_count` beats, downbeats on every 4th beat, each
/// beat spaced 492 ms apart.
fn grid_of_beats(beat_count: usize) -> BeatGrid {
    let beats = (0..beat_count)
        .map(|beat_index| Beat {
            beat_in_bar: u16::try_from(beat_index % 4 + 1).expect("a beat in bar fits in u16"),
            tempo_hundredths: 12200,
            time_ms: u32::try_from(beat_index).expect("a beat index fits in u32") * BEAT_SPACING_MS,
        })
        .collect();
    BeatGrid { beats }
}

/// The cue that lands on `beat_number` (1-based) of `grid`, from `source`.
fn cue_at_beat(grid: &BeatGrid, beat_number: u32, source: ProposalSource) -> ProposedCue {
    let beat = grid
        .beat(beat_number)
        .expect("the test grid is long enough");
    ProposedCue {
        time_ms: beat.time_ms,
        source,
    }
}

fn phrase(index: u16, kind: PhraseKind, start_beat: u16, end_beat: u16) -> Phrase {
    Phrase {
        index,
        kind,
        raw_kind: index,
        start_beat,
        end_beat,
        fill_start_beat: None,
        lighting_beats: Vec::new(),
    }
}

fn structure_of(end_beat: u16, phrases: Vec<Phrase>) -> SongStructure {
    SongStructure {
        mood: Mood::Low,
        end_beat,
        bank: LightingBank::Default,
        was_masked: false,
        phrases,
    }
}

/// A four-minute track: intro, verse, chorus, verse, then two outros.
fn four_minute_structure() -> SongStructure {
    structure_of(
        444,
        vec![
            phrase(1, PhraseKind::Intro(None), 1, 33),
            phrase(2, PhraseKind::Verse(1), 33, 97),
            phrase(3, PhraseKind::Chorus(None), 97, 161),
            phrase(4, PhraseKind::Verse(2), 161, 257),
            phrase(5, PhraseKind::Chorus(None), 257, 321),
            phrase(6, PhraseKind::Outro(None), 321, 401),
            phrase(7, PhraseKind::Outro(None), 401, 444),
        ],
    )
}

#[test]
fn mix_in_starts_on_the_first_phrase_and_mix_out_on_the_last_outro() {
    let grid = grid_of_beats(500);
    let structure = four_minute_structure();

    let cues = propose_mix_cues(&grid, Some(&structure));

    assert_eq!(
        cues,
        ProposedCues {
            mix_in: Some(cue_at_beat(&grid, 1, ProposalSource::RekordboxPhrases)),
            mix_out: Some(cue_at_beat(&grid, 401, ProposalSource::RekordboxPhrases)),
        }
    );
}

#[test]
fn a_track_without_an_outro_hands_over_128_beats_before_the_end() {
    let grid = grid_of_beats(500);
    let structure = structure_of(
        260,
        vec![
            phrase(1, PhraseKind::Intro(None), 1, 33),
            phrase(2, PhraseKind::Verse(1), 33, 97),
            phrase(3, PhraseKind::Chorus(None), 97, 260),
        ],
    );

    let cues = propose_mix_cues(&grid, Some(&structure));

    // 260 - 128 is beat 132, a second beat, so the handover snaps back to beat 129.
    assert_eq!(
        cues.mix_out,
        Some(cue_at_beat(&grid, 129, ProposalSource::RekordboxPhrases))
    );
}

#[test]
fn a_phrase_starting_off_the_bar_snaps_forward_for_mix_in() {
    let grid = grid_of_beats(500);
    // The intro starts on beat 3, a third beat, not a downbeat.
    let structure = structure_of(101, vec![phrase(1, PhraseKind::Intro(None), 3, 101)]);

    let cues = propose_mix_cues(&grid, Some(&structure));

    assert_eq!(
        cues.mix_in,
        Some(cue_at_beat(&grid, 5, ProposalSource::RekordboxPhrases))
    );
}

#[test]
fn a_handover_landing_off_the_bar_snaps_back_to_the_previous_downbeat() {
    let grid = grid_of_beats(500);
    // No outro: 128 beats before beat 263 lands on beat 135, a fourth beat.
    let structure = structure_of(
        263,
        vec![
            phrase(1, PhraseKind::Intro(None), 1, 33),
            phrase(2, PhraseKind::Verse(1), 33, 263),
        ],
    );

    let cues = propose_mix_cues(&grid, Some(&structure));

    assert_eq!(
        cues.mix_out,
        Some(cue_at_beat(&grid, 133, ProposalSource::RekordboxPhrases))
    );
}

#[test]
fn without_a_structure_the_grid_alone_makes_both_cues() {
    let grid = grid_of_beats(500);

    let cues = propose_mix_cues(&grid, None);

    // 500 - 128 is beat 372, a second beat, so the handover snaps back to beat 369.
    assert_eq!(
        cues,
        ProposedCues {
            mix_in: Some(cue_at_beat(&grid, 1, ProposalSource::BeatGridOnly)),
            mix_out: Some(cue_at_beat(&grid, 369, ProposalSource::BeatGridOnly)),
        }
    );
}

#[test]
fn a_very_short_track_gets_a_mix_in_but_no_mix_out() {
    let grid = grid_of_beats(32);

    let cues = propose_mix_cues(&grid, None);

    assert_eq!(
        cues.mix_in,
        Some(cue_at_beat(&grid, 1, ProposalSource::BeatGridOnly))
    );
    assert_eq!(cues.mix_out, None);
}

#[test]
fn an_empty_grid_proposes_nothing() {
    let grid = grid_of_beats(0);

    let cues = propose_mix_cues(&grid, None);
    let structured_cues = propose_mix_cues(&grid, Some(&four_minute_structure()));

    assert_eq!(
        cues,
        ProposedCues {
            mix_in: None,
            mix_out: None
        }
    );
    assert_eq!(
        structured_cues,
        ProposedCues {
            mix_in: None,
            mix_out: None
        }
    );
}
