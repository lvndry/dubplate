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

    /// No audio output device is available.
    #[error("no audio output device is available")]
    NoOutputDevice,

    /// The audio output could not be configured or started.
    #[error("audio output error: {0}")]
    Output(String),
}
