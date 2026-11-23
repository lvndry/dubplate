//! The performance pads and loop buttons on the decks: what each pad does in its deck's pad
//! mode, the loop section, and what the pads and loop buttons light.

use std::time::{Duration, Instant};

use dubplate_engine::DeckId;
use dubplate_midi::PadMode;
use dubplate_rekordbox::{HotCue, HotCuePad};
use gpui::{Context, SharedString};

use super::DubplateApp;
use super::notice::NoticePlace;
use super::sampler::sampler_slot;
use super::stems::stem_pad_action;
use crate::commands::{LoopAction, PadPress, Press};
use crate::cue_marks::MarkerPulse;
use crate::pad_fx::{PadFxLayer, PadFxSlot, PadTrigger};
use crate::pads::{
    AUTO_BEAT_LOOP, BEAT_JUMP_SCALE_RANGE, BEAT_LOOP_LENGTHS, KEY_PADS_DOWN_PAD, KEY_PADS_UP_PAD,
    LONGER_JUMPS_PAD, PAD_COUNT, PadFx, SHORTER_JUMPS_PAD, beat_jump_beats, beats_label,
    clamp_key_pad_offset, key_pad_semitones,
};

/// What a pad just did, shown over its deck's lane so a press on the controller is seen: the
/// jump, the loop length, or the effect while it is held.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct PadNote {
    /// What the press did, such as `−4 beats`.
    pub text: SharedString,
    started: Instant,
    held: bool,
}

/// How long a jump or a loop length stays shown.
const PAD_NOTE_DURATION: Duration = Duration::from_millis(900);

impl PadNote {
    fn flash(text: impl Into<SharedString>, now: Instant) -> Self {
        Self {
            text: text.into(),
            started: now,
            held: false,
        }
    }

    fn held(text: impl Into<SharedString>, now: Instant) -> Self {
        Self {
            text: text.into(),
            started: now,
            held: true,
        }
    }

    /// Whether the note still shows at `now`: while its pad is held, or for a moment after a
    /// jump or a loop.
    pub fn shows(&self, now: Instant) -> bool {
        self.held || now.saturating_duration_since(self.started) < PAD_NOTE_DURATION
    }
}

/// A beat jump as the deck shows it: `+4 beats`, `−1/2 beat`.
pub(super) fn jump_note(beats: f64) -> String {
    let sign = if beats < 0.0 { '\u{2212}' } else { '+' };
    let unit = if beats.abs() > 1.0 { "beats" } else { "beat" };
    format!("{sign}{} {unit}", beats_label(beats))
}

/// A Pad FX pad being held, or latched.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct HeldPad {
    /// The pad, 0 for pad 1.
    pub pad: usize,
    /// The layer it was pressed in.
    pub layer: PadFxLayer,
    /// What it plays.
    pub fx: PadFx,
    /// It stays on until pressed again: a latched pad, or a tap from the keys or the screen.
    pub latched: bool,
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

/// What a press on a Pad FX pad does.
#[derive(Debug, Clone, Copy, PartialEq)]
enum PadFxAction {
    /// Nothing changes.
    Nothing,
    /// The pad on lets go.
    Release,
    /// This pad takes over from any pad on.
    Start(HeldPad),
}

/// The controller holds a pad while it is pressed, unless the pad is set to latch; a latched
/// pad, or a tap from the keys or the screen, stays on until the same pad is pressed again.
/// Another pad takes over from the one on. Letting go matches the pad by number alone, so
/// lifting SHIFT first never leaves an effect stuck on.
fn pad_fx_action(
    held: Option<HeldPad>,
    press: PadPress,
    layer: PadFxLayer,
    slot: Option<PadFxSlot>,
) -> PadFxAction {
    let held = held.filter(|held| held.pad == press.pad);
    match press.press {
        Press::Up if held.is_some_and(|held| !held.latched) => PadFxAction::Release,
        Press::Up => PadFxAction::Nothing,
        Press::Down | Press::Tap if held.is_some_and(|held| held.layer == layer) => {
            if press.press == Press::Tap || held.is_some_and(|held| held.latched) {
                PadFxAction::Release
            } else {
                PadFxAction::Nothing
            }
        }
        Press::Down | Press::Tap => slot.map_or(PadFxAction::Nothing, |slot| {
            PadFxAction::Start(HeldPad {
                pad: press.pad,
                layer,
                fx: slot.play(),
                latched: press.press == Press::Tap || slot.trigger == PadTrigger::Latch,
            })
        }),
    }
}

impl DubplateApp {
    /// Routes a pad press to the deck’s current mode or the mode in a controller note.
    pub(super) fn press_pad(&mut self, press: PadPress, cx: &mut Context<'_, Self>) {
        let deck = press.deck;
        let index = deck.index();
        if press.press != Press::Up {
            let chosen = crate::pads::pad_mode_for_press(self.pad_modes[index], press.mode);
            if let Some(sent) = press.mode {
                self.controller_pad_modes[index] = sent;
            }
            self.set_pad_mode(deck, chosen, cx);
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
        if crate::pads::is_slicer_mode(mode) {
            let held = self.held_slices[index];
            if press.press == Press::Up
                || (press.press == Press::Tap && held.is_some_and(|(pad, _)| pad == press.pad))
            {
                if held.is_some_and(|(pad, _)| pad == press.pad) {
                    self.release_held_slice(deck, cx);
                }
            } else if let Some((slice, roll)) = crate::pads::slicer_pad_target(
                press.pad,
                press.shift,
                self.slicer_state(deck).and_then(|s| s.slice),
            ) {
                self.engine_command(cx, |c| c.slicer_pad(deck, slice, true, roll));
                self.held_slices[index] = Some((press.pad, slice));
            }
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
                    let now = Instant::now();
                    let from = self.deck_state(deck).position_seconds;
                    self.engine_command(cx, |controller| controller.beat_jump(deck, beats));
                    self.pad_notes[deck.index()] = Some(PadNote::flash(jump_note(beats), now));
                    self.decks[deck.index()].marker_pulse = Some(MarkerPulse {
                        seconds: from,
                        started: now,
                    });
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
                let note = if same {
                    "Loop off".to_owned()
                } else {
                    format!("Loop {length}")
                };
                self.pad_notes[deck.index()] = Some(PadNote::flash(note, Instant::now()));
            }
            PadMode::BeatLoop => {
                if let Some(action) = stem_pad_action(press.pad) {
                    self.apply_stem_action(deck, action, cx);
                }
            }
            PadMode::PadFx1
            | PadMode::PadFx2
            | PadMode::Sampler
            | PadMode::Slicer
            | PadMode::SlicerLoop => {}
        }
    }

    /// Puts a deck's pads in `mode`, letting go of a pad effect held in the mode it leaves.
    pub(super) fn set_pad_mode(&mut self, deck: DeckId, mode: PadMode, cx: &mut Context<'_, Self>) {
        let index = deck.index();
        if self.pad_modes[index] == mode {
            return;
        }
        self.release_held_pad(deck, cx);
        self.release_held_slice(deck, cx);
        if let Some(slicer) = crate::pads::slicer_engine_mode(mode) {
            let domain = self.slicer_domains[index];
            let result = self
                .engine
                .as_mut()
                .map_err(|e| e.to_string())
                .and_then(|e| {
                    e.controller
                        .slicer_on(deck, slicer, domain)
                        .map_err(|e| e.to_string())
                });
            if let Err(error) = result {
                self.show_notice(NoticePlace::Deck(deck), error, cx);
                return;
            }
            self.slicer_updated_at = Some(Instant::now());
        } else if crate::pads::is_slicer_mode(self.pad_modes[index]) {
            self.engine_command(cx, |c| c.slicer_off(deck));
        }
        self.pad_modes[deck.index()] = mode;
    }

    pub(super) fn reset_slicer_without_grid(&mut self, deck: DeckId) {
        let index = deck.index();
        if crate::pads::is_slicer_mode(self.pad_modes[index])
            && self
                .engine
                .as_ref()
                .ok()
                .and_then(|e| e.controller.beat_grid(deck))
                .is_none()
        {
            self.forget_slicer(deck);
        }
    }

    pub(super) fn forget_slicer(&mut self, deck: DeckId) {
        let index = deck.index();
        if crate::pads::is_slicer_mode(self.pad_modes[index]) {
            self.pad_modes[index] = self.controller_pad_modes[index];
            self.held_slices[index] = None;
        }
    }

    pub(super) fn set_slicer_domain(
        &mut self,
        deck: DeckId,
        domain: dubplate_engine::SlicerDomain,
        cx: &mut Context<'_, Self>,
    ) {
        let index = deck.index();
        if domain == self.slicer_domains[index] {
            return;
        }
        let Some(mode) = crate::pads::slicer_engine_mode(self.pad_modes[index]) else {
            return;
        };
        self.release_held_slice(deck, cx);
        let result = self
            .engine
            .as_mut()
            .map_err(|e| e.to_string())
            .and_then(|engine| {
                engine
                    .controller
                    .slicer_on(deck, mode, domain)
                    .map_err(|e| e.to_string())
            });
        match result {
            Ok(()) => {
                self.slicer_domains[index] = domain;
                self.slicer_updated_at = Some(Instant::now());
            }
            Err(error) => self.show_notice(NoticePlace::Deck(deck), error, cx),
        }
        cx.notify();
    }

    fn release_held_slice(&mut self, deck: DeckId, cx: &mut Context<'_, Self>) {
        if let Some((_, slice)) = self.held_slices[deck.index()].take() {
            self.engine_command(cx, |c| c.slicer_pad(deck, slice, false, None));
        }
    }

    pub(super) fn slicer_state(&self, deck: DeckId) -> Option<dubplate_engine::SlicerState> {
        self.engine
            .as_ref()
            .ok()
            .map(|e| e.controller.slicer_state(deck))
    }

    /// A Pad FX pad, as [`pad_fx_action`] decides.
    fn press_pad_fx(&mut self, press: PadPress, mode: PadMode, cx: &mut Context<'_, Self>) {
        let deck = press.deck;
        let Some(layer) = PadFxLayer::of(mode, press.shift) else {
            return;
        };
        let slot = self.pad_fx.slot(layer, press.pad);
        match pad_fx_action(self.held_pads[deck.index()], press, layer, slot) {
            PadFxAction::Nothing => {}
            PadFxAction::Release => self.release_held_pad(deck, cx),
            PadFxAction::Start(held) => {
                self.release_held_pad(deck, cx);
                self.pad_notes[deck.index()] = slot.map(|slot| {
                    let (name, parameter) = slot.label();
                    PadNote::held(
                        format!("{name} {parameter}").trim_end().to_owned(),
                        Instant::now(),
                    )
                });
                self.engine_command(cx, |controller| match held.fx {
                    PadFx::Roll(length) => controller.beat_roll(deck, length),
                    PadFx::Effect(effect) => controller.hold_pad_effect(deck, effect),
                });
                self.held_pads[deck.index()] = Some(held);
            }
        }
    }

    /// Lets go of the pad effect or roll held on a deck.
    pub(super) fn release_held_pad(&mut self, deck: DeckId, cx: &mut Context<'_, Self>) {
        if self.pad_notes[deck.index()]
            .as_ref()
            .is_some_and(|note| note.held)
        {
            self.pad_notes[deck.index()] = None;
        }
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
    /// that is on.
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
        });
    }

    /// The start and end of the loop a deck is going round, in seconds, when one is on.
    pub(super) fn active_loop(&self, deck: DeckId) -> Option<(f64, f64)> {
        self.loop_state(deck)
            .filter(|state| state.active)
            .and_then(|state| Some((state.start_seconds?, state.end_seconds?)))
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
                    let held = self.held_pads[deck.index()]
                        .filter(|held| {
                            PadFxLayer::of(self.pad_modes[deck.index()], held.layer.shifted())
                                == Some(held.layer)
                        })
                        .map(|held| held.pad);
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
                PadMode::Slicer | PadMode::SlicerLoop => {
                    if let Some(slice) = self.slicer_state(deck).and_then(|s| s.slice) {
                        pads[usize::from(slice.index())] = true;
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

#[cfg(test)]
mod tests {
    use dubplate_engine::{BeatLength, EffectKind};

    use super::*;

    #[test]
    fn a_beat_jump_shows_how_far_and_which_way() {
        assert_eq!(jump_note(-4.0), "\u{2212}4 beats");
        assert_eq!(jump_note(8.0), "+8 beats");
        assert_eq!(jump_note(1.0), "+1 beat");
        assert_eq!(jump_note(-0.5), "\u{2212}1/2 beat");
    }

    #[test]
    fn a_pad_note_shows_while_held_and_briefly_after_a_jump() {
        let now = Instant::now();
        let jump = PadNote::flash("+4 beats", now);
        assert!(jump.shows(now + Duration::from_millis(500)));
        assert!(!jump.shows(now + PAD_NOTE_DURATION));
        let effect = PadNote::held("Echo 1/2", now);
        assert!(effect.shows(now + Duration::from_secs(30)));
    }
    use crate::pad_fx::PadFxBanks;

    fn press(pad: usize, press: Press, shift: bool) -> PadPress {
        PadPress {
            deck: DeckId::A,
            pad,
            mode: Some(PadMode::PadFx1),
            press,
            shift,
        }
    }

    /// Plays `presses` on PAD FX1 from nothing held, and gives what is held after each.
    fn play(banks: &PadFxBanks, presses: &[PadPress]) -> Vec<Option<(usize, PadFxLayer)>> {
        let mut held = None;
        presses
            .iter()
            .map(|press| {
                let layer = PadFxLayer::of(PadMode::PadFx1, press.shift).unwrap();
                match pad_fx_action(held, *press, layer, banks.slot(layer, press.pad)) {
                    PadFxAction::Nothing => {}
                    PadFxAction::Release => held = None,
                    PadFxAction::Start(started) => held = Some(started),
                }
                held.map(|held: HeldPad| (held.pad, held.layer))
            })
            .collect()
    }

    #[test]
    fn a_held_pad_plays_until_let_go_even_when_shift_is_lifted_first() {
        let banks = PadFxBanks::default();
        let on_shift = Some((2, PadFxLayer::Fx1Shift));
        assert_eq!(
            play(
                &banks,
                &[
                    press(2, Press::Down, true),
                    press(2, Press::Up, false),
                    press(0, Press::Down, false),
                    press(1, Press::Down, false),
                    press(0, Press::Up, false),
                ]
            ),
            [
                on_shift,
                None,
                Some((0, PadFxLayer::Fx1)),
                Some((1, PadFxLayer::Fx1)),
                Some((1, PadFxLayer::Fx1)),
            ]
        );
    }

    #[test]
    fn a_latched_pad_stays_on_until_pressed_again_and_a_tap_latches_any_pad() {
        let mut banks = PadFxBanks::default();
        banks.edit(PadFxLayer::Fx1, 4, PadFxSlot::toggle_trigger);
        let latched = Some((4, PadFxLayer::Fx1));
        assert_eq!(
            play(
                &banks,
                &[
                    press(4, Press::Down, false),
                    press(4, Press::Up, false),
                    press(4, Press::Down, false),
                    press(4, Press::Up, false),
                    press(5, Press::Tap, false),
                    press(5, Press::Tap, false),
                ]
            ),
            [
                latched,
                latched,
                None,
                None,
                Some((5, PadFxLayer::Fx1)),
                None,
            ]
        );
    }

    #[test]
    fn a_pad_plays_what_the_editor_gave_it() {
        let mut banks = PadFxBanks::default();
        banks.edit(PadFxLayer::Fx1Shift, 0, |slot| slot.step_parameter(-1));
        let layer = PadFxLayer::Fx1Shift;
        let action = pad_fx_action(
            None,
            press(0, Press::Down, true),
            layer,
            banks.slot(layer, 0),
        );
        let PadFxAction::Start(held) = action else {
            panic!("{action:?}");
        };
        let PadFx::Effect(effect) = held.fx else {
            panic!("{held:?}");
        };
        assert_eq!(effect.kind, EffectKind::Trans);
        assert_eq!(
            effect.parameter,
            Some(dubplate_engine::ParameterValue::Beats(BeatLength::EIGHTH))
        );
    }
}
