//! The grid keys as buttons, shared by the track check and the decks' grid bars: the grid 5 ms
//! earlier or later, the downbeat a beat earlier or later, and confirming the grid by ear.

use dubplate_prep::{GridEdit, TrackGrid};
use gpui::prelude::*;
use gpui::{ClickEvent, Context, MouseButton, Pixels, div, px};

use super::DubplateApp;
use crate::commands::Command;
use crate::fonts::MONO_FAMILY;
use crate::shortcuts::GRID_NUDGE_MS;
use crate::theme;
use crate::widgets::InteractiveControl;

/// Room for the confirm button in both its states, so the controls beside it stay put when it
/// changes.
const CONFIRM_WIDTH: Pixels = px(136.0);
const BUTTON_HEIGHT: Pixels = px(24.0);

/// The grid controls for `grid`, each sending `command` with its edit. `scope` tells apart the
/// ids of controls shown more than once.
pub(super) fn render<Edit: Fn(GridEdit) -> Command>(
    scope: usize,
    grid: TrackGrid,
    command: Edit,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<Edit> {
    let correction = grid.correction;
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.0))
        .child(small_label("Grid"))
        .child(step_button(
            ("grid-earlier", scope),
            "\u{2039}",
            "Grid 5 ms earlier",
            command(GridEdit::Nudge(-GRID_NUDGE_MS)),
            cx,
        ))
        .child(reading(px(54.0), format!("{:+} ms", correction.offset_ms)))
        .child(step_button(
            ("grid-later", scope),
            "\u{203a}",
            "Grid 5 ms later",
            command(GridEdit::Nudge(GRID_NUDGE_MS)),
            cx,
        ))
        .child(small_label("Bar"))
        .child(step_button(
            ("downbeat-earlier", scope),
            "\u{2039}",
            "Downbeat one beat earlier",
            command(GridEdit::ShiftDownbeat(-1)),
            cx,
        ))
        .child(reading(px(28.0), format!("+{}", correction.downbeat_shift)))
        .child(step_button(
            ("downbeat-later", scope),
            "\u{203a}",
            "Downbeat one beat later",
            command(GridEdit::ShiftDownbeat(1)),
            cx,
        ))
        .child(confirm_button(
            scope,
            grid.confirmed,
            command(GridEdit::ToggleConfirmed),
            cx,
        ))
}

fn confirm_button(
    scope: usize,
    confirmed: bool,
    command: Command,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<> {
    div()
        .id(("confirm-grid", scope))
        .accessible_toggle("Beat grid confirmed", confirmed)
        .flex()
        .items_center()
        .justify_center()
        .w(CONFIRM_WIDTH)
        .h(BUTTON_HEIGHT)
        .px(px(8.0))
        .rounded(px(5.0))
        .border_1()
        .text_size(theme::SMALL_TEXT_SIZE)
        .cursor_pointer()
        .map(|button| {
            let colour = if confirmed {
                theme::OK
            } else {
                theme::ATTENTION
            };
            button.border_color(colour).text_color(colour)
        })
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            this.perform(command, window, cx);
        }))
        .child(if confirmed {
            "Grid confirmed \u{b7} G"
        } else {
            "Confirm grid \u{b7} G"
        })
}

fn step_button(
    id: (&'static str, usize),
    glyph: &'static str,
    description: &'static str,
    command: Command,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<> {
    div()
        .id(id)
        .accessible_button(description)
        .flex()
        .items_center()
        .justify_center()
        .h(BUTTON_HEIGHT)
        .min_w(BUTTON_HEIGHT)
        .px(px(6.0))
        .rounded(px(5.0))
        .border_1()
        .border_color(theme::LINE)
        .text_size(theme::SMALL_TEXT_SIZE)
        .text_color(theme::MUTED)
        .cursor_pointer()
        .hover(|button| button.text_color(theme::TEXT).bg(theme::RAISED))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            this.perform(command, window, cx);
        }))
        .child(glyph)
}

fn reading(width: Pixels, text: String) -> impl IntoElement {
    div()
        .w(width)
        .flex()
        .justify_center()
        .font_family(MONO_FAMILY)
        .text_size(theme::SMALL_TEXT_SIZE)
        .child(text)
}

fn small_label(text: &'static str) -> impl IntoElement {
    div()
        .text_size(theme::TINY_TEXT_SIZE)
        .text_color(theme::MUTED)
        .child(text)
}
