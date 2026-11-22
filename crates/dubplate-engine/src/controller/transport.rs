//! Loading tracks and moving through them: play, pause, cue and seek.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use crate::command::DeckCommand;
use crate::deck::DeckId;
use crate::decode::decode_file;
use crate::error::{EngineError, check_range};
use crate::state::DeckState;
use crate::track::{MAX_TRACK_SECONDS, Track};

use super::Controller;

impl Controller {
    /// Opens an audio file, starts decoding it and loads it on a deck, stopped at its start.
    pub fn load_file(
        &mut self,
        deck: DeckId,
        path: impl AsRef<Path>,
    ) -> Result<Track, EngineError> {
        let track = decode_file(path)?;
        self.load_track(deck, track.clone())?;
        Ok(track)
    }

    /// Loads a track on a deck, stopped at its start. Loading the track another deck holds
    /// plays the same decoded audio on both. A track's decoding stops early once nothing holds
    /// it any more: no deck and no [`Track`] handle.
    pub fn load_track(&mut self, deck: DeckId, track: Track) -> Result<(), EngineError> {
        self.send_deck(deck, DeckCommand::Load(Some(Arc::clone(track.buffer()))))?;
        self.replace_track(deck, Some(track));
        Ok(())
    }

    /// Takes the track off a deck.
    pub fn eject(&mut self, deck: DeckId) -> Result<(), EngineError> {
        self.send_deck(deck, DeckCommand::Load(None))?;
        self.replace_track(deck, None);
        Ok(())
    }

    /// The track last loaded on a deck.
    pub fn track(&self, deck: DeckId) -> Option<&Track> {
        self.decks[deck.index()].track.as_ref()
    }

    /// Starts playback.
    pub fn play(&mut self, deck: DeckId) -> Result<(), EngineError> {
        self.send_deck(deck, DeckCommand::Play)
    }

    /// Stops playback where it is.
    pub fn pause(&mut self, deck: DeckId) -> Result<(), EngineError> {
        self.send_deck(deck, DeckCommand::Pause)
    }

    /// Plays a stopped deck or stops a playing one, as the deck was last published.
    pub fn toggle_play(&mut self, deck: DeckId) -> Result<(), EngineError> {
        if self.deck_state(deck).playing {
            self.pause(deck)
        } else {
            self.play(deck)
        }
    }

    /// The CDJ cue button: while playing, jumps back to the cue point and stops; while stopped,
    /// sets the cue point at the current position.
    pub fn cue(&mut self, deck: DeckId) -> Result<(), EngineError> {
        self.send_deck(deck, DeckCommand::Cue)
    }

    /// Sets the cue point, in seconds from the start of the track.
    pub fn set_cue_point(&mut self, deck: DeckId, seconds: f64) -> Result<(), EngineError> {
        check_range("cue point", seconds, 0.0, MAX_TRACK_SECONDS as f64)?;
        self.send_deck(deck, DeckCommand::SetCuePoint { seconds })
    }

    /// Jumps to a position in seconds from the start of the track, keeping the deck playing or
    /// stopped as it was.
    pub fn seek(&mut self, deck: DeckId, seconds: f64) -> Result<(), EngineError> {
        check_range("seek position", seconds, 0.0, MAX_TRACK_SECONDS as f64)?;
        self.send_deck(deck, DeckCommand::Seek { seconds })
    }

    /// What a deck is doing, as the audio thread last published it.
    pub fn deck_state(&self, deck: DeckId) -> DeckState {
        let shared = &self.shared.decks[deck.index()];
        DeckState {
            loaded: shared.loaded.load(Ordering::Relaxed),
            playing: shared.playing.load(Ordering::Relaxed),
            position_seconds: shared.position_seconds.load(),
            cue_point_seconds: shared.cue_point_seconds.load(),
            tempo: shared.tempo.load(),
        }
    }

    fn replace_track(&mut self, deck: DeckId, track: Option<Track>) {
        self.decks[deck.index()].track = track;
        self.decks[deck.index()].beat_grid = None;
    }
}
