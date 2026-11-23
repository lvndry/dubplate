//! The Pad FX editor, over the window: the 32 Pad FX pads in their four layers, and for the pad
//! chosen its sound, beat or parameter, depth, how it is triggered and what letting go
//! does, with the built-in presets. Changes apply at once and are remembered.

use dubplate_engine::{DeckId, PadRelease};
use gpui::base::FocusTrapElement;
use gpui::prelude::*;
use gpui::{ClickEvent, Context, FontWeight, MouseButton, SharedString, div, px};

use super::DubplateApp;
use crate::commands::{Command, PadFxEdit};
use crate::fonts::MONO_FAMILY;
use crate::pad_fx::{PadFxLayer, PadFxPreset, PadFxSlot, PadTrigger};
use crate::pads::PAD_COUNT;
use crate::theme;
use crate::widgets::{Emphasis, InteractiveControl, button};

/// The pad the editor is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PadFxEditing {
    /// Its layer.
    pub layer: PadFxLayer,
    /// The pad, 0 for pad 1.
    pub pad: usize,
}

impl DubplateApp {
    /// Whether a deck's pads show and play their SHIFT layer: SHIFT is held on the controller,
    /// or the SHIFT layer was chosen on screen.
    pub(super) fn pads_shifted(&self, deck: DeckId) -> bool {
        self.shift_held[deck.index()] || self.shift_layer_shown[deck.index()]
    }

    /// Carries out a change in the Pad FX editor.
    pub(super) fn edit_pad_fx(&mut self, edit: PadFxEdit, cx: &mut Context<'_, Self>) {
        let editing = self.pad_fx_editor;
        let change = |change: fn(PadFxSlot, i32) -> PadFxSlot, steps: i32| {
            move |slot: PadFxSlot| change(slot, steps)
        };
        match (edit, editing) {
            (PadFxEdit::Toggle(layer), None) => {
                self.pad_fx_editor = Some(PadFxEditing { layer, pad: 0 });
                self.library_open = false;
                self.help_open = false;
                return;
            }
            (PadFxEdit::Toggle(_) | PadFxEdit::Close, _) => {
                self.pad_fx_editor = None;
                return;
            }
            (PadFxEdit::Select(layer, pad), _) => {
                if pad < PAD_COUNT {
                    self.pad_fx_editor = Some(PadFxEditing { layer, pad });
                }
                return;
            }
            (PadFxEdit::Preset(preset), _) => {
                self.pad_fx = crate::pad_fx::PadFxBanks::from_preset(preset);
            }
            (_, None) => return,
            (PadFxEdit::Sound(steps), Some(on)) => {
                self.pad_fx
                    .edit(on.layer, on.pad, change(PadFxSlot::step_sound, steps));
            }
            (PadFxEdit::Parameter(steps), Some(on)) => {
                self.pad_fx
                    .edit(on.layer, on.pad, change(PadFxSlot::step_parameter, steps));
            }
            (PadFxEdit::Depth(steps), Some(on)) => {
                self.pad_fx
                    .edit(on.layer, on.pad, change(PadFxSlot::step_depth, steps));
            }
            (PadFxEdit::Trigger, Some(on)) => {
                self.pad_fx
                    .edit(on.layer, on.pad, PadFxSlot::toggle_trigger);
            }
            (PadFxEdit::Release, Some(on)) => {
                self.pad_fx
                    .edit(on.layer, on.pad, PadFxSlot::toggle_release);
            }
            (PadFxEdit::ResetPad, Some(on)) => self.pad_fx.reset_pad(on.layer, on.pad),
        }
        self.settings.pad_fx = self.pad_fx.setting();
        self.save_settings_soon(cx);
    }
}

pub(super) fn render(
    app: &DubplateApp,
    editing: PadFxEditing,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement {
    let preset = app.pad_fx.preset();
    let summary = if app.pad_fx.edited() {
        format!("{}, edited", preset.label())
    } else {
        preset.label().to_owned()
    };
    div()
        .id("pad-fx-editor")
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(theme::active().scrim)
        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
            this.perform(Command::PadFx(PadFxEdit::Close), window, cx);
        }))
        .child(
            div()
                .id("pad-fx-card")
                .role(gpui::Role::Dialog)
                .aria_label("Pad FX editor")
                .key_context("Modal")
                .track_focus(&app.modal_focus)
                .tab_group()
                .tab_stop(false)
                .flex()
                .flex_col()
                .gap(px(14.0))
                .w(px(720.0))
                .p(px(20.0))
                .rounded(px(10.0))
                .border_1()
                .border_color(theme::active().line)
                .bg(theme::active().panel)
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .items_baseline()
                        .child(
                            div()
                                .text_size(px(15.0))
                                .font_weight(FontWeight::MEDIUM)
                                .child("Pad FX"),
                        )
                        .child(
                            div()
                                .text_size(theme::SMALL_TEXT_SIZE)
                                .text_color(theme::active().muted)
                                .child(format!(
                                    "{summary} \u{b7} PAD FX1 and FX2, SHIFT for the second eight \u{b7} Esc closes"
                                )),
                        )
                        .child(
                            button("close-pad-fx", "Close", Emphasis::Secondary).on_click(
                                cx.listener(|this, _: &ClickEvent, window, cx| {
                                    this.perform(Command::PadFx(PadFxEdit::Close), window, cx)
                                }),
                            ),
                        ),
                )
                .child(presets(preset, cx))
                .child(
                    div()
                        .flex()
                        .gap(px(18.0))
                        .child(layers(app, editing, cx))
                        .child(pad_settings(app, editing, cx)),
                )
                .focus_trap("pad-fx-focus-trap", &app.modal_focus),
        )
}

/// The presets: a click replaces all 32 pads with the preset's.
fn presets(current: PadFxPreset, cx: &mut Context<'_, DubplateApp>) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(
            div()
                .text_size(theme::TINY_TEXT_SIZE)
                .text_color(theme::active().muted)
                .child("Start from"),
        )
        .children(PadFxPreset::ALL.map(|preset| {
            let emphasis = if preset == current {
                Emphasis::Primary
            } else {
                Emphasis::Secondary
            };
            button(("pad-fx-preset", preset as usize), preset.label(), emphasis).on_click(
                cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.perform(Command::PadFx(PadFxEdit::Preset(preset)), window, cx)
                }),
            )
        }))
}

/// The four layers of eight pads, laid out as on the controller; a click edits a pad.
fn layers(
    app: &DubplateApp,
    editing: PadFxEditing,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .children(PadFxLayer::ALL.map(|layer| {
            let pads = (0..PAD_COUNT).map(|pad| {
                let chosen = editing == PadFxEditing { layer, pad };
                let slot = app.pad_fx.slot(layer, pad);
                let (name, parameter) = slot.map(PadFxSlot::label).unwrap_or_default();
                let detail = if slot.is_some_and(|slot| slot.trigger == PadTrigger::Latch) {
                    format!("{parameter} latch")
                } else {
                    parameter
                };
                div()
                    .id(("pad-fx-slot", layer.index() * PAD_COUNT + pad))
                    .accessible_button(format!(
                        "{} pad {}: {name} {detail}",
                        layer.label(),
                        pad + 1
                    ))
                    .flex()
                    .flex_col()
                    .justify_center()
                    .w(px(78.0))
                    .h(px(36.0))
                    .px(px(6.0))
                    .rounded(theme::SMALL_RADIUS)
                    .border_1()
                    .bg(theme::active().raised)
                    .cursor_pointer()
                    .map(|slot| {
                        if chosen {
                            slot.border_color(theme::active().text)
                        } else {
                            slot.border_color(theme::active().line)
                                .hover(|slot| slot.border_color(theme::active().muted))
                        }
                    })
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.perform(Command::PadFx(PadFxEdit::Select(layer, pad)), window, cx)
                    }))
                    .child(
                        div()
                            .text_size(theme::SMALL_TEXT_SIZE)
                            .overflow_hidden()
                            .child(name),
                    )
                    .child(
                        div()
                            .font_family(MONO_FAMILY)
                            .text_size(theme::TINY_TEXT_SIZE)
                            .text_color(theme::active().muted)
                            .child(detail),
                    )
            });
            let pads: Vec<_> = pads.collect();
            let mut rows = pads.into_iter();
            let top: Vec<_> = rows.by_ref().take(PAD_COUNT / 2).collect();
            let bottom: Vec<_> = rows.collect();
            div()
                .flex()
                .flex_col()
                .gap(px(3.0))
                .child(
                    div()
                        .text_size(theme::TINY_TEXT_SIZE)
                        .text_color(theme::active().muted)
                        .child(format!("{}  \u{b7}  {}", layer.label(), layer.numbers())),
                )
                .child(div().flex().gap(px(3.0)).children(top))
                .child(div().flex().gap(px(3.0)).children(bottom))
        }))
}

/// The chosen pad's settings.
fn pad_settings(
    app: &DubplateApp,
    editing: PadFxEditing,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement {
    let Some(slot) = app.pad_fx.slot(editing.layer, editing.pad) else {
        return div();
    };
    let parameter_name = slot.sound.parameter().map_or("Parameter", |spec| spec.name);
    let parameter = slot
        .parameter
        .map_or_else(|| "\u{2013}".to_owned(), |value| value.to_string());
    let depth = if slot.sound.has_release() {
        format!("{:.0}%", slot.depth * 100.0)
    } else {
        "\u{2013}".to_owned()
    };
    let trigger = match slot.trigger {
        PadTrigger::Hold => "Hold",
        PadTrigger::Latch => "Latch",
        PadTrigger::Tap => "Tap",
    };
    let tap = slot.trigger == PadTrigger::Tap;
    let release = if !slot.sound.has_release() {
        "Lands in time"
    } else if tap || slot.release == PadRelease::RingOut {
        "Ring out"
    } else {
        "Back to dry"
    };
    div()
        .flex()
        .flex_col()
        .flex_1()
        .gap(px(10.0))
        .child(
            div()
                .text_size(theme::SMALL_TEXT_SIZE)
                .font_weight(FontWeight::MEDIUM)
                .child(format!(
                    "{} pad {} (effect {})",
                    editing.layer.label(),
                    editing.pad + 1,
                    editing.layer.index() * PAD_COUNT + editing.pad + 1
                )),
        )
        .child(stepper(
            "Sound",
            slot.sound.label().to_owned(),
            PadFxEdit::Sound,
            cx,
        ))
        .child(stepper(parameter_name, parameter, PadFxEdit::Parameter, cx))
        .child(stepper("Depth", depth, PadFxEdit::Depth, cx))
        .child(toggle("Pad", trigger, PadFxEdit::Trigger, true, cx))
        .child(toggle(
            "Let go",
            release,
            PadFxEdit::Release,
            slot.sound.has_release() && !tap,
            cx,
        ))
        .child(div().pt(px(4.0)).child(
            button("pad-fx-reset", "Reset pad", Emphasis::Secondary).on_click(cx.listener(
                |this, _: &ClickEvent, window, cx| {
                    this.perform(Command::PadFx(PadFxEdit::ResetPad), window, cx)
                },
            )),
        ))
}

fn field(name: &'static str) -> impl IntoElement {
    div()
        .w(px(70.0))
        .flex_none()
        .text_size(theme::TINY_TEXT_SIZE)
        .text_color(theme::active().muted)
        .child(name)
}

/// A value with arrows either side that step it.
fn stepper(
    name: &'static str,
    value: String,
    edit: fn(i32) -> PadFxEdit,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement {
    let arrow = |glyph: &'static str, steps: i32| {
        let direction = if steps < 0 { "down" } else { "up" };
        div()
            .id(SharedString::from(format!("pad-fx-{name}-{direction}")))
            .accessible_button(format!("{name} {direction}"))
            .flex()
            .items_center()
            .justify_center()
            .w(px(24.0))
            .h(px(24.0))
            .rounded(px(5.0))
            .text_color(theme::active().muted)
            .cursor_pointer()
            .hover(|arrow| {
                arrow
                    .bg(theme::active().raised)
                    .text_color(theme::active().text)
            })
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.perform(Command::PadFx(edit(steps)), window, cx)
            }))
            .child(glyph)
    };
    div()
        .flex()
        .items_center()
        .child(field(name))
        .child(arrow("\u{2039}", -1))
        .child(
            div()
                .min_w(px(130.0))
                .flex()
                .justify_center()
                .font_family(MONO_FAMILY)
                .text_size(theme::SMALL_TEXT_SIZE)
                .child(value),
        )
        .child(arrow("\u{203a}", 1))
}

/// A value a click cycles when it can be edited.
fn toggle(
    name: &'static str,
    value: &'static str,
    edit: PadFxEdit,
    enabled: bool,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement {
    div().flex().items_center().child(field(name)).child(
        div()
            .id(SharedString::from(format!("pad-fx-{name}")))
            .min_w(px(178.0))
            .h(px(24.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(5.0))
            .border_1()
            .border_color(theme::active().line)
            .font_family(MONO_FAMILY)
            .text_size(theme::SMALL_TEXT_SIZE)
            .map(|toggle| {
                if enabled {
                    toggle
                        .accessible_button(format!("{name}: {value}"))
                        .cursor_pointer()
                        .hover(|toggle| toggle.bg(theme::active().raised))
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.perform(Command::PadFx(edit), window, cx)
                        }))
                } else {
                    toggle.text_color(theme::active().muted)
                }
            })
            .child(value),
    )
}
