//! The cues the DJ makes on the decks: kept in Dubplate's own database as each track's whole
//! list, in place of rekordbox's, so they are there again the next time the track loads, after
//! a relaunch too, and go into the export to rekordbox. rekordbox's own files are never written.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use dubplate_engine::DeckId;
use dubplate_prep::TrackIdentity;
use dubplate_rekordbox::{TrackCues, cue_milliseconds};
use gpui::Context;

use super::DubplateApp;
use super::notice::NoticePlace;
use crate::collection::CollectionTrack;

/// Where a cue set now goes, in milliseconds, with its loop end: the loop that is on, as a
/// saved loop, else a cue point at `seconds`.
pub(super) fn new_cue_span(active_loop: Option<(f64, f64)>, seconds: f64) -> (u32, Option<u32>) {
    match active_loop {
        Some((start, end)) => (cue_milliseconds(start), Some(cue_milliseconds(end))),
        None => (cue_milliseconds(seconds), None),
    }
}

impl DubplateApp {
    /// The cues `track` loads with: the ones the DJ made in Dubplate, else rekordbox's.
    pub(super) fn cues_of(&self, track: &CollectionTrack) -> Arc<TrackCues> {
        self.track_cues
            .get(&track.identity())
            .map_or_else(|| Arc::clone(&track.cues), Arc::clone)
    }

    /// Takes the cues read from Dubplate's database, and gives them to the decks holding those
    /// tracks, as a track can be loaded before the database has opened.
    pub(super) fn restore_track_cues(&mut self, saved: HashMap<TrackIdentity, TrackCues>) {
        for (identity, cues) in saved {
            self.track_cues.insert(identity, Arc::new(cues));
        }
        for side in &mut self.decks {
            if let Some(cues) = side
                .track
                .as_ref()
                .and_then(|track| self.track_cues.get(&track.identity()))
            {
                side.cues = Arc::clone(cues);
            }
        }
    }

    /// Changes the cues of the track on `deck`, on every deck holding that track, and keeps the
    /// track's whole list in Dubplate's database. Nothing happens on a deck still loading, nor
    /// before the database has opened, as the cue could not be kept.
    pub(super) fn edit_cues(
        &mut self,
        deck: DeckId,
        edit: impl FnOnce(&mut TrackCues),
        cx: &mut Context<'_, Self>,
    ) {
        if self.set_editor.is_none() {
            self.show_notice(
                NoticePlace::Deck(deck),
                "Cues can be set once Dubplate's database has opened.",
                cx,
            );
            return;
        }
        let side = self.deck(deck);
        let Some(track) = side.track.clone() else {
            return;
        };
        if side.visuals().is_none() {
            return;
        }
        let mut cues = TrackCues::clone(&side.cues);
        edit(&mut cues);
        if cues == *side.cues {
            return;
        }
        let cues = Arc::new(cues);
        let identity = track.identity();
        for other in &mut self.decks {
            if other
                .track
                .as_ref()
                .is_some_and(|loaded| loaded.identity() == identity)
            {
                other.cues = Arc::clone(&cues);
            }
        }
        self.track_cues.insert(identity.clone(), Arc::clone(&cues));
        self.save_track_cues(identity, track.track.file_path.clone(), cues, cx);
    }

    /// Writes a track's cues on the database's worker, after every edit asked for before, so
    /// the last list always wins and quitting waits for it.
    fn save_track_cues(
        &mut self,
        identity: TrackIdentity,
        track_path: Option<PathBuf>,
        cues: Arc<TrackCues>,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(session) = &self.set_editor else {
            return;
        };
        let job = match session
            .submit(move |editor| editor.save_track_cues(&identity, track_path.as_deref(), &cues))
        {
            Ok(job) => job,
            Err(error) => {
                self.report_cue_save_failure(error, cx);
                return;
            }
        };
        self.set_edits_pending += 1;
        cx.spawn(async move |this, cx| {
            let answer = cx
                .background_executor()
                .spawn(async move { job.wait() })
                .await;
            this.update(cx, |this, cx| {
                this.set_edits_pending = this.set_edits_pending.saturating_sub(1);
                match answer.map(|completion| completion.outcome) {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => this.report_cue_save_failure(error, cx),
                    Err(error) => this.report_cue_save_failure(error, cx),
                }
                this.finish_quit_if_saved(cx);
            })
            .ok();
        })
        .detach();
    }

    /// Says a cue was not saved, and keeps Dubplate open once on quitting so the DJ sees it.
    fn report_cue_save_failure(
        &mut self,
        error: impl std::fmt::Display,
        cx: &mut Context<'_, Self>,
    ) {
        self.set_save_problem = Some(error.to_string());
        self.show_notice(
            NoticePlace::Window,
            format!("A cue was not saved: {error}"),
            cx,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cue_set_during_a_loop_keeps_the_loop_and_otherwise_the_point() {
        assert_eq!(new_cue_span(Some((8.0, 10.0)), 3.0), (8_000, Some(10_000)));
        assert_eq!(new_cue_span(None, 3.0004), (3_000, None));
    }
}
