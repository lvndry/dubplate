//! The sampler in the Sampler pad mode, as rekordbox lays it out on the DDJ-FLX4: the left
//! deck's pads play slots 1 to 8 and the right deck's slots 9 to 16, SHIFT and a pad stops a
//! slot or loads the selected track into an empty one. The slots and their settings come back
//! at the next launch.

use std::path::{Path, PathBuf};

use dubplate_engine::{DeckId, SAMPLER_SLOTS, SampleMode, SampleSettings, SamplerState};
use gpui::{Context, SharedString};

use super::DubplateApp;
use super::notice::NoticePlace;
use crate::commands::{PadPress, Press};
use crate::pads::PAD_COUNT;
use crate::settings::SampleSlotSetting;

/// A slot setting the sampler controls switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SampleOption {
    /// Loop rather than play once.
    Loop,
    /// Play only while the pad is held.
    Gate,
    /// Play at the tempo master's BPM.
    Sync,
}

/// The slot a deck's sampler pad plays.
pub(super) fn sampler_slot(deck: DeckId, pad: usize) -> usize {
    deck.index() * PAD_COUNT + pad
}

/// A saved slot's settings as the engine takes them.
fn engine_settings(setting: &SampleSlotSetting) -> SampleSettings {
    SampleSettings {
        mode: if setting.looped {
            SampleMode::Loop
        } else {
            SampleMode::OneShot
        },
        gate: setting.gate,
        sync: setting.sync && setting.bpm.is_some(),
        bpm: setting.bpm,
        gain: 1.0,
    }
}

impl DubplateApp {
    /// A pad in Sampler mode.
    pub(super) fn press_sampler_pad(&mut self, press: PadPress, cx: &mut Context<'_, Self>) {
        let slot = sampler_slot(press.deck, press.pad);
        if slot >= SAMPLER_SLOTS {
            return;
        }
        let state = self
            .sampler_state()
            .and_then(|state| state.slots[slot].as_ref().copied());
        if matches!(press.press, Press::Down | Press::Tap) {
            self.selected_samples[press.deck.index()] = press.pad;
        }
        match (press.press, press.shift, state) {
            (Press::Down | Press::Tap, true, Some(slot_state)) if slot_state.playing => {
                self.engine_command(cx, |controller| controller.stop_sample(slot));
            }
            (Press::Down | Press::Tap, true, _) => self.load_selected_into_slot(slot, cx),
            (Press::Down, false, Some(_)) => {
                self.engine_command(cx, |controller| controller.trigger_sample(slot));
            }
            (Press::Up, false, Some(slot_state)) if slot_state.settings.gate => {
                self.engine_command(cx, |controller| controller.release_sample(slot));
            }
            (Press::Tap, false, Some(slot_state))
                if slot_state.settings.gate && slot_state.playing =>
            {
                self.engine_command(cx, |controller| controller.release_sample(slot));
            }
            (Press::Tap, false, Some(_)) => {
                self.engine_command(cx, |controller| controller.trigger_sample(slot));
            }
            (Press::Down | Press::Tap, false, None) => self.show_notice(
                NoticePlace::Deck(press.deck),
                format!(
                    "Slot {} is empty: SHIFT and its pad load the selected track.",
                    slot + 1
                ),
                cx,
            ),
            (Press::Up, _, _) => {}
        }
        cx.notify();
    }

    /// The sampler's slots, volume and CUE, when the engine runs.
    pub(super) fn sampler_state(&self) -> Option<SamplerState> {
        self.engine
            .as_ref()
            .ok()
            .map(|engine| engine.controller.sampler_state())
    }

    /// The title of the sample saved in `slot`.
    pub(super) fn sample_title(&self, slot: usize) -> Option<SharedString> {
        let setting = self
            .settings
            .sampler
            .slots
            .iter()
            .find(|setting| setting.slot == slot)?;
        Some(self.title_of(&setting.path).into())
    }

    /// Switches one setting of the slot last pressed on `deck`.
    pub(super) fn toggle_sample_option(
        &mut self,
        deck: DeckId,
        option: SampleOption,
        cx: &mut Context<'_, Self>,
    ) {
        let slot = sampler_slot(deck, self.selected_samples[deck.index()]);
        let Some(setting) = self
            .settings
            .sampler
            .slots
            .iter_mut()
            .find(|setting| setting.slot == slot)
        else {
            return;
        };
        match option {
            SampleOption::Loop => setting.looped = !setting.looped,
            SampleOption::Gate => setting.gate = !setting.gate,
            SampleOption::Sync if setting.bpm.is_none() => {
                self.show_notice(
                    NoticePlace::Deck(deck),
                    "This sample has no tempo to sync.",
                    cx,
                );
                return;
            }
            SampleOption::Sync => setting.sync = !setting.sync,
        }
        let settings = engine_settings(setting);
        self.engine_command(cx, |controller| {
            controller.set_sample_settings(slot, settings)
        });
        self.save_settings(cx);
        cx.notify();
    }

    /// The saved settings of the slot last pressed on `deck`, when it holds a sample.
    pub(super) fn selected_sample(&self, deck: DeckId) -> Option<&SampleSlotSetting> {
        let slot = sampler_slot(deck, self.selected_samples[deck.index()]);
        self.settings
            .sampler
            .slots
            .iter()
            .find(|setting| setting.slot == slot)
    }

    /// Turns the sampler's CUE on or off: hear the sampler in the headphones.
    pub(super) fn toggle_sampler_cue(&mut self, cx: &mut Context<'_, Self>) {
        let cued = self.sampler_state().is_some_and(|state| state.cue);
        self.engine_command(cx, |controller| controller.set_sampler_cue(!cued));
        cx.notify();
    }

    /// Puts every saved sample back in its slot, after a launch or a new sound card.
    pub(super) fn restore_sampler(&mut self, cx: &mut Context<'_, Self>) {
        for setting in self.settings.sampler.slots.clone() {
            self.load_into_slot(setting, cx);
        }
    }

    fn load_selected_into_slot(&mut self, slot: usize, cx: &mut Context<'_, Self>) {
        let selected = self.browser.read(cx).selected().and_then(|track| {
            let entry = self.collection()?.track(track);
            Some((entry.track.file_path.clone()?, entry.track.bpm))
        });
        let Some((path, bpm)) = selected else {
            self.show_notice(
                NoticePlace::Window,
                "Select a track in the library to load it into the sampler.",
                cx,
            );
            return;
        };
        self.load_into_slot(
            SampleSlotSetting {
                slot,
                path,
                bpm,
                looped: false,
                gate: false,
                sync: false,
            },
            cx,
        );
    }

    /// Opens the sample's file off the interface thread and puts it in its slot.
    fn load_into_slot(&mut self, setting: SampleSlotSetting, cx: &mut Context<'_, Self>) {
        let path = setting.path.clone();
        cx.spawn(async move |this, cx| {
            let track = cx
                .background_executor()
                .spawn(async move { dubplate_engine::decode_file(&path) })
                .await;
            this.update(cx, |this, cx| match track {
                Ok(track) => this.put_in_slot(setting, track, cx),
                Err(error) => this.show_notice(
                    NoticePlace::Window,
                    format!(
                        "{} could not go in the sampler: {error}",
                        file_title(&setting.path)
                    ),
                    cx,
                ),
            })
            .ok();
        })
        .detach();
    }

    fn put_in_slot(
        &mut self,
        setting: SampleSlotSetting,
        track: dubplate_engine::Track,
        cx: &mut Context<'_, Self>,
    ) {
        let slot = setting.slot;
        let settings = engine_settings(&setting);
        let loaded = match &mut self.engine {
            Ok(engine) => engine
                .controller
                .load_sample(slot, track, settings)
                .map_err(|error| error.to_string()),
            Err(problem) => Err(problem.to_string()),
        };
        if let Err(problem) = loaded {
            self.show_notice(NoticePlace::Window, problem, cx);
            return;
        }
        let slots = &mut self.settings.sampler.slots;
        slots.retain(|existing| existing.slot != slot);
        slots.push(setting);
        slots.sort_by_key(|existing| existing.slot);
        self.save_settings(cx);
        cx.notify();
    }

    /// A track's title from the library, or its file name.
    fn title_of(&self, path: &Path) -> String {
        self.collection()
            .and_then(|collection| collection.find_path(path))
            .and_then(|index| {
                self.collection()
                    .map(|collection| collection.track(index).row.title.to_string())
            })
            .unwrap_or_else(|| file_title(path))
    }
}

/// A file's name without its extension.
fn file_title(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| PathBuf::from(path).display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_left_deck_plays_slots_1_to_8_and_the_right_9_to_16() {
        assert_eq!(sampler_slot(DeckId::A, 0), 0);
        assert_eq!(sampler_slot(DeckId::A, 7), 7);
        assert_eq!(sampler_slot(DeckId::B, 0), 8);
        assert_eq!(sampler_slot(DeckId::B, 7), 15);
        assert_eq!(SAMPLER_SLOTS, 16);
    }

    #[test]
    fn a_sample_syncs_only_with_a_tempo() {
        let setting = SampleSlotSetting {
            slot: 0,
            path: PathBuf::from("/Samples/Horn.wav"),
            bpm: None,
            looped: true,
            gate: true,
            sync: true,
        };
        let settings = engine_settings(&setting);
        assert_eq!(settings.mode, SampleMode::Loop);
        assert!(settings.gate);
        assert!(!settings.sync, "nothing to sync to without a tempo");
        let with_tempo = engine_settings(&SampleSlotSetting {
            bpm: Some(122.0),
            ..setting
        });
        assert!(with_tempo.sync);
    }
}
