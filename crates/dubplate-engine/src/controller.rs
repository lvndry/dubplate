use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use rtrb::{Consumer, Producer, RingBuffer};

use crate::deck::DeckId;
use crate::decode::decode_file;
use crate::error::EngineError;
use crate::mixer::{Command, Renderer, RetireQueue, Retired};
use crate::processor::AudioProcessor;
use crate::state::{DeckState, EngineStats, Meters, SharedState, take_peak};
use crate::track::{MAX_TRACK_SECONDS, Track};
use crate::{DECK_COUNT, INSERT_SLOTS};

const COMMAND_CAPACITY: usize = 1024;
const RETIRE_CAPACITY: usize = COMMAND_CAPACITY * 4;
const MAX_TEMPO: f64 = 4.0;
const MAX_GAIN: f32 = 16.0;

/// Creates an engine whose audio side is driven by hand instead of a sound card.
///
/// Call [`Renderer::render`] to produce audio. [`crate::AudioOutput::start`] does the same with
/// a real device.
pub fn offline_engine(sample_rate: u32) -> (Controller, Renderer) {
    let (command_producer, command_consumer) = RingBuffer::new(COMMAND_CAPACITY);
    let (retire_producer, retire_consumer) = RingBuffer::new(RETIRE_CAPACITY);
    let shared = Arc::new(SharedState::new(sample_rate));
    let controller = Controller {
        commands: command_producer,
        retired: retire_consumer,
        shared: Arc::clone(&shared),
        tracks: std::array::from_fn(|_| None),
    };
    let renderer = Renderer::new(
        sample_rate,
        command_consumer,
        RetireQueue::new(retire_producer),
        shared,
    );
    (controller, renderer)
}

/// The control side of the engine, for the UI, a MIDI controller or a test.
///
/// Every method returns at once: commands go to the audio thread through a lock-free queue and
/// take effect at the start of its next callback, and state comes back through atomics.
pub struct Controller {
    commands: Producer<Command>,
    retired: Consumer<Retired>,
    shared: Arc<SharedState>,
    tracks: [Option<Track>; DECK_COUNT],
}

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
        self.send(Command::Load {
            deck,
            track: Some(Arc::clone(track.buffer())),
        })?;
        self.replace_track(deck, Some(track));
        Ok(())
    }

    /// Takes the track off a deck.
    pub fn eject(&mut self, deck: DeckId) -> Result<(), EngineError> {
        self.send(Command::Load { deck, track: None })?;
        self.replace_track(deck, None);
        Ok(())
    }

    /// The track last loaded on a deck.
    pub fn track(&self, deck: DeckId) -> Option<&Track> {
        self.tracks[deck.index()].as_ref()
    }

    /// Starts playback.
    pub fn play(&mut self, deck: DeckId) -> Result<(), EngineError> {
        self.send(Command::Play(deck))
    }

    /// Stops playback where it is.
    pub fn pause(&mut self, deck: DeckId) -> Result<(), EngineError> {
        self.send(Command::Pause(deck))
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
        self.send(Command::Cue(deck))
    }

    /// Sets the cue point, in seconds from the start of the track.
    pub fn set_cue_point(&mut self, deck: DeckId, seconds: f64) -> Result<(), EngineError> {
        check_range("cue point", seconds, 0.0, MAX_TRACK_SECONDS as f64)?;
        self.send(Command::SetCuePoint { deck, seconds })
    }

    /// Jumps to a position in seconds from the start of the track, keeping the deck playing or
    /// stopped as it was.
    pub fn seek(&mut self, deck: DeckId, seconds: f64) -> Result<(), EngineError> {
        check_range("seek position", seconds, 0.0, MAX_TRACK_SECONDS as f64)?;
        self.send(Command::Seek { deck, seconds })
    }

    /// Sets the playback speed, where 1.0 is the track's own tempo and 1.08 is +8%. Pitch
    /// follows speed, as on a turntable.
    pub fn set_tempo(&mut self, deck: DeckId, tempo: f64) -> Result<(), EngineError> {
        if !(tempo.is_finite() && tempo > 0.0 && tempo <= MAX_TEMPO) {
            return Err(EngineError::InvalidParameter {
                name: "tempo",
                value: tempo,
            });
        }
        self.send(Command::SetTempo { deck, tempo })
    }

    /// Sets the deck's trim gain, a linear factor where 1.0 leaves the level unchanged.
    pub fn set_gain(&mut self, deck: DeckId, gain: f32) -> Result<(), EngineError> {
        check_range("gain", f64::from(gain), 0.0, f64::from(MAX_GAIN))?;
        self.send(Command::SetTrim { deck, gain })
    }

    /// Sets the channel fader, from 0.0 (closed) to 1.0 (fully open).
    pub fn set_channel_fader(&mut self, deck: DeckId, position: f32) -> Result<(), EngineError> {
        check_range("channel fader", f64::from(position), 0.0, 1.0)?;
        self.send(Command::SetChannelFader { deck, position })
    }

    /// Sets the crossfader, from 0.0 (deck A only) to 1.0 (deck B only).
    pub fn set_crossfader(&mut self, position: f32) -> Result<(), EngineError> {
        check_range("crossfader", f64::from(position), 0.0, 1.0)?;
        self.send(Command::SetCrossfader(position))
    }

    /// Sets the master gain, a linear factor where 1.0 leaves the level unchanged.
    pub fn set_master_gain(&mut self, gain: f32) -> Result<(), EngineError> {
        check_range("master gain", f64::from(gain), 0.0, f64::from(MAX_GAIN))?;
        self.send(Command::SetMasterGain(gain))
    }

    /// Puts a processor in one of a deck's insert slots, or empties the slot with `None`. Deck
    /// inserts run after the trim and before the channel fader, in slot order.
    pub fn set_deck_insert(
        &mut self,
        deck: DeckId,
        slot: usize,
        processor: Option<Box<dyn AudioProcessor>>,
    ) -> Result<(), EngineError> {
        check_slot(slot)?;
        self.send(Command::SetDeckInsert {
            deck,
            slot,
            processor,
        })
    }

    /// Puts a processor in one of the master insert slots, or empties the slot with `None`.
    /// Master inserts run on the mix before the master gain, in slot order.
    pub fn set_master_insert(
        &mut self,
        slot: usize,
        processor: Option<Box<dyn AudioProcessor>>,
    ) -> Result<(), EngineError> {
        check_slot(slot)?;
        self.send(Command::SetMasterInsert { slot, processor })
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

    /// Peak levels since the previous call.
    pub fn take_meters(&self) -> Meters {
        Meters {
            deck_peaks: std::array::from_fn(|index| take_peak(&self.shared.decks[index].peak)),
            master_peak: take_peak(&self.shared.master_peak),
        }
    }

    /// Buffer size, latency and dropout counters.
    pub fn stats(&self) -> EngineStats {
        self.shared.stats.snapshot()
    }

    pub(crate) fn shared(&self) -> &Arc<SharedState> {
        &self.shared
    }

    /// Drops what the audio thread handed back. Every command does this first, so calling it
    /// is only needed to free memory sooner on an idle controller.
    pub fn collect_garbage(&mut self) {
        while let Ok(retired) = self.retired.pop() {
            drop(retired);
        }
    }

    fn send(&mut self, command: Command) -> Result<(), EngineError> {
        self.collect_garbage();
        self.commands
            .push(command)
            .map_err(|_| EngineError::CommandQueueFull)
    }

    fn replace_track(&mut self, deck: DeckId, track: Option<Track>) {
        self.tracks[deck.index()] = track;
    }
}

fn check_range(
    name: &'static str,
    value: f64,
    minimum: f64,
    maximum: f64,
) -> Result<(), EngineError> {
    if value.is_finite() && value >= minimum && value <= maximum {
        Ok(())
    } else {
        Err(EngineError::InvalidParameter { name, value })
    }
}

fn check_slot(slot: usize) -> Result<(), EngineError> {
    if slot < INSERT_SLOTS {
        Ok(())
    } else {
        Err(EngineError::InvalidInsertSlot {
            slot,
            slot_count: INSERT_SLOTS,
        })
    }
}
