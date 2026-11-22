//! Waveforms for the tracks rekordbox has none for, drawn once from their audio and shared:
//! the deck draws a loaded track's once its audio is decoded, the track check draws a set
//! track's from its file, and whichever comes second takes the first one's drawing.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use dubplate_engine::{DeckId, decode_file};
use gpui::Context;

use super::DubplateApp;
use crate::deck::DeckContent;
use crate::prep::TrackDetail;
use crate::waveform_cache::{DrawnWaveforms, decoded};

/// How often decoding is checked while a waveform waits for it.
const DECODE_CHECK_INTERVAL: Duration = Duration::from_millis(100);

impl DubplateApp {
    /// Shows the waveforms of the track loaded on `deck` by load `generation`, drawn from its
    /// audio once it is decoded, or at once when they are drawn already.
    pub(super) fn draw_waveform_from_audio(
        &mut self,
        deck: DeckId,
        generation: u64,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(path) = self
            .deck(deck)
            .track
            .as_ref()
            .and_then(|track| track.track.file_path.clone())
        else {
            return;
        };
        if let Some(drawn) = self.waveform_cache.get(&path) {
            self.show_drawn_waveforms(&path, &drawn, cx);
            return;
        }
        let Some(audio) = self
            .engine
            .as_ref()
            .ok()
            .and_then(|engine| engine.controller.track(deck).cloned())
        else {
            return;
        };
        if !self.waveform_cache.claim(&path) {
            return;
        }
        cx.spawn(async move |this, cx| {
            while !decoded(&audio) {
                cx.background_executor().timer(DECODE_CHECK_INTERVAL).await;
                let still_loaded = this
                    .update(cx, |this, _| {
                        this.deck(deck).is_loaded_generation(generation)
                    })
                    .unwrap_or(false);
                if !still_loaded {
                    this.update(cx, |this, _| this.waveform_cache.abandon(&path))
                        .ok();
                    return;
                }
            }
            let drawn = cx
                .background_executor()
                .spawn(async move { DrawnWaveforms::draw(&audio) })
                .await;
            this.update(cx, |this, cx| this.waveforms_drawn(path, drawn, cx))
                .ok();
        })
        .detach();
    }

    /// Draws the waveforms of the set track at `path` for the track check, from its file, when
    /// neither rekordbox nor a deck has drawn them.
    pub(super) fn draw_check_waveforms(&mut self, path: PathBuf, cx: &mut Context<'_, Self>) {
        if let Some(drawn) = self.waveform_cache.get(&path) {
            self.show_drawn_waveforms(&path, &drawn, cx);
            return;
        }
        if !self.waveform_cache.claim(&path) {
            return;
        }
        cx.spawn(async move |this, cx| {
            let file = path.clone();
            let opened = cx
                .background_executor()
                .spawn(async move { decode_file(&file) })
                .await;
            let Ok(audio) = opened else {
                this.update(cx, |this, _| this.waveform_cache.abandon(&path))
                    .ok();
                return;
            };
            while !decoded(&audio) {
                cx.background_executor().timer(DECODE_CHECK_INTERVAL).await;
            }
            let drawn = cx
                .background_executor()
                .spawn(async move { DrawnWaveforms::draw(&audio) })
                .await;
            this.update(cx, |this, cx| this.waveforms_drawn(path, drawn, cx))
                .ok();
        })
        .detach();
    }

    /// Keeps a drawing for the session and shows it wherever the track is.
    fn waveforms_drawn(
        &mut self,
        path: PathBuf,
        drawn: Option<DrawnWaveforms>,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(drawn) = drawn else {
            self.waveform_cache.abandon(&path);
            return;
        };
        let drawn = Arc::new(drawn);
        self.waveform_cache.finish(path.clone(), Arc::clone(&drawn));
        self.show_drawn_waveforms(&path, &drawn, cx);
    }

    /// Shows `drawn` on every deck playing the track at `path` and in its track check, where
    /// they have no waveform yet.
    fn show_drawn_waveforms(
        &mut self,
        path: &Path,
        drawn: &DrawnWaveforms,
        cx: &mut Context<'_, Self>,
    ) {
        for side in &mut self.decks {
            let holds_track = side
                .track
                .as_ref()
                .is_some_and(|track| track.track.file_path.as_deref() == Some(path));
            if !holds_track {
                continue;
            }
            if let DeckContent::Loaded(visuals) = &mut side.content
                && visuals.zoomed.is_none()
            {
                visuals.show_waveforms(Arc::clone(&drawn.zoomed), drawn.overview.clone());
            }
        }
        if let Some((detail_path, detail)) = &mut self.track_detail
            && detail_path.as_path() == path
            && detail.zoomed.is_none()
        {
            *detail = Arc::new(TrackDetail {
                zoomed: Some(Arc::clone(&drawn.zoomed)),
                overview: drawn.overview.clone(),
                ..(**detail).clone()
            });
        }
        cx.notify();
    }
}
