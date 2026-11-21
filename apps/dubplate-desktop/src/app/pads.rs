//! The performance pads and loop buttons on the decks: what each pad does in its deck's pad
//! mode, the loop section, and what the pads and loop buttons light.

use dubplate_engine::DeckId;
use dubplate_midi::PadMode;
use dubplate_rekordbox::HotCuePad;
use gpui::Context;

use super::DubplateApp;
use crate::commands::{LoopAction, PadPress, Press};
use crate::deck::pad_position;
use crate::pads::{
    AUTO_BEAT_LOOP, BEAT_JUMP_SCALE_RANGE, BEAT_LOOP_LENGTHS, PAD_COUNT, beat_jump_beats,
};

/// SHIFT with this beat jump pad makes the jumps shorter.
const SHORTER_JUMPS_PAD: usize = 6;
/// SHIFT with this beat jump pad makes the jumps longer.
const LONGER_JUMPS_PAD: usize = 7;

/// What a deck's loop buttons and pads show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct PadLights {
    /// The pads lit, pad 1 first.
    pub pads: [bool; PAD_COUNT],
    /// LOOP IN.
    pub loop_in: bool,
    /// LOOP OUT.
    pub loop_out: bool,
    /// 4 BEAT/EXIT.
    pub four_beat: bool,
}

impl DubplateApp {
    /// A pad press in the mode the controller sent it in, or the deck's mode. A controller's
    /// mode wins: the deck takes it, as the pads are where the DJ's hands are.
    pub(super) fn press_pad(&mut self, press: PadPress, cx: &mut Context<'_, Self>) {
        let deck = press.deck;
        if let Some(mode) = press.mode {
            self.pad_modes[deck.index()] = mode;
        }
        if !matches!(press.press, Press::Down | Press::Tap) || self.deck(deck).visuals().is_none() {
            return;
        }
        match self.pad_modes[deck.index()] {
            PadMode::HotCue => {
                let Some(pad) = HotCuePad::ALL.get(press.pad).copied() else {
                    return;
                };
                if press.shift {
                    self.edit_hot_cues(deck, |hot_cues| hot_cues.delete(pad));
                } else {
                    self.press_hot_cue(deck, pad, cx);
                }
            }
            PadMode::BeatJump if press.shift => {
                let scale = &mut self.beat_jump_scales[deck.index()];
                match press.pad {
                    SHORTER_JUMPS_PAD => *scale = (*scale - 1).max(-BEAT_JUMP_SCALE_RANGE),
                    LONGER_JUMPS_PAD => *scale = (*scale + 1).min(BEAT_JUMP_SCALE_RANGE),
                    _ => {}
                }
            }
            PadMode::BeatJump => {
                let scale = self.beat_jump_scales[deck.index()];
                if let Some(beats) = beat_jump_beats(press.pad, scale) {
                    self.engine_command(cx, |controller| controller.beat_jump(deck, beats));
                }
            }
            PadMode::BeatLoop if !press.shift => {
                let Some(length) = BEAT_LOOP_LENGTHS.get(press.pad).copied() else {
                    return;
                };
                let current = self.loop_state(deck);
                let same =
                    current.is_some_and(|state| state.active && state.length == Some(length));
                self.engine_command(cx, |controller| {
                    if same {
                        controller.reloop_or_exit(deck)
                    } else {
                        controller.beat_loop(deck, length)
                    }
                });
            }
            PadMode::BeatLoop
            | PadMode::PadFx1
            | PadMode::PadFx2
            | PadMode::Sampler
            | PadMode::Keyboard
            | PadMode::KeyShift => {}
        }
    }

    /// A loop button, as on rekordbox: 4 BEAT/EXIT starts a four-beat loop or leaves the one
    /// that is on, and the CUE/LOOP CALL arrows halve or double a loop that is on.
    pub(super) fn loop_button(
        &mut self,
        deck: DeckId,
        action: LoopAction,
        cx: &mut Context<'_, Self>,
    ) {
        if self.deck(deck).visuals().is_none() {
            return;
        }
        let looping = self.loop_state(deck).is_some_and(|state| state.active);
        self.engine_command(cx, |controller| match action {
            LoopAction::In => controller.loop_in(deck),
            LoopAction::Out => controller.loop_out(deck),
            LoopAction::FourBeatOrExit if looping => controller.reloop_or_exit(deck),
            LoopAction::FourBeatOrExit => controller.beat_loop(deck, AUTO_BEAT_LOOP),
            LoopAction::Halve if looping => controller.halve_loop(deck),
            LoopAction::Double if looping => controller.double_loop(deck),
            LoopAction::Halve | LoopAction::Double => Ok(()),
        });
    }

    /// A deck's loop, when the engine runs.
    pub(super) fn loop_state(&self, deck: DeckId) -> Option<dubplate_engine::LoopState> {
        self.engine
            .as_ref()
            .ok()
            .map(|engine| engine.controller.loop_state(deck))
    }

    /// What a deck's pads and loop buttons light: set hot cues; every beat jump pad; every beat
    /// loop pad, with the loop that is on blinking; nothing in the modes still to come.
    pub(super) fn pad_lights(&self, deck: DeckId, blink: bool) -> PadLights {
        let side = self.deck(deck);
        let loop_state = self.loop_state(deck);
        let looping = loop_state.is_some_and(|state| state.active);
        let active_length = loop_state
            .filter(|state| state.active)
            .and_then(|state| state.length);
        let waiting_for_out = loop_state.is_some_and(|state| state.pending_start_seconds.is_some());
        let mut pads = [false; PAD_COUNT];
        if side.visuals().is_some() {
            match self.pad_modes[deck.index()] {
                PadMode::HotCue => {
                    for (pad, _) in side.hot_cues.iter() {
                        if let Some(lit) = pads.get_mut(pad_position(pad)) {
                            *lit = true;
                        }
                    }
                }
                PadMode::BeatJump => pads = [true; PAD_COUNT],
                PadMode::BeatLoop => {
                    for (lit, length) in pads.iter_mut().zip(BEAT_LOOP_LENGTHS) {
                        *lit = active_length != Some(length) || blink;
                    }
                }
                PadMode::PadFx1
                | PadMode::PadFx2
                | PadMode::Sampler
                | PadMode::Keyboard
                | PadMode::KeyShift => {}
            }
        }
        PadLights {
            pads,
            loop_in: looping || (waiting_for_out && blink),
            loop_out: looping,
            four_beat: looping,
        }
    }
}
