//! The window's side of recording the mix: REC starts a WAV file of the master as it goes to the
//! speakers, the header counts the time and warns when audio was skipped, and stopping or
//! quitting closes the file.

use std::path::Path;
use std::time::Duration;

use gpui::Context;

use super::notice::NoticePlace;
use super::{DubplateApp, error_chain};
use crate::recording::{Recording, RecordingError, RecordingSummary, recordings_directory};
use crate::set::running_time;

/// How much audio the engine holds for the writer, more than a slow disk ever needs.
const TAP_BUFFER_SECONDS: f64 = 4.0;
/// How often the header's time and the writer's health are looked at.
const CHECK_INTERVAL: Duration = Duration::from_millis(500);

/// What the header shows about a recording in progress.
pub(super) struct RecordingStatus {
    /// How long it has run.
    pub elapsed: Duration,
    /// How much audio was skipped because the disk fell behind.
    pub skipped: Duration,
}

impl DubplateApp {
    /// Starts recording the mix, or stops and closes the file.
    pub(super) fn toggle_recording(&mut self, cx: &mut Context<'_, Self>) {
        if self.recording.is_some() {
            self.stop_recording(cx);
        } else {
            self.start_recording(cx);
        }
    }

    /// The recording in progress, as the header shows it.
    pub(super) fn recording_status(&self) -> Option<RecordingStatus> {
        let recording = self.recording.as_ref()?;
        Some(RecordingStatus {
            elapsed: recording.elapsed(),
            skipped: self.frames_as_time(recording.dropped_frames()),
        })
    }

    /// Whether the mix is being recorded.
    pub(super) fn is_recording(&self) -> bool {
        self.recording.is_some()
    }

    fn start_recording(&mut self, cx: &mut Context<'_, Self>) {
        let Some(directory) = recordings_directory() else {
            self.show_notice(
                NoticePlace::Window,
                "There is no Music folder to record into.",
                cx,
            );
            return;
        };
        let tap = match &mut self.engine {
            Ok(engine) => engine.controller.start_recording(TAP_BUFFER_SECONDS),
            Err(_) => {
                self.show_notice(NoticePlace::Window, "There is no audio to record.", cx);
                return;
            }
        };
        let started = tap
            .map_err(|error| error_chain(&error))
            .and_then(|tap| Recording::start(&directory, tap).map_err(|error| error.to_string()));
        match started {
            Ok(recording) => {
                self.recording = Some(recording);
                self.recording_skip_reported = false;
                self.watch_recording(cx);
            }
            Err(problem) => {
                self.engine_command(cx, |controller| controller.stop_recording());
                self.show_notice(
                    NoticePlace::Window,
                    format!("The recording could not start: {problem}"),
                    cx,
                );
            }
        }
        cx.notify();
    }

    fn stop_recording(&mut self, cx: &mut Context<'_, Self>) {
        let Some(recording) = self.recording.take() else {
            return;
        };
        self.engine_command(cx, |controller| controller.stop_recording());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let finished = cx
                .background_executor()
                .spawn(async move { recording.finish() })
                .await;
            this.update(cx, |this, cx| this.report_recording(finished, cx))
                .ok();
        })
        .detach();
    }

    /// Closes the file before the app quits, so the recording is complete.
    pub(super) fn finish_recording_on_quit(&mut self) {
        if let Some(recording) = self.recording.take() {
            if let Ok(engine) = &mut self.engine {
                engine.controller.stop_recording().ok();
            }
            recording.finish().ok();
        }
    }

    fn report_recording(
        &mut self,
        finished: Result<RecordingSummary, RecordingError>,
        cx: &mut Context<'_, Self>,
    ) {
        let message = match finished {
            Ok(summary) => {
                let skipped = self.frames_as_time(summary.dropped_frames);
                let mut message = format!(
                    "Recorded {} to {}",
                    running_time(summary.duration),
                    file_label(&summary.path)
                );
                if !skipped.is_zero() {
                    message.push_str(&format!(
                        ", with {:.1} s skipped where the disk fell behind",
                        skipped.as_secs_f64()
                    ));
                }
                message
            }
            Err(error) => format!("The recording failed: {error}"),
        };
        self.show_notice(NoticePlace::Window, message, cx);
    }

    /// Repaints the header's time while recording, warns once when audio is skipped, and closes
    /// the file when the writer fails.
    fn watch_recording(&mut self, cx: &mut Context<'_, Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(CHECK_INTERVAL).await;
                let recording = this.update(cx, |this, cx| {
                    this.check_recording(cx);
                    this.recording.is_some()
                });
                if !matches!(recording, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    fn check_recording(&mut self, cx: &mut Context<'_, Self>) {
        let Some(recording) = &self.recording else {
            return;
        };
        if recording.has_failed() {
            if let Some(recording) = self.recording.take() {
                self.engine_command(cx, |controller| controller.stop_recording());
                let finished = recording.finish();
                self.report_recording(finished, cx);
            }
        } else if recording.dropped_frames() > 0 && !self.recording_skip_reported {
            self.recording_skip_reported = true;
            self.show_notice(
                NoticePlace::Window,
                "The recording skipped some audio: the disk fell behind.",
                cx,
            );
        }
        cx.notify();
    }

    fn frames_as_time(&self, frames: u64) -> Duration {
        let sample_rate = self
            .engine
            .as_ref()
            .map_or(0, |engine| engine.controller.stats().sample_rate);
        if sample_rate == 0 {
            return Duration::ZERO;
        }
        Duration::from_secs_f64(frames as f64 / f64::from(sample_rate))
    }
}

/// `Dubplate 2026-11-22 01-23-45.wav in Dubplate Recordings`.
fn file_label(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    match path
        .parent()
        .and_then(Path::file_name)
        .map(|folder| folder.to_string_lossy().into_owned())
    {
        Some(folder) => format!("{name} in {folder}"),
        None => name,
    }
}
