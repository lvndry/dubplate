use std::path::PathBuf;

/// Errors returned by the engine.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    /// The audio file could not be opened.
    #[error("could not open {path}: {source}")]
    OpenFile {
        /// File that failed to open.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },

    /// The file is not audio Dubplate can decode, or its format is not supported.
    #[error("could not decode {path}: {message}")]
    Decode {
        /// File that failed to decode.
        path: PathBuf,
        /// What went wrong.
        message: String,
    },

    /// The command queue to the audio thread is full; the command was not sent.
    #[error("the audio thread is not keeping up with commands; the command was dropped")]
    CommandQueueFull,

    /// The audio thread has not published the latest control changes yet.
    #[error("the deck is still applying a change; try again in a moment")]
    DeckStatePending,

    /// A parameter was out of range or not a finite number.
    #[error("invalid {name}: {value}")]
    InvalidParameter {
        /// Parameter name.
        name: &'static str,
        /// Value that was rejected.
        value: f64,
    },

    /// An insert slot index past the last slot.
    #[error("insert slot {slot} does not exist; there are {slot_count} slots")]
    InvalidInsertSlot {
        /// Slot that was requested.
        slot: usize,
        /// Number of slots available.
        slot_count: usize,
    },

    /// An effect unit number past the last unit.
    #[error("effect unit {unit} does not exist; there are {unit_count} units")]
    InvalidEffectUnit {
        /// Unit that was requested, counted from 0.
        unit: usize,
        /// Number of effect units.
        unit_count: usize,
    },

    /// A sampler slot past the last slot.
    #[error("sampler slot {slot} does not exist; there are {slot_count} slots")]
    InvalidSamplerSlot {
        /// Slot that was requested, counted from 0.
        slot: usize,
        /// Number of sampler slots.
        slot_count: usize,
    },

    /// The stems are not as long as the track, so they are not this track's.
    #[error("the stems last {stems_seconds:.2} s but the track lasts {track_seconds:.2} s")]
    StemsDoNotMatch {
        /// Length of the track in seconds, or of what is decoded of it so far.
        track_seconds: f64,
        /// Length of the stems in seconds.
        stems_seconds: f64,
    },

    /// The stems are not at the track's sample rate.
    #[error("the stems are at {stems} Hz but the track is at {track} Hz")]
    StemsSampleRate {
        /// The track's sample rate in Hz.
        track: u32,
        /// The stems' sample rate in Hz.
        stems: u32,
    },

    /// The track has its stems already.
    #[error("the track has its stems already")]
    StemsAlreadyAttached,

    /// The command needs the track's beat grid and the deck does not have one.
    #[error("the deck has no beat grid, so it cannot {name}")]
    NoBeatGrid {
        /// The command that was refused.
        name: &'static str,
    },

    /// No audio output device is available.
    #[error("no audio output device is available")]
    NoOutputDevice,

    /// No output device has the name asked for.
    #[error("no audio output device is called {0}")]
    OutputDeviceNotFound(String),

    /// The audio output could not be configured or started.
    #[error("audio output error: {0}")]
    Output(String),
}

/// Accepts a finite `value` from `minimum` to `maximum`, both included.
pub(crate) fn check_range(
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
