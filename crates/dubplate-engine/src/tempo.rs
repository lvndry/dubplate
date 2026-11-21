use std::fmt;

use crate::deck::DeckId;

/// How much louder another deck must be before it takes over as the automatic tempo master,
/// so two decks at the same level do not swap on every block. About 3 dB.
const TAKEOVER_RATIO: f32 = 1.41;

/// How far a deck's tempo fader reaches either side of the track's own tempo, like the TEMPO
/// RANGE button of a CDJ.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum TempoRange {
    /// ±6%, in steps of 0.02%.
    SixPercent,
    /// ±10%, in steps of 0.04%.
    #[default]
    TenPercent,
    /// ±16%, in steps of 0.04%.
    SixteenPercent,
    /// ±100%, in steps of 0.5%. All the way down stops the track.
    Wide,
}

impl TempoRange {
    /// Every range, in the order the button steps through them.
    pub const ALL: [TempoRange; 4] = [
        TempoRange::SixPercent,
        TempoRange::TenPercent,
        TempoRange::SixteenPercent,
        TempoRange::Wide,
    ];

    /// How far the fader reaches either side, as a share of the track's tempo.
    pub fn span(self) -> f64 {
        match self {
            TempoRange::SixPercent => 0.06,
            TempoRange::TenPercent => 0.10,
            TempoRange::SixteenPercent => 0.16,
            TempoRange::Wide => 1.0,
        }
    }

    /// The smallest tempo step the fader makes, as a share of the track's tempo: rekordbox's
    /// resolution for the range.
    pub fn step(self) -> f64 {
        match self {
            TempoRange::SixPercent => 0.0002,
            TempoRange::TenPercent | TempoRange::SixteenPercent => 0.0004,
            TempoRange::Wide => 0.005,
        }
    }

    /// The next range, wrapping from WIDE back to ±6%.
    pub fn next(self) -> TempoRange {
        let index = Self::ALL
            .iter()
            .position(|range| *range == self)
            .unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }

    pub(crate) fn index(self) -> u8 {
        match self {
            TempoRange::SixPercent => 0,
            TempoRange::TenPercent => 1,
            TempoRange::SixteenPercent => 2,
            TempoRange::Wide => 3,
        }
    }

    pub(crate) fn from_index(index: u8) -> TempoRange {
        Self::ALL
            .get(usize::from(index))
            .copied()
            .unwrap_or_default()
    }
}

impl fmt::Display for TempoRange {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            TempoRange::SixPercent => "±6%",
            TempoRange::TenPercent => "±10%",
            TempoRange::SixteenPercent => "±16%",
            TempoRange::Wide => "WIDE",
        })
    }
}

/// A deck's tempo fader. When the deck's tempo comes from somewhere else, such as sync, the
/// fader no longer matches it; like a CDJ's, it then waits to be moved through the playing
/// tempo before it takes over again, so letting go of sync never makes the tempo jump.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TempoFader {
    position: f64,
    range: TempoRange,
    in_control: bool,
}

impl Default for TempoFader {
    fn default() -> Self {
        Self {
            position: 0.0,
            range: TempoRange::default(),
            in_control: true,
        }
    }
}

impl TempoFader {
    /// The tempo the fader asks for, where 1.0 is the track's own tempo.
    pub(crate) fn tempo(&self) -> f64 {
        (1.0 + self.position * self.range.span()).max(0.0)
    }

    pub(crate) fn range(&self) -> TempoRange {
        self.range
    }

    pub(crate) fn is_in_control(&self) -> bool {
        self.in_control
    }

    /// Moves the fader to `position`, from -1 (slowest) to 1 (fastest), in the range's steps.
    /// Returns the tempo to play when the fader is in control.
    pub(crate) fn move_to(&mut self, position: f64, playing_tempo: f64) -> Option<f64> {
        let steps = (position.clamp(-1.0, 1.0) * self.range.span() / self.range.step()).round();
        self.slide(steps * self.range.step() / self.range.span(), playing_tempo)
    }

    /// Moves the fader to where it asks for `tempo`, which may lie outside the range.
    pub(crate) fn move_to_tempo(&mut self, tempo: f64, playing_tempo: f64) -> Option<f64> {
        self.slide((tempo - 1.0) / self.range.span(), playing_tempo)
    }

    fn slide(&mut self, position: f64, playing_tempo: f64) -> Option<f64> {
        let before = self.tempo() - playing_tempo;
        self.position = position;
        let after = self.tempo() - playing_tempo;
        if !self.in_control && before * after <= 0.0 {
            self.in_control = true;
        }
        self.in_control.then(|| self.tempo())
    }

    /// Changes the range. The fader keeps its position, so the tempo it asks for changes, as on
    /// a CDJ. Returns the tempo to play when the fader is in control.
    pub(crate) fn set_range(&mut self, range: TempoRange) -> Option<f64> {
        self.range = range;
        self.in_control.then(|| self.tempo())
    }

    /// Hands the tempo to something else, such as sync.
    pub(crate) fn hand_over(&mut self) {
        self.in_control = false;
    }

    /// Takes the tempo back: at once when the fader sits on `playing_tempo`, otherwise once it
    /// is moved through it.
    pub(crate) fn take_back(&mut self, playing_tempo: f64) {
        self.in_control = (self.tempo() - playing_tempo).abs() < 1e-9;
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
    /// Always this deck, like the tempo master button on a CDJ. When the deck is stopped or has
    /// no beat grid, the master hands over to the louder playing deck, or keeps the last tempo
    /// when nothing plays.
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
    /// Whether the deck is playing at all, even if the faders cut it to silence.
    pub(crate) playing: bool,
}

impl TempoMaster {
    pub(crate) fn set_source(&mut self, source: MasterTempo) {
        self.source = source;
    }

    pub(crate) fn current(&self) -> Option<DeckId> {
        self.current
    }

    /// Picks the tempo master for the next block. With no deck playing, the last master stays,
    /// so echo trails keep their timing after the music stops. A chosen deck that is stopped or
    /// has no beat grid cannot carry the tempo, so it hands over the same way.
    pub(crate) fn update(&mut self, decks: &[DeckLevel]) -> Option<DeckId> {
        let chosen = match self.source {
            MasterTempo::Deck(deck) => Some(deck),
            MasterTempo::LouderDeck => None,
        };
        if let Some(deck) = chosen {
            let qualifies = decks
                .iter()
                .find(|deck_level| deck_level.deck == deck)
                .is_some_and(|level| level.has_beat_grid && level.playing);
            self.current = if qualifies {
                Some(deck)
            } else {
                self.louder_deck(decks)
            };
        } else {
            self.current = self.louder_deck(decks);
        }
        self.current
    }

    /// The louder-deck rule: the grid deck that reaches the master loudest, keeping the current
    /// one until a challenger is clearly louder. With no qualifying deck, the last master stays.
    fn louder_deck(&self, decks: &[DeckLevel]) -> Option<DeckId> {
        let candidates = decks
            .iter()
            .filter(|candidate| candidate.has_beat_grid && candidate.level > 0.0);
        let loudest = candidates
            .clone()
            .max_by(|first, second| first.level.total_cmp(&second.level));
        let current = candidates
            .clone()
            .find(|candidate| Some(candidate.deck) == self.current);
        match (current, loudest) {
            (Some(current), Some(loudest)) if loudest.level <= current.level * TAKEOVER_RATIO => {
                Some(current.deck)
            }
            (_, Some(loudest)) => Some(loudest.deck),
            (_, None) => self.current,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level(deck: DeckId, level: f32) -> DeckLevel {
        level_with_grid(deck, level, true)
    }

    fn level_with_grid(deck: DeckId, level: f32, has_beat_grid: bool) -> DeckLevel {
        DeckLevel {
            deck,
            has_beat_grid,
            level,
            playing: level > 0.0,
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
            playing: true,
        };
        assert_eq!(
            master.update(&[without_grid, level(DeckId::B, 0.1)]),
            Some(DeckId::B)
        );
    }

    #[test]
    fn a_chosen_deck_stays_master_while_it_plays_with_a_grid() {
        let mut master = TempoMaster::default();
        master.set_source(MasterTempo::Deck(DeckId::B));
        assert_eq!(
            master.update(&[level(DeckId::A, 1.0), level(DeckId::B, 0.4)]),
            Some(DeckId::B)
        );
        assert_eq!(
            master.update(&[level(DeckId::A, 1.0), level(DeckId::B, 0.4)]),
            Some(DeckId::B)
        );
    }

    #[test]
    fn a_stopped_chosen_deck_hands_the_master_to_the_playing_deck() {
        let mut master = TempoMaster::default();
        master.set_source(MasterTempo::Deck(DeckId::A));
        assert_eq!(
            master.update(&[level(DeckId::A, 1.0), level(DeckId::B, 1.0)]),
            Some(DeckId::A)
        );
        assert_eq!(
            master.update(&[level(DeckId::A, 0.0), level(DeckId::B, 1.0)]),
            Some(DeckId::B)
        );
        let mut without_grid = TempoMaster::default();
        without_grid.set_source(MasterTempo::Deck(DeckId::A));
        assert_eq!(
            without_grid.update(&[
                level_with_grid(DeckId::A, 1.0, false),
                level(DeckId::B, 1.0),
            ]),
            Some(DeckId::B)
        );
    }

    #[test]
    fn the_master_keeps_the_last_clock_when_everything_stops() {
        let mut master = TempoMaster::default();
        master.set_source(MasterTempo::Deck(DeckId::A));
        assert_eq!(master.update(&[level(DeckId::A, 1.0)]), Some(DeckId::A));
        let mut louder = TempoMaster::default();
        assert_eq!(
            louder.update(&[level(DeckId::A, 1.0), level(DeckId::B, 0.5)]),
            Some(DeckId::A)
        );
        assert_eq!(
            louder.update(&[level(DeckId::A, 0.0), level(DeckId::B, 0.0)]),
            Some(DeckId::A)
        );
        assert_eq!(
            louder.update(&[level(DeckId::A, 0.0), level(DeckId::B, 0.0)]),
            Some(DeckId::A)
        );
    }

    #[test]
    fn the_fader_moves_in_the_range_steps() {
        let mut fader = TempoFader::default();
        assert_eq!(fader.move_to(0.5, 1.0), Some(1.05));
        fader.set_range(TempoRange::SixPercent);
        let tempo = fader.move_to(0.123_45, 1.0).unwrap();
        assert!((tempo - 1.0074).abs() < 1e-12, "{tempo}");
        fader.set_range(TempoRange::Wide);
        assert_eq!(fader.move_to(-1.0, 1.0), Some(0.0));
        assert_eq!(TempoRange::Wide.next(), TempoRange::SixPercent);
    }

    #[test]
    fn a_fader_that_lost_control_takes_over_once_moved_through_the_playing_tempo() {
        let mut fader = TempoFader::default();
        fader.move_to(0.0, 1.0);
        fader.hand_over();
        fader.take_back(1.03);
        assert!(!fader.is_in_control());
        assert_eq!(fader.move_to(0.2, 1.03), None);
        assert_eq!(fader.move_to(0.4, 1.03), Some(1.04));
        assert!(fader.is_in_control());
    }

    #[test]
    fn changing_the_range_keeps_the_fader_position() {
        let mut fader = TempoFader::default();
        fader.move_to(0.5, 1.0);
        assert_eq!(fader.set_range(TempoRange::SixteenPercent), Some(1.08));
    }
}
