//! Auto Cue: a freshly loaded track waits at its earliest memory or hot cue, else at its first
//! sound on the beat it starts with, as rekordbox's Preferences > Controller > Deck set it.

use std::time::Duration;

use dubplate_engine::{DeckId, Track};
use dubplate_rekordbox::MemoryCue;
use gpui::Context;

use super::DubplateApp;
use crate::auto_cue::{FirstSound, cue_at_first_sound, earliest_saved_cue, first_sound};

/// How long Auto Cue waits for more of the track to decode before it looks again.
const DECODE_WAIT: Duration = Duration::from_millis(40);
/// How far the DJ may have moved a deck from the start before Auto Cue leaves it alone.
const UNTOUCHED_SECONDS: f64 = 0.01;

impl DubplateApp {
    /// Cues the track just loaded on `deck` as the settings say. Looking for the first sound
    /// runs in the background while the track decodes; it gives up if the deck loads another
    /// track, plays, or is moved first.
    pub(super) fn auto_cue(
        &mut self,
        deck: DeckId,
        generation: u64,
        audio: Option<Track>,
        cx: &mut Context<'_, Self>,
    ) {
        let setting = self.settings.auto_cue;
        let side = self.deck(deck);
        if setting.saved_cues_first {
            let memory_cues = side.cues.memory_cues().iter().map(MemoryCue::start_seconds);
            let hot_cues = side.cues.hot_cues().map(|(_, cue)| cue.start_seconds());
            if let Some(seconds) = earliest_saved_cue(memory_cues, hot_cues) {
                self.jump(deck, seconds, cx);
                return;
            }
        }
        let (true, Some(audio)) = (setting.first_sound, audio) else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let mut from_frame = 0;
            loop {
                let searched = audio.clone();
                let found = cx
                    .background_executor()
                    .spawn(async move { first_sound(&searched, from_frame) })
                    .await;
                match found {
                    FirstSound::At(seconds) => {
                        this.update(cx, |this, cx| {
                            this.place_auto_cue(deck, generation, seconds, cx);
                        })
                        .ok();
                        return;
                    }
                    FirstSound::Silent => return,
                    FirstSound::Pending(next) => from_frame = next,
                }
                cx.background_executor().timer(DECODE_WAIT).await;
                let waiting =
                    this.update(cx, |this, _| this.untouched_since_load(deck, generation));
                if !matches!(waiting, Ok(true)) {
                    return;
                }
            }
        })
        .detach();
    }

    fn place_auto_cue(
        &mut self,
        deck: DeckId,
        generation: u64,
        first_sound: f64,
        cx: &mut Context<'_, Self>,
    ) {
        if !self.untouched_since_load(deck, generation) {
            return;
        }
        let beats = self
            .deck(deck)
            .visuals()
            .map(|visuals| visuals.beats.clone());
        let seconds = cue_at_first_sound(first_sound, beats.as_deref());
        self.jump(deck, seconds, cx);
        cx.notify();
    }

    /// Whether `deck` still holds the track of load `generation`, stopped where it loaded.
    fn untouched_since_load(&self, deck: DeckId, generation: u64) -> bool {
        let state = self.deck_state(deck);
        self.deck(deck).is_current_load(generation)
            && !state.playing
            && state.position_seconds <= UNTOUCHED_SECONDS
            && state.cue_point_seconds <= UNTOUCHED_SECONDS
    }
}
