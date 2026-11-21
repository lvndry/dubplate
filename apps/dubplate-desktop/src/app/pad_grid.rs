//! A deck's pad section: the four pad mode buttons with their SHIFT modes, the loop buttons, and
//! the eight pads labelled for the mode they are in.

use dubplate_engine::DeckId;
use dubplate_midi::PadMode;
use dubplate_rekordbox::HotCuePad;
use gpui::prelude::*;
use gpui::{ClickEvent, Context, FontWeight, MouseButton, Pixels, Rgba, SharedString, div, px};

use super::DubplateApp;
use crate::collection::clock;
use crate::commands::{Command, LoopAction, PadPress, Press};
use crate::fonts::MONO_FAMILY;
use crate::pads::{
    BEAT_LOOP_LENGTHS, PAD_COUNT, beat_jump_beats, beats_label, mode_buttons, mode_is_playable,
    mode_label,
};
use crate::theme;

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
                .child(loop_buttons(app, deck, cx)),
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
    let loaded = app.deck(deck).visuals().is_some();
    let faces = faces(app, deck);
    div()
        .flex()
        .gap(px(4.0))
        .children(faces.into_iter().enumerate().map(|(pad, face)| {
            let playable = loaded && face.playable;
            div()
                .id(("pad", deck.index() * PAD_COUNT + pad))
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
    (0..PAD_COUNT)
        .map(|pad| match mode {
            PadMode::HotCue => {
                let letter = HotCuePad::ALL.get(pad).copied();
                let cue = letter.and_then(|letter| side.hot_cues.get(letter));
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
            PadMode::PadFx1
            | PadMode::PadFx2
            | PadMode::Sampler
            | PadMode::Keyboard
            | PadMode::KeyShift => PadFace {
                title: (pad + 1).to_string().into(),
                detail: if pad == 0 && !mode_is_playable(mode) {
                    "coming soon"
                } else {
                    ""
                }
                .into(),
                dot: None,
                lit: false,
                playable: false,
            },
        })
        .collect()
}
