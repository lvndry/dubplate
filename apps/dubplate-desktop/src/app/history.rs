//! The window's side of the play history: watching the decks for plays, keeping them in
//! Dubplate's database through the set worker, and saving a page of it as a tracklist where
//! the DJ chooses in the system's save dialog.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use dubplate_engine::DeckId;
use dubplate_prep::{Play, SetEditor, StoreError, default_export_folder};
use gpui::Context;

use super::DubplateApp;
use super::export::with_extension;
use super::notice::NoticePlace;
use crate::collection::TrackOrigin;
use crate::history::{
    Counted, DeckSample, Finished, HistoryPage, PlayEvent, SessionRecord, local_time, play_deck,
};
use crate::tracklist::{TracklistFormat, tracklist};

/// How often the decks are sampled for the history: often enough that a minute is a minute to
/// within half a second.
const PLAY_SAMPLE_INTERVAL: Duration = Duration::from_millis(500);

impl DubplateApp {
    /// Samples the decks for the history every half second for as long as the window lives.
    pub(super) fn keep_watching_plays(cx: &mut Context<'_, Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(PLAY_SAMPLE_INTERVAL).await;
                if this.update(cx, |this, cx| this.watch_plays(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn watch_plays(&mut self, cx: &mut Context<'_, Self>) {
        let at = Instant::now();
        let wall_clock = SystemTime::now();
        for deck in DeckId::ALL {
            let state = self.deck_state(deck);
            let sample = DeckSample {
                load: self.deck(deck).loaded_generation(),
                playing: state.playing,
                tempo: state.tempo,
                at,
                wall_clock,
            };
            match self.play_watches[deck.index()].observe(sample) {
                Some(PlayEvent::Counted(counted)) => self.record_play(deck, counted, cx),
                Some(PlayEvent::Finished(finished)) => self.finish_play(finished, cx),
                None => {}
            }
        }
    }

    /// Keeps a play that has just counted, in the history at once and in the database through
    /// the set worker.
    fn record_play(&mut self, deck: DeckId, counted: Counted, cx: &mut Context<'_, Self>) {
        let (Some(track), Some(deck_letter)) = (self.deck(deck).track.clone(), play_deck(deck))
        else {
            return;
        };
        let Some(track_path) = track.track.file_path.clone() else {
            return;
        };
        let set = self.set_editor.as_ref().and_then(|session| {
            let set = session.set();
            set.entries
                .iter()
                .any(|entry| entry.track_path == track_path)
                .then_some(set.id)
        });
        let play = Play {
            set,
            track_path,
            rekordbox_id: match track.origin {
                TrackOrigin::Rekordbox => Some(track.track.id.clone()),
                TrackOrigin::Folder | TrackOrigin::File => None,
            },
            title: track.row.title.to_string(),
            artist: track.row.artist.to_string(),
            started_at: counted.started_at,
            played: counted.played,
            deck: deck_letter,
        };
        let record = self.play_history.add(play.clone());
        let stored = Arc::clone(&record.stored);
        self.play_watches[deck.index()].counted_as(record);
        self.save_history(
            move |editor| {
                let id = editor.record_play(&play)?;
                let _ = stored.set(id);
                Ok(())
            },
            cx,
        );
        self.history_changed(cx);
    }

    /// Keeps how long a counted play lasted once it leaves its deck.
    fn finish_play(&mut self, finished: Finished<SessionRecord>, cx: &mut Context<'_, Self>) {
        self.play_history
            .set_length(&finished.record, finished.played);
        let stored = finished.record.stored;
        let played = finished.played;
        self.save_history(
            move |editor| match stored.get() {
                Some(id) => editor.set_play_length(*id, played),
                None => Ok(()),
            },
            cx,
        );
    }

    /// Keeps the length of the plays still on the decks, as Dubplate quits.
    pub(super) fn finish_plays(&mut self, cx: &mut Context<'_, Self>) {
        for deck in DeckId::ALL {
            if let Some(finished) = self.play_watches[deck.index()].finish() {
                self.finish_play(finished, cx);
            }
        }
    }

    /// Hands the history's database work to the set worker, which keeps it in order with the
    /// set's edits and finishes it before Dubplate quits, and reports a failure.
    fn save_history(
        &mut self,
        work: impl FnOnce(&mut SetEditor) -> Result<(), StoreError> + Send + 'static,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(session) = &self.set_editor else {
            return;
        };
        let job = match session.submit(work) {
            Ok(job) => job,
            Err(error) => {
                self.report_history_failure(error, cx);
                return;
            }
        };
        cx.spawn(async move |this, cx| {
            let answer = cx
                .background_executor()
                .spawn(async move { job.wait() })
                .await;
            let problem = match answer {
                Ok(completion) => completion.outcome.err().map(|error| error.to_string()),
                Err(error) => Some(error.to_string()),
            };
            if let Some(problem) = problem {
                this.update(cx, |this, cx| this.report_history_failure(problem, cx))
                    .ok();
            }
        })
        .detach();
    }

    fn report_history_failure(
        &mut self,
        error: impl std::fmt::Display,
        cx: &mut Context<'_, Self>,
    ) {
        self.show_notice(
            NoticePlace::Window,
            format!("The play history was not saved: {error}"),
            cx,
        );
    }

    /// Reads the plays of earlier sessions once the database is open.
    pub(super) fn restore_play_history(&mut self, cx: &mut Context<'_, Self>) {
        let Some(session) = &self.set_editor else {
            return;
        };
        let job = match session.submit(|editor| editor.play_history()) {
            Ok(job) => job,
            Err(error) => {
                self.report_history_failure(error, cx);
                return;
            }
        };
        cx.spawn(async move |this, cx| {
            let answer = cx
                .background_executor()
                .spawn(async move { job.wait() })
                .await;
            this.update(cx, |this, cx| {
                match answer
                    .map_err(|error| error.to_string())
                    .and_then(|completion| completion.outcome.map_err(|error| error.to_string()))
                {
                    Ok(plays) => {
                        this.play_history
                            .restore_earlier(plays.into_iter().map(|recorded| recorded.play));
                        this.history_changed(cx);
                    }
                    Err(problem) => this.show_notice(
                        NoticePlace::Window,
                        format!("The play history could not be read: {problem}"),
                        cx,
                    ),
                }
            })
            .ok();
        })
        .detach();
    }

    /// Shows the history as it now stands in the browser.
    fn history_changed(&mut self, cx: &mut Context<'_, Self>) {
        let shelf = Arc::new(self.play_history.shelf(&self.time_zone));
        let played: HashSet<PathBuf> = self
            .play_history
            .played_paths()
            .map(Path::to_path_buf)
            .collect();
        let played = Arc::new(played);
        self.browser.update(cx, |browser, cx| {
            browser.set_history(shelf, cx);
            browser.set_played(played, cx);
        });
    }

    /// Asks where to save a page of the history as a tracklist, starting in
    /// `~/Music/Dubplate Exports`, and writes it there in the background.
    pub(super) fn export_history(
        &mut self,
        page: HistoryPage,
        format: TracklistFormat,
        cx: &mut Context<'_, Self>,
    ) {
        for watch in &self.play_watches {
            if let Some(record) = watch.record() {
                self.play_history.set_length(record, watch.played());
            }
        }
        let plays = self.play_history.plays(page, &self.time_zone);
        let Some(first) = plays.first() else {
            self.show_notice(
                NoticePlace::Window,
                "No track has played long enough to be in this history yet.",
                cx,
            );
            return;
        };
        let file_name = format!(
            "Dubplate history {}.{}",
            local_time(first.started_at, &self.time_zone).strftime("%Y-%m-%d %H-%M"),
            format.extension()
        );
        let contents = tracklist(format, &plays, &self.time_zone);
        let folder = default_export_folder()
            .filter(|folder| std::fs::create_dir_all(folder).is_ok())
            .or_else(dirs::home_dir)
            .unwrap_or_default();
        let chosen = cx.prompt_for_new_path(&folder, Some(&file_name));
        cx.spawn(async move |this, cx| {
            let path = match chosen.await {
                Ok(Ok(Some(path))) => with_extension(path, format.extension()),
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(error)) => {
                    this.update(cx, |this, cx| {
                        this.show_notice(
                            NoticePlace::Window,
                            format!("The save dialog could not open: {error}"),
                            cx,
                        );
                    })
                    .ok();
                    return;
                }
            };
            let written = cx
                .background_executor()
                .spawn({
                    let path = path.clone();
                    async move { std::fs::write(&path, contents) }
                })
                .await;
            let saved_name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            this.update(cx, |this, cx| {
                let notice = match written {
                    Ok(()) => format!("Saved the tracklist as {saved_name}."),
                    Err(error) => format!("The tracklist could not be saved: {error}"),
                };
                this.show_notice(NoticePlace::Window, notice, cx);
            })
            .ok();
        })
        .detach();
    }
}
