//! Auditioning a seam in the headphones.

use std::sync::atomic::Ordering;

use crate::command::Command;
use crate::error::{EngineError, check_range};
use crate::preview::{MAX_PREVIEW_SECONDS, Preview, PreviewPlan, PreviewState};
use crate::track::MAX_TRACK_SECONDS;

use super::{Controller, MAX_TEMPO};

impl Controller {
    /// Auditions a seam in the headphones, or stops the audition with `None`: two tracks, each
    /// from its own start at its own tempo, the second coming in at the planned overlap, with
    /// key lock if asked. The preview plays on the headphone bus only, after the cue mix and
    /// before the headphone level, and never touches the decks or the master output. It stops
    /// itself when both tracks end, or after [`crate::MAX_PREVIEW_SECONDS`].
    pub fn preview(&mut self, preview: Option<Preview>) -> Result<(), EngineError> {
        if let Some(preview) = &preview {
            for preview_track in [&preview.first, &preview.second] {
                check_range(
                    "preview start",
                    preview_track.start_seconds,
                    0.0,
                    MAX_TRACK_SECONDS as f64,
                )?;
                if !(preview_track.tempo.is_finite()
                    && preview_track.tempo > 0.0
                    && preview_track.tempo <= MAX_TEMPO)
                {
                    return Err(EngineError::InvalidParameter {
                        name: "preview tempo",
                        value: preview_track.tempo,
                    });
                }
            }
            check_range(
                "preview overlap",
                preview.second_offset_seconds,
                0.0,
                MAX_PREVIEW_SECONDS,
            )?;
        }
        self.send(Command::Preview(preview.as_ref().map(PreviewPlan::new)))
    }

    /// Whether a preview plays and how far it is, as the audio thread last published it.
    pub fn preview_state(&self) -> PreviewState {
        PreviewState {
            playing: self.shared.preview_playing.load(Ordering::Relaxed),
            position_seconds: self.shared.preview_position_seconds.load(),
        }
    }
}
