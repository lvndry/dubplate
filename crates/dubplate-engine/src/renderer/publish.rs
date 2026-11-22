//! Publishing what the audio thread knows to the control side, through atomics.

use std::sync::atomic::Ordering;

use crate::state::encode_loop_length;

use super::Renderer;

impl Renderer {
    pub(super) fn publish_state(&self) {
        for (deck, shared) in self.decks.iter().zip(&self.shared.decks) {
            shared.loaded.store(deck.is_loaded(), Ordering::Relaxed);
            shared.playing.store(deck.is_playing(), Ordering::Relaxed);
            shared.position_seconds.store(deck.position_seconds());
            shared.cue_point_seconds.store(deck.cue_point_seconds());
            shared.tempo.store(deck.tempo());
            shared.speed.store(deck.speed());
            shared.bpm.store(deck.bpm().unwrap_or(f64::NAN));
            shared
                .tempo_range
                .store(deck.tempo_range().index(), Ordering::Relaxed);
            shared
                .sync_mode
                .store(deck.sync_mode().index(), Ordering::Relaxed);
            shared
                .following
                .store(deck.is_following(), Ordering::Relaxed);
            shared
                .fader_waits_for_pickup
                .store(deck.fader_waits_for_pickup(), Ordering::Relaxed);
            shared
                .slip_enabled
                .store(deck.slip_enabled(), Ordering::Relaxed);
            shared.slip_shadow_seconds.store(
                deck.slip_shadow()
                    .map_or(f64::NAN, |frames| deck.frames_to_seconds(frames)),
            );
            let region = deck.loop_region();
            let seconds_or_none = |frames: Option<f64>| {
                frames.map_or(f64::NAN, |frames| deck.frames_to_seconds(frames))
            };
            shared
                .loop_start_seconds
                .store(seconds_or_none(region.map(|region| region.start)));
            shared
                .loop_end_seconds
                .store(seconds_or_none(region.map(|region| region.end)));
            shared.loop_active.store(
                region.is_some_and(|region| region.active),
                Ordering::Relaxed,
            );
            shared.loop_length.store(
                encode_loop_length(region.and_then(|region| region.length)),
                Ordering::Relaxed,
            );
            shared
                .pending_loop_start_seconds
                .store(seconds_or_none(deck.pending_loop_start()));
            shared.touched.store(deck.is_touched(), Ordering::Relaxed);
            shared
                .vinyl_mode
                .store(deck.vinyl_mode(), Ordering::Relaxed);
        }
        for (sounding, shared) in self
            .effects
            .sounding()
            .into_iter()
            .zip(&self.shared.effect_sounding)
        {
            shared.store(sounding, Ordering::Relaxed);
        }
        self.shared.tempo_master.store(self.tempo_master.current());
        let (playing, loaded) = self.sampler.playing_and_loaded();
        self.shared
            .sampler_playing
            .store(playing, Ordering::Relaxed);
        self.shared.sampler_loaded.store(loaded, Ordering::Relaxed);
        self.shared
            .preview_playing
            .store(self.preview.is_playing(), Ordering::Relaxed);
        self.shared
            .preview_position_seconds
            .store(self.preview.position_seconds());
    }
}
