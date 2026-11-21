//! Small controls shared by the views, drawn the Graphite way.

use gpui::prelude::*;
use gpui::{Div, ElementId, MouseButton, SharedString, Stateful, div, px};

use crate::theme;

/// How much a button stands out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Emphasis {
    /// The one thing to do next.
    Primary,
    /// Everything else.
    Secondary,
}

/// A text button. The caller adds what a click does.
pub fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    emphasis: Emphasis,
) -> Stateful<Div> {
    let base = div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .h(px(28.0))
        .px(px(10.0))
        .rounded(theme::CONTROL_RADIUS)
        .text_size(theme::SMALL_TEXT_SIZE)
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(label.into());
    match emphasis {
        Emphasis::Primary => base
            .bg(theme::TEXT)
            .text_color(theme::BACKGROUND)
            .font_weight(gpui::FontWeight::MEDIUM)
            .hover(|button| button.opacity(0.9)),
        Emphasis::Secondary => base
            .border_1()
            .border_color(theme::LINE)
            .bg(theme::PANEL)
            .text_color(theme::TEXT)
            .hover(|button| button.bg(theme::RAISED)),
    }
}
