//! The window's side of seam auditions: S on a set track plays the seam after it in the
//! headphones for twenty seconds, through the engine's preview bus, never on the master.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use dubplate_engine::{DecodeStatus, Preview, PreviewTrack, Track};
use gpui::Context;

use super::DubplateApp;
use super::notice::NoticePlace;
use crate::audition::{
    AUDITION_LENGTH, AuditionPlan, AuditionProblem, DEFAULT_OVERLAP_BARS, SeamSide, plan_audition,
};

/// How often a decoding track is looked at, and a playing audition checked.
const CHECK_INTERVAL: Duration = Duration::from_millis(50);
/// Decoded audio wanted past the audition's end before it starts, for tempo changes.
const DECODE_MARGIN_SECONDS: f64 = 2.0;

/// A seam being auditioned.
pub(super) struct Audition {
    /// The set track before the seam.
    pub after: usize,
    /// When it started playing, once both tracks were decoded far enough.
    pub started: Option<Instant>,
    generation: u64,
}

impl DubplateApp {
    /// Auditions the seam after the selected set track, or stops the audition playing.
    pub(super) fn toggle_seam_audition(&mut self, cx: &mut Context<'_, Self>) {
        if self.audition.is_some() {
            self.stop_audition(cx);
            return;
        }
        let Some(after) = self.set_selection else {
            self.show_notice(
                NoticePlace::Window,
                "Select the set track before the seam to hear it.",
                cx,
            );
            return;
        };
        self.start_audition(after, cx);
    }

    /// Auditions the seam after set track `after`, or stops it when it is the one playing.
    pub(super) fn toggle_audition_of(&mut self, after: usize, cx: &mut Context<'_, Self>) {
        let playing_this = self
            .audition
            .as_ref()
            .is_some_and(|audition| audition.after == after);
        self.stop_audition(cx);
        if !playing_this {
            self.start_audition(after, cx);
        }
    }

    fn start_audition(&mut self, after: usize, cx: &mut Context<'_, Self>) {
        match self.audition_request(after) {
            Ok((paths, plan)) => {
                self.audition_generation += 1;
                let generation = self.audition_generation;
                self.audition = Some(Audition {
                    after,
                    started: None,
                    generation,
                });
                self.decode_for_audition(paths, plan, generation, cx);
            }
            Err(problem) => self.show_notice(NoticePlace::Window, problem, cx),
        }
        cx.notify();
    }

    /// The two files and the plan for the seam after set track `after`, or what stops it.
    fn audition_request(&self, after: usize) -> Result<([PathBuf; 2], AuditionPlan), String> {
        let headphones = self
            .engine
            .as_ref()
            .is_ok_and(|engine| engine.output.has_headphone_output());
        if !headphones {
            return Err(
                "Seams play in the headphones only: choose a sound card with a \
                        headphone output, like the FLX4's, in Settings."
                    .to_owned(),
            );
        }
        let editor = self.set_editor.as_ref().ok_or("There is no set.")?;
        let tracks = editor.tracks();
        let (Some(outgoing), Some(incoming)) = (tracks.get(after), tracks.get(after + 1)) else {
            return Err("The last track has no seam after it.".to_owned());
        };
        let side = |path: &Path, cue_ms| SeamSide {
            bpm: self.catalogue.get(path).and_then(|track| track.bpm),
            cue_ms,
        };
        let overlap_bars = editor
            .set()
            .seam_after(after)
            .map_or(DEFAULT_OVERLAP_BARS, |seam| seam.overlap_bars);
        let plan = plan_audition(
            side(&outgoing.track_path, outgoing.mix_out_ms),
            side(&incoming.track_path, incoming.mix_in_ms),
            overlap_bars,
        )
        .map_err(|problem| {
            let (entry, cue, key) = match problem {
                AuditionProblem::NoMixOut => (outgoing, "mix-out", "O"),
                AuditionProblem::NoMixIn => (incoming, "mix-in", "I"),
            };
            let title = self
                .catalogue
                .get(&entry.track_path)
                .map_or_else(|| "the track".to_owned(), |track| track.title.clone());
            format!("Set the {cue} of {title} first: {key} puts it at the playhead.")
        })?;
        Ok((
            [outgoing.track_path.clone(), incoming.track_path.clone()],
            plan,
        ))
    }

    /// Opens both files, waits until each is decoded past its part of the audition, and starts
    /// the preview, unless the audition was stopped meanwhile.
    fn decode_for_audition(
        &mut self,
        paths: [PathBuf; 2],
        plan: AuditionPlan,
        generation: u64,
        cx: &mut Context<'_, Self>,
    ) {
        cx.spawn(async move |this, cx| {
            let tracks = cx
                .background_executor()
                .spawn(async move {
                    let [outgoing_path, incoming_path] = paths;
                    let outgoing = dubplate_engine::decode_file(&outgoing_path)
                        .map_err(|error| error.to_string())?;
                    let incoming = dubplate_engine::decode_file(&incoming_path)
                        .map_err(|error| error.to_string())?;
                    Ok::<_, String>((outgoing, incoming))
                })
                .await;
            let (outgoing, incoming) = match tracks {
                Ok(tracks) => tracks,
                Err(problem) => {
                    this.update(cx, |this, cx| {
                        this.end_audition(generation, cx);
                        this.show_notice(
                            NoticePlace::Window,
                            format!("The seam could not be opened: {problem}"),
                            cx,
                        );
                    })
                    .ok();
                    return;
                }
            };
            let length = AUDITION_LENGTH.as_secs_f64();
            let outgoing_needed = plan.outgoing_start_seconds + length + DECODE_MARGIN_SECONDS;
            let incoming_needed = plan.incoming_start_seconds
                + (length - plan.incoming_offset_seconds).max(0.0) * plan.incoming_tempo
                + DECODE_MARGIN_SECONDS;
            while !(decoded_to(&outgoing, outgoing_needed)
                && decoded_to(&incoming, incoming_needed))
            {
                cx.background_executor().timer(CHECK_INTERVAL).await;
                let current = this
                    .update(cx, |this, _| this.is_audition(generation))
                    .unwrap_or(false);
                if !current {
                    return;
                }
            }
            this.update(cx, |this, cx| {
                if this.is_audition(generation) {
                    this.play_audition(outgoing, incoming, plan, generation, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn play_audition(
        &mut self,
        outgoing: Track,
        incoming: Track,
        plan: AuditionPlan,
        generation: u64,
        cx: &mut Context<'_, Self>,
    ) {
        let preview = seam_preview(outgoing, incoming, plan);
        let started = match &mut self.engine {
            Ok(engine) => engine
                .controller
                .preview(Some(preview))
                .map_err(|error| error.to_string()),
            Err(problem) => Err(problem.to_string()),
        };
        if let Err(problem) = started {
            self.end_audition(generation, cx);
            self.show_notice(
                NoticePlace::Window,
                format!("The seam could not play: {problem}"),
                cx,
            );
            return;
        }
        if let Some(audition) = &mut self.audition {
            audition.started = Some(Instant::now());
        }
        cx.notify();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(CHECK_INTERVAL * 5).await;
                let playing = this.update(cx, |this, cx| this.check_audition(generation, cx));
                if !matches!(playing, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// Ends the audition after its twenty seconds or when the preview has stopped on its own.
    /// Returns whether it still plays.
    fn check_audition(&mut self, generation: u64, cx: &mut Context<'_, Self>) -> bool {
        let Some(started) = self
            .audition
            .as_ref()
            .filter(|audition| audition.generation == generation)
            .and_then(|audition| audition.started)
        else {
            return false;
        };
        let preview_playing = self
            .engine
            .as_ref()
            .is_ok_and(|engine| engine.controller.preview_state().playing);
        let elapsed = started.elapsed();
        if elapsed >= AUDITION_LENGTH || (!preview_playing && elapsed > CHECK_INTERVAL * 10) {
            self.stop_audition(cx);
            return false;
        }
        cx.notify();
        true
    }

    /// Stops the audition, in the engine too.
    pub(super) fn stop_audition(&mut self, cx: &mut Context<'_, Self>) {
        if self.audition.take().is_some() {
            if let Ok(engine) = &mut self.engine {
                engine.controller.preview(None).ok();
            }
            cx.notify();
        }
    }

    fn end_audition(&mut self, generation: u64, cx: &mut Context<'_, Self>) {
        if self.is_audition(generation) {
            self.stop_audition(cx);
        }
    }

    fn is_audition(&self, generation: u64) -> bool {
        self.audition
            .as_ref()
            .is_some_and(|audition| audition.generation == generation)
    }
}

/// The preview of a seam: the outgoing track at its own tempo, the incoming one matched to it
/// and coming in as planned, both with key lock so neither changes key.
fn seam_preview(outgoing: Track, incoming: Track, plan: AuditionPlan) -> Preview {
    Preview {
        first: PreviewTrack {
            track: outgoing,
            start_seconds: plan.outgoing_start_seconds,
            tempo: 1.0,
        },
        second: PreviewTrack {
            track: incoming,
            start_seconds: plan.incoming_start_seconds,
            tempo: plan.incoming_tempo,
        },
        second_offset_seconds: plan.incoming_offset_seconds,
        key_lock: true,
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
    use dubplate_engine::{DeckId, offline_engine};

    use super::*;
    use crate::audition::SeamSide;

    const SAMPLE_RATE: u32 = 48_000;

    fn track(seconds: usize) -> Track {
        let samples = vec![0.2; SAMPLE_RATE as usize * seconds * 2];
        Track::from_interleaved_stereo(&samples, SAMPLE_RATE).unwrap()
    }

    #[test]
    fn a_seam_plays_on_the_preview_bus_and_leaves_the_decks_alone() {
        let plan = plan_audition(
            SeamSide {
                bpm: Some(120.0),
                cue_ms: Some(40_000),
            },
            SeamSide {
                bpm: Some(124.0),
                cue_ms: Some(2_000),
            },
            8,
        )
        .unwrap();
        let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
        controller
            .preview(Some(seam_preview(track(60), track(60), plan)))
            .unwrap();
        let mut block = vec![0.0; 480 * 2];
        for _ in 0..10 {
            renderer.render(&mut block, 2);
        }
        let state = controller.preview_state();
        assert!(state.playing, "{state:?}");
        assert!(state.position_seconds > 0.0);
        assert!(
            block.iter().all(|sample| *sample == 0.0),
            "nothing on the master"
        );
        for deck in DeckId::ALL {
            assert!(!controller.deck_state(deck).playing);
        }
        controller.preview(None).unwrap();
        renderer.render(&mut block, 2);
        assert!(!controller.preview_state().playing);
    }

    #[test]
    fn a_fully_decoded_track_is_ready_for_any_audition() {
        let complete = track(1);
        assert!(
            decoded_to(&complete, 100.0),
            "a decoded track goes as far as it goes"
        );
    }
}
