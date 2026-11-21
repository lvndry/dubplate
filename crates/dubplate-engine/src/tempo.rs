use dubplate_fx::{MAX_TEMPO_BPM, MIN_TEMPO_BPM};

use crate::deck::DeckId;
use crate::error::{EngineError, check_range};
use crate::track::MAX_TRACK_SECONDS;

/// How much louder another deck must be before it takes over as the automatic tempo master,
/// so two decks at the same level do not swap on every block. About 3 dB.
const TAKEOVER_RATIO: f32 = 1.41;

/// A track's beat grid at its own tempo, as rekordbox analyses it: the BPM and where the first
/// beat falls.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BeatGrid {
    bpm: f64,
    first_beat_seconds: f64,
}

impl BeatGrid {
    /// A grid of `bpm` beats per minute whose first beat falls `first_beat_seconds` into the
    /// track. The BPM must be one effects can follow, from [`MIN_TEMPO_BPM`] to
    /// [`MAX_TEMPO_BPM`].
    pub fn new(bpm: f64, first_beat_seconds: f64) -> Result<Self, EngineError> {
        check_range("BPM", bpm, MIN_TEMPO_BPM, MAX_TEMPO_BPM)?;
        let longest = MAX_TRACK_SECONDS as f64;
        check_range("first beat", first_beat_seconds, -longest, longest)?;
        Ok(Self {
            bpm,
            first_beat_seconds,
        })
    }

    /// The track's own tempo in beats per minute.
    pub fn bpm(self) -> f64 {
        self.bpm
    }

    /// Where the first beat falls, in seconds from the start of the track.
    pub fn first_beat_seconds(self) -> f64 {
        self.first_beat_seconds
    }

    /// Beats from the first beat to `seconds` into the track.
    pub fn beat_at(self, seconds: f64) -> f64 {
        (seconds - self.first_beat_seconds) * self.bpm / 60.0
    }
}

/// The tempo and musical position of a block of audio, for processors that follow the beat.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BeatClock {
    /// The tempo as it sounds, in beats per minute: the grid's BPM times the deck's speed.
    pub bpm: f64,
    /// Beats from the first beat of the grid to the block's first frame.
    pub beat_position: f64,
}

/// Which deck's tempo effects on the master follow.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum MasterTempo {
    /// The playing deck with a beat grid that reaches the master loudest, through its trim,
    /// channel fader and the crossfader. Another deck takes over only once it is clearly
    /// louder, so a blend at equal levels keeps one tempo master.
    #[default]
    LouderDeck,
    /// Always this deck, like the tempo master button on a CDJ.
    Deck(DeckId),
}

/// The audio thread's choice of tempo master deck.
#[derive(Debug, Default)]
pub(crate) struct TempoMaster {
    source: MasterTempo,
    current: Option<DeckId>,
}

/// What the tempo master choice needs to know about a deck.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DeckLevel {
    pub(crate) deck: DeckId,
    pub(crate) has_beat_grid: bool,
    /// The deck's gain to the master while it plays, 0 when it is stopped.
    pub(crate) level: f32,
}

impl TempoMaster {
    pub(crate) fn set_source(&mut self, source: MasterTempo) {
        self.source = source;
    }

    pub(crate) fn current(&self) -> Option<DeckId> {
        self.current
    }

    /// Picks the tempo master for the next block. With no deck playing, the last master stays,
    /// so echo trails keep their timing after the music stops.
    pub(crate) fn update(&mut self, decks: &[DeckLevel]) -> Option<DeckId> {
        match self.source {
            MasterTempo::Deck(deck) => self.current = Some(deck),
            MasterTempo::LouderDeck => {
                let candidates = decks
                    .iter()
                    .filter(|candidate| candidate.has_beat_grid && candidate.level > 0.0);
                let loudest = candidates
                    .clone()
                    .max_by(|first, second| first.level.total_cmp(&second.level));
                let current = candidates
                    .clone()
                    .find(|candidate| Some(candidate.deck) == self.current);
                self.current = match (current, loudest) {
                    (Some(current), Some(loudest))
                        if loudest.level <= current.level * TAKEOVER_RATIO =>
                    {
                        Some(current.deck)
                    }
                    (_, Some(loudest)) => Some(loudest.deck),
                    (_, None) => self.current,
                };
            }
        }
        self.current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level(deck: DeckId, level: f32) -> DeckLevel {
        DeckLevel {
            deck,
            has_beat_grid: true,
            level,
        }
    }

    #[test]
    fn the_louder_deck_takes_over_only_when_clearly_louder() {
        let mut master = TempoMaster::default();
        assert_eq!(
            master.update(&[level(DeckId::A, 1.0), level(DeckId::B, 0.0)]),
            Some(DeckId::A)
        );
        assert_eq!(
            master.update(&[level(DeckId::A, 0.8), level(DeckId::B, 1.0)]),
            Some(DeckId::A)
        );
        assert_eq!(
            master.update(&[level(DeckId::A, 0.5), level(DeckId::B, 1.0)]),
            Some(DeckId::B)
        );
        assert_eq!(
            master.update(&[level(DeckId::A, 0.0), level(DeckId::B, 0.0)]),
            Some(DeckId::B)
        );
    }

    #[test]
    fn decks_without_a_beat_grid_never_lead() {
        let mut master = TempoMaster::default();
        let without_grid = DeckLevel {
            deck: DeckId::A,
            has_beat_grid: false,
            level: 1.0,
        };
        assert_eq!(
            master.update(&[without_grid, level(DeckId::B, 0.1)]),
            Some(DeckId::B)
        );
    }

    #[test]
    fn a_chosen_deck_stays_master_whatever_the_levels() {
        let mut master = TempoMaster::default();
        master.set_source(MasterTempo::Deck(DeckId::B));
        assert_eq!(
            master.update(&[level(DeckId::A, 1.0), level(DeckId::B, 0.0)]),
            Some(DeckId::B)
        );
    }

    #[test]
    fn grids_are_checked_and_count_beats_from_the_first_one() {
        let grid = BeatGrid::new(120.0, 0.5).unwrap();
        assert_eq!(grid.beat_at(0.5), 0.0);
        assert_eq!(grid.beat_at(2.5), 4.0);
        assert!(BeatGrid::new(0.0, 0.0).is_err());
        assert!(BeatGrid::new(f64::NAN, 0.0).is_err());
        assert!(BeatGrid::new(128.0, f64::INFINITY).is_err());
    }
}
