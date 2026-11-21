//! Dubplate's audio engine: two decks and a master bus, mixed on a real-time audio thread.
//!
//! The engine has three sides that never wait for each other:
//!
//! - The **audio thread** runs [`Renderer::render`] from the sound card's callback. It reads
//!   decoded audio, mixes the decks and runs the insert processors. It never allocates, locks,
//!   blocks or does I/O.
//! - A **decoding thread** per track ([`decode_file`]) turns the file into stereo `f32` frames
//!   in a [`Track`], much faster than real time. The audio thread reads any frame already
//!   decoded without waiting, so cue, seek and hot cues are instant.
//! - The **control side** ([`Controller`]) sends commands (load, play, cue, seek, tempo, gain,
//!   faders, inserts) over a lock-free queue and reads deck state, meters and dropout counters
//!   back through atomics. Objects the audio thread lets go of, such as a replaced track or
//!   effect, come back to the control side to be freed.
//!
//! [`AudioOutput::start`] runs the engine on the default sound card; [`offline_engine`] gives
//! the same engine without one, for tests and offline rendering.

mod controller;
mod curves;
mod deck;
mod decode;
mod error;
mod mixer;
mod output;
mod processor;
mod state;
mod track;

pub use controller::{Controller, offline_engine};
pub use cpal;
pub use curves::{channel_fader_gain, crossfader_gains, decibels_to_gain, gain_to_decibels};
pub use deck::DeckId;
pub use decode::decode_file;
pub use error::EngineError;
pub use mixer::Renderer;
pub use output::{AudioOutput, OutputConfig};
pub use processor::{AudioProcessor, ProcessContext, StereoBlock};
pub use state::{DeckState, EngineStats, Meters};
pub use track::{DecodeStatus, MAX_SAMPLE_RATE, MAX_TRACK_SECONDS, MIN_SAMPLE_RATE, Track};

/// Number of decks.
pub const DECK_COUNT: usize = 2;

/// Number of insert slots on each deck and on the master bus.
pub const INSERT_SLOTS: usize = 4;

/// Largest block the engine processes at once. Device buffers larger than this are rendered
/// in several blocks, so [`StereoBlock`]s handed to processors never hold more frames.
pub const MAX_BLOCK_FRAMES: usize = 512;
