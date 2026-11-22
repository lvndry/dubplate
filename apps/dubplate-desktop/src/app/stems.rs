//! The window's side of stems: asking for the set's and the shortlist's separations, taking the
//! statuses as they arrive, giving each deck its track's stems once they are ready, and the
//! mutes, levels, instrumental and acapella the DJ plays with.

use std::path::PathBuf;
use std::time::Duration;

use dubplate_engine::{DeckId, Stem, StemLevel, StemSettings, channel_fader_gain};
use dubplate_stems::{SeparationPriority, StemFiles, StemsStatus};
use dubplate_midi::PadMode;
use gpui::Context;

use super::DubplateApp;
use super::notice::NoticePlace;
use crate::collection::TrackIndex;
use crate::commands::{StemAction, is_groove};

/// How often the separation queue's reports are taken.
const COLLECT_INTERVAL: Duration = Duration::from_millis(400);

/// What SHIFT + a pad does in BEAT LOOP mode, where rekordbox maps nothing on the DDJ-FLX4:
/// the top row mutes or brings back the vocals, the melody, the bass and the drums, in the order
/// of the STEMS pads of Pioneer's DDJ-GRV6 and of Serato's stems pads on the FLX4; the bottom
/// row plays the acapella, the instrumental, the groove of drums and bass, and every stem back.
pub(super) fn stem_pad_action(pad: usize) -> Option<StemAction> {
    Some(match pad {
        0 => StemAction::Toggle(Stem::Vocals),
        1 => StemAction::Toggle(Stem::Melody),
        2 => StemAction::Toggle(Stem::Bass),
        3 => StemAction::Toggle(Stem::Drums),
        4 => StemAction::Acapella,
        5 => StemAction::Instrumental,
        6 => StemAction::Groove,
        7 => StemAction::AllStems,
        _ => return None,
    })
}

impl DubplateApp {
    /// Asks for the stems of the set's tracks, then of its shortlist, so they are separated
    /// while the set is prepared.
    pub(super) fn request_set_stems(&mut self) {
        let Some(editor) = &self.set_editor else {
            return;
        };
        let set = editor
            .tracks()
            .into_iter()
            .map(|entry| (entry.track_path.clone(), SeparationPriority::Set));
        let shortlist = editor
            .backups()
            .into_iter()
            .map(|entry| (entry.track_path.clone(), SeparationPriority::Shortlist));
        let requests: Vec<(PathBuf, SeparationPriority)> = set.chain(shortlist).collect();
        self.stems.follow_set(requests);
    }

    /// Separates `track` before every other track waiting, as the DJ asked.
    pub(super) fn separate_stems_now(&mut self, track: PathBuf, cx: &mut Context<'_, Self>) {
        self.stems.request([(track, SeparationPriority::Now)]);
        cx.notify();
    }

    /// Stops separating `track`, or forgets it if it waits.
    pub(super) fn cancel_stems(&mut self, track: PathBuf, cx: &mut Context<'_, Self>) {
        self.stems.cancel(track);
        cx.notify();
    }

    /// Asks which tracks of the library have stems already, from Dubplate's own record, without
    /// reading a single track.
    pub(super) fn probe_library_stems(&mut self) {
        let tracks: Vec<PathBuf> = self
            .catalogue
            .tracks
            .iter()
            .map(|track| track.track_path.clone())
            .collect();
        self.stems.probe(tracks);
    }

    /// Takes the separation queue's reports for as long as the window is open: statuses go to
    /// the library and the set, and stems that become ready go to the decks playing their track.
    pub(super) fn keep_collecting_stems(cx: &mut Context<'_, Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(COLLECT_INTERVAL).await;
                let open = this.update(cx, |this, cx| this.collect_stems(cx)).is_ok();
                if !open {
                    return;
                }
            }
        })
        .detach();
    }

    fn collect_stems(&mut self, cx: &mut Context<'_, Self>) {
        let ready = self.stems.collect();
        if ready.is_empty() && !self.stems.is_busy() && !self.stems_changed {
            return;
        }
        self.stems_changed = self.stems.is_busy();
        let statuses = self.stems.statuses();
        self.browser
            .update(cx, |browser, cx| browser.set_stems(statuses, cx));
        for deck in DeckId::ALL {
            let playing = self.deck_track_path(deck);
            if playing.is_some_and(|path| ready.contains(&path)) {
                self.give_deck_stems(deck, cx);
            }
        }
        self.measure_vocals(ready, cx);
        self.refresh_prepare_view(cx);
        cx.notify();
    }

    /// Reads where the vocals sound near the start and end of each of `tracks`, in the
    /// background, for the set's seams.
    fn measure_vocals(&mut self, tracks: Vec<PathBuf>, cx: &mut Context<'_, Self>) {
        let files: Vec<(PathBuf, StemFiles)> = tracks
            .into_iter()
            .filter_map(|track| {
                let files = self.stems.status(&track).files()?.clone();
                Some((track, files))
            })
            .collect();
        if files.is_empty() {
            return;
        }
        cx.spawn(async move |this, cx| {
            let measured = cx
                .background_executor()
                .spawn(async move {
                    files
                        .into_iter()
                        .map(|(track, files)| (track, files.vocal_presence()))
                        .collect::<Vec<_>>()
                })
                .await;
            this.update(cx, |this, cx| {
                for (track, vocals) in measured {
                    match vocals {
                        Ok(vocals) => this.stems.set_vocals(track, vocals),
                        Err(error) => eprintln!("warning: {error}"),
                    }
                }
                this.refresh_prepare_view(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The file of the library's track `track`.
    pub(super) fn track_file(&self, track: TrackIndex) -> Option<PathBuf> {
        self.collection()?.track(track).track.file_path.clone()
    }

    /// The file of the track on `deck`.
    fn deck_track_path(&self, deck: DeckId) -> Option<PathBuf> {
        self.deck(deck).track.as_ref()?.track.file_path.clone()
    }

    /// Loads the stems of the track on `deck` in the background, when they are ready, and gives
    /// them to the track. The deck plays the track as it is until then.
    pub(super) fn give_deck_stems(&mut self, deck: DeckId, cx: &mut Context<'_, Self>) {
        let Some(path) = self.deck_track_path(deck) else {
            return;
        };
        let Some(files) = self.stems.status(&path).files().cloned() else {
            return;
        };
        let Some(generation) = self.deck(deck).loaded_generation() else {
            return;
        };
        let Some(audio) = self
            .engine
            .as_ref()
            .ok()
            .and_then(|engine| engine.controller.track(deck).cloned())
            .filter(|audio| !audio.has_stems())
        else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let sample_rate = audio.sample_rate();
            let loaded = cx
                .background_executor()
                .spawn(async move { files.load(sample_rate) })
                .await;
            this.update(cx, |this, cx| {
                if !this.deck(deck).is_loaded_generation(generation) {
                    return;
                }
                let attached = loaded
                    .map_err(|error| error.to_string())
                    .and_then(|stems| audio.attach_stems(stems).map_err(|error| error.to_string()));
                if let Err(problem) = attached {
                    this.show_notice(
                        NoticePlace::Deck(deck),
                        format!("The stems cannot play: {problem}"),
                        cx,
                    );
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// A new track on `deck` starts with every stem up, and gets its stems if they are ready.
    pub(super) fn reset_deck_stems(&mut self, deck: DeckId, cx: &mut Context<'_, Self>) {
        self.mixer.stem_levels[deck.index()] = [1.0; Stem::COUNT];
        self.engine_command(cx, |controller| {
            controller.set_stems(deck, StemSettings::UNTOUCHED)
        });
        self.give_deck_stems(deck, cx);
    }

    /// Whether the track on `deck` plays its stems.
    pub(super) fn deck_has_stems(&self, deck: DeckId) -> bool {
        self.engine
            .as_ref()
            .ok()
            .and_then(|engine| engine.controller.track(deck))
            .is_some_and(dubplate_engine::Track::has_stems)
    }

    /// What SHIFT + the pads light in BEAT LOOP mode once the deck plays stems: the stems
    /// heard on pads 1 to 4, and the one-touch mix in play on pads 5 to 8.
    pub(super) fn stem_pad_lights(&self, deck: DeckId) -> Option<[bool; 8]> {
        if self.pad_modes[deck.index()] != PadMode::BeatLoop || !self.deck_has_stems(deck) {
            return None;
        }
        let settings = self.deck_stems(deck);
        Some(std::array::from_fn(|pad| {
            stem_pad_action(pad).is_some_and(|action| match action {
                StemAction::Toggle(stem) => settings.is_heard(stem),
                StemAction::Acapella => settings.is_solo(Stem::Vocals),
                StemAction::Instrumental => settings.is_without(Stem::Vocals),
                StemAction::Groove => is_groove(settings),
                StemAction::AllStems => Stem::ALL.into_iter().all(|stem| settings.is_heard(stem)),
            })
        }))
    }

    /// The stem settings of `deck`.
    pub(super) fn deck_stems(&self, deck: DeckId) -> StemSettings {
        self.engine
            .as_ref()
            .map_or(StemSettings::UNTOUCHED, |engine| {
                engine.controller.stems(deck)
            })
    }

    /// Where the stems of the track on `deck` stand.
    pub(super) fn deck_stems_status(&self, deck: DeckId) -> &StemsStatus {
        match self
            .deck(deck)
            .track
            .as_ref()
            .and_then(|track| track.track.file_path.as_deref())
        {
            Some(path) => self.stems.status(path),
            None => &StemsStatus::None,
        }
    }

    /// Mutes or brings back a stem, or plays a one-touch mix, on `deck`.
    pub(super) fn apply_stem_action(
        &mut self,
        deck: DeckId,
        action: StemAction,
        cx: &mut Context<'_, Self>,
    ) {
        let settings = action.apply(self.deck_stems(deck));
        self.engine_command(cx, |controller| controller.set_stems(deck, settings));
        cx.notify();
    }

    /// Sets the level of `stem` on `deck` from a knob position, on the channel fader's curve.
    pub(super) fn set_stem_level(
        &mut self,
        deck: DeckId,
        stem: Stem,
        fraction: f32,
        cx: &mut Context<'_, Self>,
    ) {
        self.mixer.stem_levels[deck.index()][stem.index()] = fraction;
        let settings = self
            .deck_stems(deck)
            .with_level(stem, StemLevel::new(channel_fader_gain(fraction)));
        self.engine_command(cx, |controller| controller.set_stems(deck, settings));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pad_has_a_stems_action_and_each_stem_its_own_pad() {
        let actions: Vec<StemAction> = (0..8).filter_map(stem_pad_action).collect();
        assert_eq!(actions.len(), 8);
        for stem in Stem::ALL {
            assert_eq!(
                actions
                    .iter()
                    .filter(|action| **action == StemAction::Toggle(stem))
                    .count(),
                1
            );
        }
        assert_eq!(stem_pad_action(8), None);
    }
}
