//! The jog wheels: touch, scratch, nudge and search.

use std::sync::atomic::Ordering;

use crate::command::{Command, DeckCommand};
use crate::deck::DeckId;
use crate::error::EngineError;
use crate::jog::JogInput;
use crate::state::PlatterState;

use super::Controller;

impl Controller {
    /// The platter top was touched or let go. In vinyl mode a touch holds the record, like a
    /// hand on a turntable: the music stops under the hand and follows the platter
    /// ([`Controller::jog_scratch`]); letting go returns to the deck's tempo, or to a stop when
    /// the deck is paused. Out of vinyl mode a touch does nothing.
    pub fn set_jog_touch(&mut self, deck: DeckId, touched: bool) -> Result<(), EngineError> {
        self.jog(deck, JogInput::Touch(touched))
    }

    /// The platter turned by `ticks` while touched, positive clockwise: the record moves with
    /// it, one revolution for 1.8 seconds of the track, as a record at 33⅓ RPM. Without a hand
    /// on the platter it nudges instead.
    pub fn jog_scratch(&mut self, deck: DeckId, ticks: i32) -> Result<(), EngineError> {
        self.jog(deck, JogInput::Scratch(ticks))
    }

    /// The jog's edge turned by `ticks`, positive clockwise. On a playing deck it bends the
    /// tempo with the wheel's speed to line up beats, and the bend fades out smoothly when the
    /// wheel stops; every revolution moves the music 0.3 seconds. On a stopped deck it moves
    /// the playhead like a hand on the record, to find a beat by ear.
    pub fn jog_nudge(&mut self, deck: DeckId, ticks: i32) -> Result<(), EngineError> {
        self.jog(deck, JogInput::Nudge(ticks))
    }

    /// The jog turned by `ticks` with SHIFT held: moves quickly through the track, 15 seconds
    /// per revolution, playing or not.
    pub fn jog_search(&mut self, deck: DeckId, ticks: i32) -> Result<(), EngineError> {
        self.jog(deck, JogInput::Search(ticks))
    }

    /// Sets how many ticks the controller reports per revolution of the platter. The
    /// DDJ-FLX4 reports [`crate::DEFAULT_JOG_TICKS_PER_REVOLUTION`].
    pub fn set_jog_ticks_per_revolution(&mut self, ticks: u32) -> Result<(), EngineError> {
        if ticks == 0 {
            return Err(EngineError::InvalidParameter {
                name: "jog ticks per revolution",
                value: 0.0,
            });
        }
        self.send(Command::SetJogTicksPerRevolution(ticks))
    }

    /// Switches a deck's vinyl mode, on by default: on, touching the platter top holds the
    /// record for scratching; off, the platter only nudges, like a CDJ in CDJ mode.
    pub fn set_vinyl_mode(&mut self, deck: DeckId, enabled: bool) -> Result<(), EngineError> {
        self.send_deck(deck, DeckCommand::SetVinylMode(enabled))
    }

    fn jog(&mut self, deck: DeckId, input: JogInput) -> Result<(), EngineError> {
        self.send_deck(deck, DeckCommand::Jog(input))
    }

    /// What a deck's jog wheel is doing, as the audio thread last published it.
    pub fn platter_state(&self, deck: DeckId) -> PlatterState {
        let shared = &self.shared.decks[deck.index()];
        PlatterState {
            speed: shared.speed.load(),
            touched: shared.touched.load(Ordering::Relaxed),
            vinyl_mode: shared.vinyl_mode.load(Ordering::Relaxed),
        }
    }
}
