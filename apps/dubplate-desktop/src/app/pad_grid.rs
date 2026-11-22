//! A deck's pad section: the four pad mode buttons with their SHIFT modes, the loop buttons, and
//! the eight pads labelled for the mode they are in.

use dubplate_engine::DeckId;
use dubplate_midi::PadMode;
use dubplate_rekordbox::{HotCue, HotCuePad};
use gpui::prelude::*;
use gpui::{ClickEvent, Context, FontWeight, MouseButton, Pixels, Rgba, SharedString, div, px};

use super::sampler::{SampleOption, sampler_slot};
use super::{DubplateApp, knob};
use crate::collection::clock;
use crate::commands::{Command, LoopAction, PadPress, Press};
use crate::controls::Control;
use crate::fonts::MONO_FAMILY;
use crate::pads::{
    BEAT_LOOP_LENGTHS, PAD_COUNT, beat_jump_beats, beats_label, key_pad_semitones, mode_buttons,
    mode_label, pad_fx, semitones_label,
};
use crate::theme;
use crate::widgets::{InteractiveControl, shift_activation};

const PAD_HEIGHT: Pixels = px(38.0);
const TAB_HEIGHT: Pixels = px(24.0);

/// What one pad shows.
struct PadFace {
    title: SharedString,
    detail: SharedString,
    /// The colour of its dot, when it has one.
    dot: Option<Rgba>,
    /// Lit, as a set hot cue or the loop that is on.
    lit: bool,
    /// Pressing it does something.
    playable: bool,
}

pub(super) fn render(
    app: &DubplateApp,
    deck: DeckId,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<> {
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(
            div()
                .flex()
                .justify_between()
                .items_center()
                .child(mode_tabs(app, deck, cx))
                .map(|row| {
                    if app.pad_modes[deck.index()] == PadMode::Sampler {
                        row.child(sampler_controls(app, deck, cx))
                    } else {
                        row.child(loop_buttons(app, deck, cx))
                    }
                }),
        )
        .child(pads(app, deck, cx))
}

/// The four mode buttons. Each shows its SHIFT mode when the pads are in it; a click picks the
/// button's mode and a shift-click its SHIFT mode, as on the controller.
fn mode_tabs(
    app: &DubplateApp,
    deck: DeckId,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<> {
    let current = app.pad_modes[deck.index()];
    div()
        .flex()
        .gap(px(2.0))
        .children(
            mode_buttons()
                .enumerate()
                .map(|(button, (mode, shift_mode))| {
                    let selected = current == mode || current == shift_mode;
                    let shown = if current == shift_mode {
                        shift_mode
                    } else {
                        mode
                    };
                    div()
                        .id(("pad-mode", deck.index() * 4 + button))
                        .accessible_toggle(
                            format!(
                                "Deck {} {} pad mode",
                                crate::deck::deck_letter(deck),
                                mode_label(shown)
                            ),
                            selected,
                        )
                        .flex()
                        .items_center()
                        .h(TAB_HEIGHT)
                        .px(px(8.0))
                        .rounded(px(5.0))
                        .text_size(theme::SMALL_TEXT_SIZE)
                        .cursor_pointer()
                        .map(|tab| {
                            if selected {
                                tab.bg(theme::RAISED)
                                    .text_color(theme::TEXT)
                                    .font_weight(FontWeight::MEDIUM)
                            } else {
                                tab.text_color(theme::MUTED)
                                    .hover(|tab| tab.text_color(theme::TEXT))
                            }
                        })
                        .aria_description(
                            "Enter selects this pad mode. Shift Enter selects its alternate mode.",
                        )
                        .on_key_down(cx.listener(move |this, event, window, cx| {
                            if shift_activation(event) {
                                this.perform(Command::SetPadMode(deck, shift_mode), window, cx);
                                cx.stop_propagation();
                            }
                        }))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                            let chosen = if event.modifiers().shift {
                                shift_mode
                            } else {
                                mode
                            };
                            this.perform(Command::SetPadMode(deck, chosen), window, cx);
                        }))
                        .child(mode_label(shown))
                }),
        )
}

/// In Sampler mode: Loop, Gate and Sync for the slot last pressed, the sampler's CUE and its
/// volume.
fn sampler_controls(
    app: &DubplateApp,
    deck: DeckId,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<> {
    let colour = theme::deck_colour(deck);
    let selected = app.selected_sample(deck);
    let cued = app.sampler_state().is_some_and(|state| state.cue);
    let toggle = |id: &'static str, label: &'static str, on: bool, enabled: bool| {
        div()
            .id((id, deck.index()))
            .when(enabled, |button| {
                button.accessible_toggle(
                    format!("Deck {} sample {label}", crate::deck::deck_letter(deck)),
                    on,
                )
            })
            .flex()
            .items_center()
            .h(TAB_HEIGHT)
            .px(px(8.0))
            .rounded(px(5.0))
            .font_family(MONO_FAMILY)
            .text_size(theme::TINY_TEXT_SIZE)
            .border_1()
            .map(|button| {
                if on {
                    button.border_color(colour).text_color(theme::TEXT)
                } else if enabled {
                    button
                        .border_color(theme::LINE)
                        .text_color(theme::MUTED)
                        .hover(|button| button.text_color(theme::TEXT))
                } else {
                    button
                        .border_dashed()
                        .border_color(theme::LINE)
                        .text_color(theme::MUTED)
                }
            })
            .when(enabled, |button| button.cursor_pointer())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(label)
    };
    let option = |id, label, on: bool, option: SampleOption| {
        toggle(id, label, on, selected.is_some()).when(selected.is_some(), |button| {
            button.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.toggle_sample_option(deck, option, cx)
            }))
        })
    };
    div()
        .flex()
        .items_center()
        .gap(px(4.0))
        .child(option(
            "sample-loop",
            "Loop",
            selected.is_some_and(|setting| setting.looped),
            SampleOption::Loop,
        ))
        .child(option(
            "sample-gate",
            "Gate",
            selected.is_some_and(|setting| setting.gate),
            SampleOption::Gate,
        ))
        .child(option(
            "sample-sync",
            "Sync",
            selected.is_some_and(|setting| setting.sync && setting.bpm.is_some()),
            SampleOption::Sync,
        ))
        .child(
            toggle("sampler-cue", "Cue", cued, true)
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_sampler_cue(cx))),
        )
        .child(knob::render(
            app,
            Control::SamplerVolume,
            TAB_HEIGHT,
            colour,
            cx,
        ))
}

/// IN, OUT and the four-beat loop, lit while a loop is on.
fn loop_buttons(
    app: &DubplateApp,
    deck: DeckId,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<> {
    let looping = app.loop_state(deck).is_some_and(|state| state.active);
    let colour = theme::deck_colour(deck);
    let button = |id: &'static str, label: &'static str, action: LoopAction| {
        div()
            .id((id, deck.index()))
            .accessible_button(format!(
                "Deck {} loop {label}",
                crate::deck::deck_letter(deck)
            ))
            .flex()
            .items_center()
            .h(TAB_HEIGHT)
            .px(px(8.0))
            .rounded(px(5.0))
            .font_family(MONO_FAMILY)
            .text_size(theme::TINY_TEXT_SIZE)
            .cursor_pointer()
            .border_1()
            .map(|button| {
                if looping {
                    button.border_color(colour).text_color(theme::TEXT)
                } else {
                    button
                        .border_color(theme::LINE)
                        .text_color(theme::MUTED)
                        .hover(|button| button.text_color(theme::TEXT))
                }
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.perform(Command::Loop(deck, action), window, cx);
            }))
            .child(label)
    };
    div()
        .flex()
        .gap(px(4.0))
        .child(button("loop-in", "In", LoopAction::In))
        .child(button("loop-out", "Out", LoopAction::Out))
        .child(button(
            "loop-four",
            if looping { "Exit" } else { "4 beat" },
            LoopAction::FourBeatOrExit,
        ))
}

fn pads(
    app: &DubplateApp,
    deck: DeckId,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<> {
    let loaded =
        app.deck(deck).visuals().is_some() || app.pad_modes[deck.index()] == PadMode::Sampler;
    let faces = faces(app, deck);
    div()
        .flex()
        .gap(px(4.0))
        .children(faces.into_iter().enumerate().map(|(pad, face)| {
            let playable = loaded && face.playable;
            div()
                .id(("pad", deck.index() * PAD_COUNT + pad))
                .when(playable, |button| {
                    button.accessible_button(format!(
                        "Deck {} pad {}: {} {}",
                        crate::deck::deck_letter(deck),
                        pad + 1,
                        face.title,
                        face.detail
                    ))
                })
                .flex()
                .flex_col()
                .justify_center()
                .flex_1()
                .min_w_0()
                .gap(px(1.0))
                .h(PAD_HEIGHT)
                .px(px(8.0))
                .rounded(theme::SMALL_RADIUS)
                .border_1()
                .text_size(theme::SMALL_TEXT_SIZE)
                .overflow_hidden()
                .map(|pad_button| {
                    if face.lit && loaded {
                        pad_button
                            .bg(theme::RAISED)
                            .border_color(face.dot.unwrap_or(theme::LINE))
                    } else if playable {
                        pad_button.bg(theme::RAISED).border_color(theme::LINE)
                    } else {
                        pad_button
                            .border_dashed()
                            .border_color(theme::LINE)
                            .text_color(theme::MUTED)
                    }
                })
                .when(playable, |pad_button| {
                    pad_button
                        .cursor_pointer()
                        .hover(|pad_button| pad_button.border_color(theme::MUTED))
                        .aria_description("Enter plays this pad. Shift Enter performs its alternate action, such as deleting a hot cue.")
                        .on_key_down(cx.listener(move |this, event, window, cx| {
                            if shift_activation(event) {
                                this.perform(Command::Pad(PadPress { deck, pad, mode: None, press: Press::Tap, shift: true }), window, cx);
                                cx.stop_propagation();
                            }
                        }))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                            let press = PadPress {
                                deck,
                                pad,
                                mode: None,
                                press: Press::Tap,
                                shift: event.modifiers().shift,
                            };
                            this.perform(Command::Pad(press), window, cx);
                        }))
                })
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(5.0))
                        .when_some(face.dot, |row, colour| {
                            row.child(div().size(px(6.0)).rounded(px(2.0)).bg(colour))
                        })
                        .child(face.title),
                )
                .child(
                    div()
                        .font_family(MONO_FAMILY)
                        .text_size(theme::TINY_TEXT_SIZE)
                        .text_color(theme::MUTED)
                        .child(face.detail),
                )
        }))
}

/// What each pad shows in the deck's pad mode.
fn faces(app: &DubplateApp, deck: DeckId) -> Vec<PadFace> {
    let side = app.deck(deck);
    let mode = app.pad_modes[deck.index()];
    let active_length = app
        .loop_state(deck)
        .filter(|state| state.active)
        .and_then(|state| state.length);
    let sampler = app.sampler_state();
    (0..PAD_COUNT)
        .map(|pad| match mode {
            PadMode::HotCue => {
                let letter = HotCuePad::ALL.get(pad).copied();
                let cue = letter
                    .and_then(|letter| side.cues.hot_cue(letter))
                    .map(HotCue::start_seconds);
                PadFace {
                    title: letter
                        .map(|letter| letter.letter().to_string())
                        .unwrap_or_default()
                        .into(),
                    detail: cue.map(clock).unwrap_or_default().into(),
                    dot: cue.map(|_| theme::HOT_CUE_TINTS[pad % theme::HOT_CUE_TINTS.len()]),
                    lit: cue.is_some(),
                    playable: true,
                }
            }
            PadMode::BeatJump => {
                let beats =
                    beat_jump_beats(pad, app.beat_jump_scales[deck.index()]).unwrap_or_default();
                let title = if beats < 0.0 {
                    format!("\u{2190} {}", beats_label(beats))
                } else {
                    format!("{} \u{2192}", beats_label(beats))
                };
                PadFace {
                    title: title.into(),
                    detail: "beats".into(),
                    dot: None,
                    lit: false,
                    playable: true,
                }
            }
            PadMode::BeatLoop => {
                let length = BEAT_LOOP_LENGTHS[pad];
                let on = active_length == Some(length);
                PadFace {
                    title: length.to_string().into(),
                    detail: if on { "looping" } else { "beats" }.into(),
                    dot: on.then(|| theme::deck_colour(deck)),
                    lit: on,
                    playable: true,
                }
            }
            PadMode::PadFx1 | PadMode::PadFx2 => {
                let held = app.held_pads[deck.index()].is_some_and(|held| held.pad == pad);
                let (title, detail) = pad_fx(mode, pad, false)
                    .map(|fx| fx.label())
                    .unwrap_or_default();
                PadFace {
                    title: title.into(),
                    detail: detail.into(),
                    dot: held.then(|| theme::deck_colour(deck)),
                    lit: held,
                    playable: true,
                }
            }
            PadMode::Keyboard | PadMode::KeyShift => {
                let semitones = key_pad_semitones(pad, app.key_pad_offsets[deck.index()]);
                let on = semitones.is_some_and(|semitones| semitones == app.key_shift(deck));
                PadFace {
                    title: semitones.map(semitones_label).unwrap_or_default().into(),
                    detail: if mode == PadMode::Keyboard {
                        format!("cue {}", app.keyboard_cues[deck.index()].letter()).into()
                    } else {
                        SharedString::default()
                    },
                    dot: on.then(|| theme::deck_colour(deck)),
                    lit: on,
                    playable: semitones.is_some(),
                }
            }
            PadMode::Sampler => {
                let slot = sampler_slot(deck, pad);
                let playing = sampler
                    .as_ref()
                    .and_then(|state| state.slots.get(slot).copied().flatten())
                    .is_some_and(|slot_state| slot_state.playing);
                match app.sample_title(slot) {
                    Some(title) => PadFace {
                        title,
                        detail: format!("slot {}", slot + 1).into(),
                        dot: playing.then(|| theme::deck_colour(deck)),
                        lit: playing,
                        playable: true,
                    },
                    None => PadFace {
                        title: format!("Slot {}", slot + 1).into(),
                        detail: "empty".into(),
                        dot: None,
                        lit: false,
                        playable: true,
                    },
                }
            }
        })
        .collect()
}
