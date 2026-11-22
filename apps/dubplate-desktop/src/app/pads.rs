//! The performance pads and loop buttons on the decks: what each pad does in its deck's pad
//! mode, the loop section, and what the pads and loop buttons light.

use dubplate_engine::DeckId;
use dubplate_midi::PadMode;
use dubplate_rekordbox::{HotCue, HotCuePad};
use gpui::Context;

use super::DubplateApp;
use super::notice::NoticePlace;
use super::sampler::sampler_slot;
use super::stems::stem_pad_action;
use crate::commands::{LoopAction, PadPress, Press};
use crate::pads::{
    AUTO_BEAT_LOOP, BEAT_JUMP_SCALE_RANGE, BEAT_LOOP_LENGTHS, KEY_PADS_DOWN_PAD, KEY_PADS_UP_PAD,
    PAD_COUNT, PadFx, beat_jump_beats, clamp_key_pad_offset, key_pad_semitones, pad_fx,
};

/// SHIFT with this beat jump pad makes the jumps shorter.
const SHORTER_JUMPS_PAD: usize = 6;
/// SHIFT with this beat jump pad makes the jumps longer.
const LONGER_JUMPS_PAD: usize = 7;

/// A Pad FX pad being held, or latched from the keys or the screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct HeldPad {
    /// The pad, 0 for pad 1.
    pub pad: usize,
    /// What it plays.
    pub fx: PadFx,
}

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
            self.set_pad_mode(deck, mode, cx);
        }
        let mode = self.pad_modes[deck.index()];
        if mode == PadMode::Sampler {
            self.press_sampler_pad(press, cx);
            return;
        }
        if self.deck(deck).visuals().is_none() {
            return;
        }
        if matches!(mode, PadMode::PadFx1 | PadMode::PadFx2) {
            self.press_pad_fx(press, mode, cx);
            return;
        }
        if !matches!(press.press, Press::Down | Press::Tap) {
            return;
        }
        match mode {
            PadMode::HotCue => {
                let Some(pad) = HotCuePad::ALL.get(press.pad).copied() else {
                    return;
                };
                if press.shift {
                    self.edit_cues(deck, |cues| cues.delete_hot_cue(pad), cx);
                } else {
                    self.keyboard_cues[deck.index()] = pad;
                    self.press_hot_cue(deck, pad, cx);
                }
            }
            PadMode::Keyboard | PadMode::KeyShift if press.shift => {
                let offset = &mut self.key_pad_offsets[deck.index()];
                match press.pad {
                    KEY_PADS_UP_PAD => *offset = clamp_key_pad_offset(*offset + 1),
                    KEY_PADS_DOWN_PAD => *offset = clamp_key_pad_offset(*offset - 1),
                    _ => {}
                }
            }
            PadMode::KeyShift => {
                if let Some(semitones) =
                    key_pad_semitones(press.pad, self.key_pad_offsets[deck.index()])
                {
                    self.engine_command(cx, |controller| {
                        controller.set_key_shift(deck, semitones as f32)
                    });
                }
            }
            PadMode::Keyboard => self.play_keyboard_pad(deck, press.pad, cx),
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
            PadMode::BeatLoop => {
                if let Some(action) = stem_pad_action(press.pad) {
                    self.apply_stem_action(deck, action, cx);
                }
            }
            PadMode::PadFx1 | PadMode::PadFx2 | PadMode::Sampler => {}
        }
    }

    /// Puts a deck's pads in `mode`, letting go of a pad effect held in the mode it leaves.
    pub(super) fn set_pad_mode(&mut self, deck: DeckId, mode: PadMode, cx: &mut Context<'_, Self>) {
        if self.pad_modes[deck.index()] != mode {
            self.release_held_pad(deck, cx);
        }
        self.pad_modes[deck.index()] = mode;
    }

    /// A Pad FX pad: the controller holds it while pressed; a tap from the keys or the screen
    /// latches it on until the same pad is tapped again. Another pad takes over from the one
    /// held.
    fn press_pad_fx(&mut self, press: PadPress, mode: PadMode, cx: &mut Context<'_, Self>) {
        let deck = press.deck;
        let held_here = self.held_pads[deck.index()].is_some_and(|held| held.pad == press.pad);
        match press.press {
            Press::Up if held_here => self.release_held_pad(deck, cx),
            Press::Tap if held_here => self.release_held_pad(deck, cx),
            Press::Up => {}
            Press::Down | Press::Tap => {
                let Some(fx) = pad_fx(mode, press.pad, press.shift) else {
                    return;
                };
                self.release_held_pad(deck, cx);
                self.engine_command(cx, |controller| match fx {
                    PadFx::Roll(length) => controller.beat_roll(deck, length),
                    PadFx::Effect(effect) => controller.hold_pad_effect(deck, effect),
                });
                self.held_pads[deck.index()] = Some(HeldPad { pad: press.pad, fx });
            }
        }
    }

    /// Lets go of the pad effect or roll held on a deck.
    pub(super) fn release_held_pad(&mut self, deck: DeckId, cx: &mut Context<'_, Self>) {
        if let Some(held) = self.held_pads[deck.index()].take() {
            self.engine_command(cx, |controller| match held.fx {
                PadFx::Roll(_) => controller.release_roll(deck),
                PadFx::Effect(_) => controller.release_pad_effect(deck),
            });
        }
    }

    /// A Keyboard pad: the deck plays from its keyboard hot cue, the last one pressed, at the
    /// pad's pitch.
    fn play_keyboard_pad(&mut self, deck: DeckId, pad: usize, cx: &mut Context<'_, Self>) {
        let Some(semitones) = key_pad_semitones(pad, self.key_pad_offsets[deck.index()]) else {
            return;
        };
        let cue = self.keyboard_cues[deck.index()];
        let Some(seconds) = self.deck(deck).cues.hot_cue(cue).map(HotCue::start_seconds) else {
            self.show_notice(
                NoticePlace::Deck(deck),
                format!(
                    "The keyboard plays from hot cue {}: set it in Hot cue mode first.",
                    cue.letter()
                ),
                cx,
            );
            return;
        };
        self.decks[deck.index()].cue_preview = false;
        self.engine_command(cx, |controller| {
            controller.set_key_shift(deck, semitones as f32)?;
            controller.hot_cue(deck, seconds)
        });
    }

    /// A deck's key shift in semitones, when the engine runs.
    pub(super) fn key_shift(&self, deck: DeckId) -> i32 {
        self.engine
            .as_ref()
            .map_or(0, |engine| engine.controller.key_shift(deck).round() as i32)
    }

    /// Turns SLIP on or off for a deck.
    pub(super) fn toggle_slip(&mut self, deck: DeckId, cx: &mut Context<'_, Self>) {
        let enabled = !self.slip(deck);
        self.engine_command(cx, |controller| controller.set_slip(deck, enabled));
    }

    /// Whether SLIP is on for a deck.
    pub(super) fn slip(&self, deck: DeckId) -> bool {
        self.engine
            .as_ref()
            .is_ok_and(|engine| engine.controller.slip_state(deck).enabled)
    }

    /// Where a slipping deck would be now, while a roll, scratch or loop slips: the ghost
    /// playhead the lanes draw.
    pub(super) fn slip_shadow(&self, deck: DeckId) -> Option<f64> {
        self.engine
            .as_ref()
            .ok()
            .and_then(|engine| engine.controller.slip_state(deck).shadow_seconds)
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
        if self.pad_modes[deck.index()] == PadMode::Sampler {
            let slots = self.sampler_state().map(|state| state.slots);
            for (pad, lit) in pads.iter_mut().enumerate() {
                let slot = slots
                    .as_ref()
                    .and_then(|slots| slots.get(sampler_slot(deck, pad)).copied().flatten());
                *lit = slot.is_some_and(|slot| !slot.playing || blink);
            }
        } else if side.visuals().is_some() {
            match self.pad_modes[deck.index()] {
                PadMode::HotCue => {
                    for (pad, _) in side.cues.hot_cues() {
                        if let Some(lit) = pads.get_mut(pad.index()) {
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
                PadMode::PadFx1 | PadMode::PadFx2 => {
                    let held = self.held_pads[deck.index()].map(|held| held.pad);
                    for (pad, lit) in pads.iter_mut().enumerate() {
                        *lit = held != Some(pad) || blink;
                    }
                }
                PadMode::Keyboard | PadMode::KeyShift => {
                    let offset = self.key_pad_offsets[deck.index()];
                    let shift = self.key_shift(deck);
                    for (pad, lit) in pads.iter_mut().enumerate() {
                        let semitones = key_pad_semitones(pad, offset);
                        *lit = semitones.is_some() && (semitones != Some(shift) || blink);
                    }
                }
                PadMode::Sampler => {}
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
