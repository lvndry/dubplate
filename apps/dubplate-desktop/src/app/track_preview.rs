//! Hearing a library track in the headphones before loading it, as rekordbox's preview and a
//! CDJ's track preview: P plays the selected track on the engine's preview bus, never on the
//! master and never on a deck, from its first memory or hot cue, and P again stops it.

use std::time::Duration;

use dubplate_engine::{DecodeStatus, MAX_PREVIEW_SECONDS, Preview, PreviewTrack, Track};
use gpui::Context;

use super::DubplateApp;
use super::notice::NoticePlace;
use dubplate_rekordbox::MemoryCue;

use crate::collection::{CollectionTrack, TrackIndex};

/// How often a decoding track is looked at, and a playing preview checked.
const CHECK_INTERVAL: Duration = Duration::from_millis(50);
/// Decoded audio wanted past the start before the preview plays.
const DECODE_AHEAD_SECONDS: f64 = 4.0;

/// A library track playing, or about to play, in the headphones.
pub(super) struct TrackPreview {
    /// The track.
    pub track: TrackIndex,
    generation: u64,
}

/// Where a track's preview starts: its earliest memory or hot cue, where the DJ marked the
/// track, else its start.
fn preview_start(track: &CollectionTrack) -> f64 {
    let cues = &track.cues;
    cues.memory_cues()
        .iter()
        .map(MemoryCue::start_seconds)
        .chain(cues.hot_cues().map(|(_, cue)| cue.start_seconds()))
        .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
        .min_by(f64::total_cmp)
        .unwrap_or(0.0)
}

impl DubplateApp {
    /// Plays the track selected in the library in the headphones, or stops the one playing.
    pub(super) fn toggle_track_preview(&mut self, cx: &mut Context<'_, Self>) {
        let Some(selected) = self.selected_track(cx) else {
            return;
        };
        let same = self
            .track_preview
            .as_ref()
            .is_some_and(|preview| preview.track == selected);
        self.stop_track_preview(cx);
        if same {
            return;
        }
        self.stop_audition(cx);
        let Some(track) = self
            .collection()
            .map(|collection| std::sync::Arc::clone(collection.track(selected)))
        else {
            return;
        };
        let Some(path) = track.track.file_path.clone() else {
            return;
        };
        self.preview_generation += 1;
        let generation = self.preview_generation;
        self.track_preview = Some(TrackPreview {
            track: selected,
            generation,
        });
        let start_seconds = preview_start(&track);
        let title = track.row.title.clone();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let decoded = cx
                .background_executor()
                .spawn(async move { dubplate_engine::decode_file(&path) })
                .await;
            let audio = match decoded {
                Ok(audio) => audio,
                Err(error) => {
                    this.update(cx, |this, cx| {
                        this.end_track_preview(generation, cx);
                        this.show_notice(
                            NoticePlace::Window,
                            format!("{title} could not be previewed: {error}"),
                            cx,
                        );
                    })
                    .ok();
                    return;
                }
            };
            while !decoded_to(&audio, start_seconds + DECODE_AHEAD_SECONDS) {
                cx.background_executor().timer(CHECK_INTERVAL).await;
                let current = this
                    .update(cx, |this, _| this.is_track_preview(generation))
                    .unwrap_or(false);
                if !current {
                    return;
                }
            }
            this.update(cx, |this, cx| {
                if this.is_track_preview(generation) {
                    this.play_track_preview(audio, start_seconds, generation, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// The library track playing in the headphones, if any.
    pub(super) fn previewed_track(&self) -> Option<TrackIndex> {
        self.track_preview.as_ref().map(|preview| preview.track)
    }

    fn play_track_preview(
        &mut self,
        audio: Track,
        start_seconds: f64,
        generation: u64,
        cx: &mut Context<'_, Self>,
    ) {
        let preview = Preview {
            first: PreviewTrack {
                track: audio,
                start_seconds,
                tempo: 1.0,
            },
            second: None,
            second_offset_seconds: 0.0,
            key_lock: false,
        };
        let started = match &mut self.engine {
            Ok(engine) => engine
                .controller
                .preview(Some(preview))
                .map_err(|error| error.to_string()),
            Err(problem) => Err(problem.to_string()),
        };
        if let Err(problem) = started {
            self.end_track_preview(generation, cx);
            self.show_notice(
                NoticePlace::Window,
                format!("The preview could not play: {problem}"),
                cx,
            );
            return;
        }
        cx.notify();
        cx.spawn(async move |this, cx| {
            let longest = Duration::from_secs_f64(MAX_PREVIEW_SECONDS);
            let mut waited = Duration::ZERO;
            loop {
                cx.background_executor().timer(CHECK_INTERVAL * 5).await;
                waited += CHECK_INTERVAL * 5;
                let playing = this.update(cx, |this, cx| {
                    let playing = this.is_track_preview(generation)
                        && this
                            .engine
                            .as_ref()
                            .is_ok_and(|engine| engine.controller.preview_state().playing);
                    if !playing || waited > longest {
                        this.end_track_preview(generation, cx);
                        return false;
                    }
                    true
                });
                if !matches!(playing, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// Stops the track preview, in the engine too.
    pub(super) fn stop_track_preview(&mut self, cx: &mut Context<'_, Self>) {
        if self.track_preview.take().is_some() {
            if let Ok(engine) = &mut self.engine {
                engine.controller.preview(None).ok();
            }
            cx.notify();
        }
    }

    fn end_track_preview(&mut self, generation: u64, cx: &mut Context<'_, Self>) {
        if self.is_track_preview(generation) {
            self.stop_track_preview(cx);
        }
    }

    fn is_track_preview(&self, generation: u64) -> bool {
        self.track_preview
            .as_ref()
            .is_some_and(|preview| preview.generation == generation)
    }
}

/// Whether `track` is decoded to `seconds`, or as far as it goes.
fn decoded_to(track: &Track, seconds: f64) -> bool {
    match track.status() {
        DecodeStatus::Decoding => track.decoded_seconds() >= seconds,
        DecodeStatus::Complete | DecodeStatus::Failed(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use dubplate_rekordbox::{HotCue, TrackCues};

    use super::*;
    use crate::collection::TrackOrigin;

    #[test]
    fn a_preview_starts_at_the_first_cue_the_dj_marked() {
        let mut track = CollectionTrack::for_tests(
            dubplate_prep::file_track(std::path::Path::new("/Music/preview.mp3")),
            TrackOrigin::Folder,
        );
        assert_eq!(preview_start(&track), 0.0);
        let mut hot = <[Option<HotCue>; 8]>::default();
        hot[0] = Some(HotCue::at(48_000));
        track.cues = Arc::new(TrackCues::new(
            hot,
            vec![MemoryCue::at(64_000), MemoryCue::at(32_000)],
        ));
        assert_eq!(preview_start(&track), 32.0);
    }
}
