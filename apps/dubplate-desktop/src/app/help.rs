//! The list of keyboard shortcuts, over the window.

use gpui::prelude::*;
use gpui::{ClickEvent, Context, FontWeight, MouseButton, div, px};

use super::DubplateApp;
use crate::fonts::MONO_FAMILY;
use crate::shortcuts::{Command, help_lines};
use crate::theme;

pub(super) fn render(cx: &mut Context<'_, DubplateApp>) -> impl IntoElement {
    div()
        .id("help")
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(theme::SCRIM)
        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
            this.perform(Command::ToggleHelp, window, cx);
        }))
        .child(
            div()
                .id("help-card")
                .flex()
                .flex_col()
                .gap(px(10.0))
                .w(px(520.0))
                .p(px(20.0))
                .rounded(px(10.0))
                .border_1()
                .border_color(theme::LINE)
                .bg(theme::PANEL)
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
                                .child("Keyboard"),
                        )
                        .child(
                            div()
                                .text_size(theme::SMALL_TEXT_SIZE)
                                .text_color(theme::MUTED)
                                .child("Shift plays the same key on deck B"),
                        ),
                )
                .children(help_lines().into_iter().map(|line| {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(16.0))
                        .py(px(4.0))
                        .border_t_1()
                        .border_color(theme::ROW_LINE)
                        .child(
                            div()
                                .w(px(170.0))
                                .flex_none()
                                .font_family(MONO_FAMILY)
                                .text_size(theme::SMALL_TEXT_SIZE)
                                .child(line.keys),
                        )
                        .child(div().text_color(theme::MUTED).child(line.description))
                })),
        )
}
