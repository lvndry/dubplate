//! The list of keyboard shortcuts, over the window.

use gpui::prelude::*;
use gpui::{ClickEvent, Context, FontWeight, MouseButton, div, px, relative};

use super::DubplateApp;
use crate::commands::Command;
use crate::fonts::MONO_FAMILY;
use crate::shortcuts::{HelpLine, HelpSection, help_sections};
use crate::theme;

/// How many sections, from the first, fill the left column; the rest fill the right one.
const LEFT_COLUMN_SECTIONS: usize = 2;

pub(super) fn render(cx: &mut Context<'_, DubplateApp>) -> impl IntoElement {
    let mut sections = help_sections();
    let right_sections = sections.split_off(LEFT_COLUMN_SECTIONS.min(sections.len()));
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
                .gap(px(16.0))
                .w(px(960.0))
                .max_h(relative(0.92))
                .overflow_y_scroll()
                .p(px(24.0))
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
                                .child("Shift plays the same key on deck B \u{b7} Esc closes"),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_start()
                        .gap(px(32.0))
                        .child(column(sections))
                        .child(column(right_sections)),
                ),
        )
}

fn column(sections: Vec<HelpSection>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(18.0))
        .flex_1()
        .flex_basis(px(0.0))
        .min_w_0()
        .children(sections.into_iter().map(section))
}

fn section(section: HelpSection) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .child(
            div()
                .pb(px(6.0))
                .text_size(theme::TINY_TEXT_SIZE)
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme::MUTED)
                .child(section.title),
        )
        .children(section.lines.into_iter().map(line))
}

fn line(line: HelpLine) -> impl IntoElement {
    let mut keys = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(4.0))
        .w(px(150.0));
    for (position, keystroke) in line.keys.into_iter().enumerate() {
        if position > 0 {
            keys = keys.child(
                div()
                    .text_size(theme::TINY_TEXT_SIZE)
                    .text_color(theme::MUTED)
                    .child("/"),
            );
        }
        keys = keys.child(keycap(keystroke));
    }
    div()
        .flex()
        .items_center()
        .gap(px(12.0))
        .py(px(5.0))
        .border_t_1()
        .border_color(theme::ROW_LINE)
        .child(keys)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(theme::SMALL_TEXT_SIZE)
                .text_color(theme::MUTED)
                .child(line.description),
        )
}

fn keycap(keystroke: String) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .h(px(20.0))
        .px(px(6.0))
        .rounded(px(4.0))
        .border_1()
        .border_color(theme::LINE)
        .bg(theme::RAISED)
        .font_family(MONO_FAMILY)
        .text_size(theme::TINY_TEXT_SIZE)
        .whitespace_nowrap()
        .child(keystroke)
}
