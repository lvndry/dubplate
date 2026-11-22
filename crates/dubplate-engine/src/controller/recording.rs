//! Recording the master as it goes to the speakers.

use crate::command::Command;
use crate::error::{EngineError, check_range};
use crate::recording::{RecordingTap, recording_tap};

use super::Controller;

/// The longest a recording tap can buffer, in seconds.
const MAX_RECORDING_BUFFER_SECONDS: f64 = 600.0;

impl Controller {
    /// Starts a recording tap: the master output exactly as it goes to the speakers, after the
    /// master level and the limiter, into a buffer of `buffer_seconds` that the returned tap
    /// reads. Starting another tap stops this one. Frames that do not fit while the tap is not
    /// read are dropped and counted, never waited for.
    pub fn start_recording(&mut self, buffer_seconds: f64) -> Result<RecordingTap, EngineError> {
        check_range(
            "recording buffer seconds",
            buffer_seconds,
            0.01,
            MAX_RECORDING_BUFFER_SECONDS,
        )?;
        let sample_rate = self.shared.stats.sample_rate;
        let capacity_frames = (buffer_seconds * f64::from(sample_rate)).ceil() as usize;
        let (tap, sink) = recording_tap(capacity_frames, sample_rate);
        self.send(Command::SetRecording(Some(sink)))?;
        Ok(tap)
    }

    /// Stops the recording tap. Frames already in its buffer can still be read.
    pub fn stop_recording(&mut self) -> Result<(), EngineError> {
        self.send(Command::SetRecording(None))
    }
}
