//! Waveforms for the tracks rekordbox has none for: once the deck has decoded the whole track,
//! its 3-band waveform is worked out from the audio on a background thread and drawn as
//! rekordbox's would be.

use std::time::Duration;

use dubplate_engine::{DeckId, DecodeStatus};
use gpui::Context;

use super::DubplateApp;
use crate::audio_waveform::three_band_waveforms;
use crate::deck::DeckContent;
use crate::waveform::{OverviewWaveform, Zoom, ZoomedWaveform};

/// How often the deck's decoding is checked while the waveform waits for it.
const DECODE_CHECK_INTERVAL: Duration = Duration::from_millis(100);

impl DubplateApp {
    /// Draws the waveforms of the track loaded on `deck` by load `generation` from its audio,
    /// once it is decoded, unless another track has been loaded meanwhile.
    pub(super) fn draw_waveform_from_audio(
        &mut self,
        deck: DeckId,
        generation: u64,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(audio) = self
            .engine
            .as_ref()
            .ok()
            .and_then(|engine| engine.controller.track(deck).cloned())
        else {
            return;
        };
        cx.spawn(async move |this, cx| {
            while audio.status() == DecodeStatus::Decoding {
                cx.background_executor().timer(DECODE_CHECK_INTERVAL).await;
                let still_loaded = this
                    .update(cx, |this, _| this.deck(deck).is_current_load(generation))
                    .unwrap_or(false);
                if !still_loaded {
                    return;
                }
            }
            let (zoomed, overview) = cx
                .background_executor()
                .spawn(async move {
                    let waveforms = three_band_waveforms(&audio);
                    (
                        ZoomedWaveform::from_waveforms(&waveforms, Zoom::DEFAULT),
                        OverviewWaveform::from_waveforms(&waveforms),
                    )
                })
                .await;
            this.update(cx, |this, cx| {
                let Some(zoomed) = zoomed else {
                    return;
                };
                if !this.deck(deck).is_current_load(generation) {
                    return;
                }
                if let DeckContent::Loaded(visuals) = &mut this.decks[deck.index()].content {
                    visuals.show_waveforms(zoomed, overview);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }
}
