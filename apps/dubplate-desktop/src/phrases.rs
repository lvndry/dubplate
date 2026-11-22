//! A track's phrases as the deck overview shows them: where each one starts and ends in
//! seconds, and which section of the track it is, from rekordbox's phrase analysis.

use dubplate_rekordbox::analysis::{BeatGrid, PhraseKind, SongStructure};
use gpui::SharedString;

/// The part of a track a phrase belongs to, which decides its colour on the overview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    /// Intro: where a mix-in usually goes.
    Intro,
    /// Verse or up: the track building.
    Build,
    /// Chorus: the drop.
    Drop,
    /// Down or bridge: the breakdown.
    Breakdown,
    /// Outro: where a mix-out usually goes.
    Outro,
    /// A phrase Dubplate cannot name.
    Other,
}

impl Section {
    fn of(kind: PhraseKind) -> Self {
        match kind {
            PhraseKind::Intro(_) => Self::Intro,
            PhraseKind::Up(_) | PhraseKind::Verse(_) => Self::Build,
            PhraseKind::Chorus(_) => Self::Drop,
            PhraseKind::Down | PhraseKind::Bridge => Self::Breakdown,
            PhraseKind::Outro(_) => Self::Outro,
            PhraseKind::Unknown(_) => Self::Other,
        }
    }
}

/// A phrase on the overview.
#[derive(Debug, Clone, PartialEq)]
pub struct PhraseSpan {
    /// Where it starts, in seconds.
    pub start_seconds: f64,
    /// Where the next phrase starts, in seconds.
    pub end_seconds: f64,
    /// Which part of the track it is.
    pub section: Section,
    /// Its name, such as `Chorus 2`.
    pub name: SharedString,
}

/// The phrases of `structure` placed in time on `grid`, rekordbox's own grid, which the
/// phrases count their beats along. Phrases that fall outside the grid are left out.
pub fn phrase_spans(structure: &SongStructure, grid: &BeatGrid) -> Vec<PhraseSpan> {
    let seconds_of_beat = |beat: u16| {
        let index = usize::from(beat).checked_sub(1)?;
        grid.beats
            .get(index)
            .map(|found| f64::from(found.time_ms) / 1000.0)
    };
    structure
        .phrases
        .iter()
        .filter_map(|phrase| {
            let start_seconds = seconds_of_beat(phrase.start_beat)?;
            let end_seconds = seconds_of_beat(phrase.end_beat).or_else(|| {
                grid.beats
                    .last()
                    .map(|beat| f64::from(beat.time_ms) / 1000.0)
            })?;
            (end_seconds > start_seconds).then(|| PhraseSpan {
                start_seconds,
                end_seconds,
                section: Section::of(phrase.kind),
                name: phrase.kind.to_string().into(),
            })
        })
        .collect()
}

/// The phrase playing at `seconds`, if any.
pub fn phrase_at(phrases: &[PhraseSpan], seconds: f64) -> Option<&PhraseSpan> {
    phrases
        .iter()
        .find(|phrase| (phrase.start_seconds..phrase.end_seconds).contains(&seconds))
}

#[cfg(test)]
mod tests {
    use dubplate_rekordbox::analysis::{Beat, LightingBank, Mood, Phrase};

    use super::*;

    fn grid() -> BeatGrid {
        BeatGrid {
            beats: (0..64)
                .map(|index| Beat {
                    beat_in_bar: (index % 4 + 1) as u16,
                    tempo_hundredths: 12000,
                    time_ms: index * 500,
                })
                .collect(),
        }
    }

    fn phrase(index: u16, kind: PhraseKind, start_beat: u16, end_beat: u16) -> Phrase {
        Phrase {
            index,
            kind,
            raw_kind: 0,
            start_beat,
            end_beat,
            fill_start_beat: None,
            lighting_beats: Vec::new(),
        }
    }

    #[test]
    fn phrases_are_placed_on_the_grid_and_named_by_section() {
        let structure = SongStructure {
            mood: Mood::High,
            end_beat: 64,
            bank: LightingBank::Default,
            was_masked: false,
            phrases: vec![
                phrase(1, PhraseKind::Intro(Some(1)), 1, 17),
                phrase(2, PhraseKind::Chorus(Some(1)), 17, 49),
                phrase(3, PhraseKind::Outro(None), 49, 64),
                phrase(4, PhraseKind::Down, 90, 99),
            ],
        };
        let spans = phrase_spans(&structure, &grid());
        assert_eq!(spans.len(), 3, "a phrase past the grid is left out");
        assert_eq!(spans[0].start_seconds, 0.0);
        assert_eq!(spans[1].start_seconds, 8.0);
        assert_eq!(spans[1].section, Section::Drop);
        assert_eq!(spans[1].name.as_ref(), "Chorus 1");
        assert_eq!(spans[2].section, Section::Outro);
        assert_eq!(
            phrase_at(&spans, 9.0).map(|phrase| phrase.name.as_ref()),
            Some("Chorus 1")
        );
        assert_eq!(phrase_at(&spans, 40.0), None);
    }
}
