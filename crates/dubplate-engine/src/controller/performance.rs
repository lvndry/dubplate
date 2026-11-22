//! Quantize, hot cues, loops, beat jumps, slip, rolls and pad effects.

use std::sync::atomic::Ordering;

use crate::BeatLength;
use crate::command::{Command, DeckCommand};
use crate::deck::DeckId;
use crate::error::{EngineError, check_range};
use crate::pad_effects::PadEffect;
use crate::state::{LoopState, SlipState, decode_loop_length};
use crate::track::MAX_TRACK_SECONDS;

use super::Controller;

/// The furthest one beat jump goes, in beats.
const MAX_JUMP_BEATS: f64 = 1_024.0;

impl Controller {
    /// Switches quantize for both decks, on by default. With quantize and a beat grid, the cue
    /// button, loop in and loop out set their points on the nearest beat, beat loops start on
    /// the beat just passed, and hot cues and reloop jump in rhythm.
    pub fn set_quantize(&mut self, enabled: bool) -> Result<(), EngineError> {
        self.send(Command::SetQuantize(enabled))?;
        self.quantize = enabled;
        Ok(())
    }

    /// Whether quantize is on.
    pub fn quantize(&self) -> bool {
        self.quantize
    }

    /// A hot cue at `seconds`: while playing, the deck jumps there in rhythm with quantize, as
    /// far past it as the playhead was past its last beat, so the music carries on in time; on
    /// a stopped deck it jumps there and plays.
    pub fn hot_cue(&mut self, deck: DeckId, seconds: f64) -> Result<(), EngineError> {
        check_range("hot cue", seconds, 0.0, MAX_TRACK_SECONDS as f64)?;
        self.send_deck(deck, DeckCommand::HotCue { seconds })
    }

    /// Jumps by `beats` beats on the beat grid, forwards when positive, keeping the place in
    /// the beat. With a loop on, the loop moves with the playhead. Without a grid, beats count
    /// at 120 BPM.
    pub fn beat_jump(&mut self, deck: DeckId, beats: f64) -> Result<(), EngineError> {
        check_range("beat jump", beats, -MAX_JUMP_BEATS, MAX_JUMP_BEATS)?;
        self.send_deck(deck, DeckCommand::BeatJump { beats })
    }

    /// LOOP IN: marks where a loop starts, on the nearest beat with quantize.
    pub fn loop_in(&mut self, deck: DeckId) -> Result<(), EngineError> {
        self.send_deck(deck, DeckCommand::LoopIn)
    }

    /// LOOP OUT: ends the loop where the deck is, on the nearest beat with quantize, and turns
    /// it on. Without a loop in point it moves the end of the current loop.
    pub fn loop_out(&mut self, deck: DeckId) -> Result<(), EngineError> {
        self.send_deck(deck, DeckCommand::LoopOut)
    }

    /// A beat loop of `length`, from a sixteenth of a beat to thirty-two beats. It starts on
    /// the beat just passed with quantize, so the music never jumps; with a loop already on,
    /// it changes that loop's length from its start.
    pub fn beat_loop(&mut self, deck: DeckId, length: BeatLength) -> Result<(), EngineError> {
        self.send_deck(deck, DeckCommand::BeatLoop(length))
    }

    /// Halves the loop from its start, down to a sixteenth of a beat. A playhead past the new
    /// end goes back inside it in rhythm.
    pub fn halve_loop(&mut self, deck: DeckId) -> Result<(), EngineError> {
        self.send_deck(deck, DeckCommand::HalveLoop)
    }

    /// Doubles the loop from its start, up to thirty-two beats.
    pub fn double_loop(&mut self, deck: DeckId) -> Result<(), EngineError> {
        self.send_deck(deck, DeckCommand::DoubleLoop)
    }

    /// RELOOP/EXIT: leaves the loop that is on and plays on, or turns the last loop back on
    /// and jumps to its start, in rhythm with quantize.
    pub fn reloop_or_exit(&mut self, deck: DeckId) -> Result<(), EngineError> {
        self.send_deck(deck, DeckCommand::ReloopOrExit)
    }

    /// A deck's loop, as the audio thread last published it.
    pub fn loop_state(&self, deck: DeckId) -> LoopState {
        let shared = &self.shared.decks[deck.index()];
        let seconds = |value: f64| value.is_finite().then_some(value);
        LoopState {
            start_seconds: seconds(shared.loop_start_seconds.load()),
            end_seconds: seconds(shared.loop_end_seconds.load()),
            active: shared.loop_active.load(Ordering::Relaxed),
            length: decode_loop_length(shared.loop_length.load(Ordering::Relaxed)),
            pending_start_seconds: seconds(shared.pending_loop_start_seconds.load()),
        }
    }

    /// The SLIP button: with it on, scratching and loops slip as well as rolls. While an action
    /// slips, the track keeps playing silently underneath and the deck lands back on it, in
    /// rhythm, when the action ends.
    pub fn set_slip(&mut self, deck: DeckId, enabled: bool) -> Result<(), EngineError> {
        self.send_deck(deck, DeckCommand::SetSlip(enabled))
    }

    /// Starts or changes a loop roll of `length` while a pad is held, like rekordbox's roll:
    /// the deck repeats the beat just passed while the track keeps playing underneath.
    pub fn beat_roll(&mut self, deck: DeckId, length: BeatLength) -> Result<(), EngineError> {
        self.send_deck(deck, DeckCommand::BeatRoll(Some(length)))
    }

    /// Lets go of a roll: the deck lands in rhythm where the track would have been.
    pub fn release_roll(&mut self, deck: DeckId) -> Result<(), EngineError> {
        self.send_deck(deck, DeckCommand::BeatRoll(None))
    }

    /// A deck's slip mode, as the audio thread last published it.
    pub fn slip_state(&self, deck: DeckId) -> SlipState {
        let shared = &self.shared.decks[deck.index()];
        let shadow = shared.slip_shadow_seconds.load();
        SlipState {
            enabled: shared.slip_enabled.load(Ordering::Relaxed),
            shadow_seconds: shadow.is_finite().then_some(shadow),
        }
    }

    /// Holds a pad effect on a deck, after its EQ and filter, until released. Holding another
    /// lets go of the first. Brake and backspin give back the live track on release, which
    /// kept playing underneath; echoes and reverbs ring out.
    pub fn hold_pad_effect(&mut self, deck: DeckId, pad: PadEffect) -> Result<(), EngineError> {
        check_range("pad effect depth", f64::from(pad.depth), 0.0, 1.0)?;
        self.send_deck(deck, DeckCommand::HoldPadEffect(pad))?;
        self.decks[deck.index()].pad_effect = Some(pad);
        Ok(())
    }

    /// Lets go of a deck's pad effect.
    pub fn release_pad_effect(&mut self, deck: DeckId) -> Result<(), EngineError> {
        self.send_deck(deck, DeckCommand::ReleasePadEffect)?;
        self.decks[deck.index()].pad_effect = None;
        Ok(())
    }

    /// The pad effect held on a deck.
    pub fn pad_effect(&self, deck: DeckId) -> Option<PadEffect> {
        self.decks[deck.index()].pad_effect
    }
}
