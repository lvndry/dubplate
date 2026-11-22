//! Small controls shared by the views, drawn the Graphite way.

use gpui::prelude::*;
use gpui::{
    AnyView, App, Context, Div, ElementId, MouseButton, SharedString, Stateful, Window, div, px,
};

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

/// A short explanation that shows under the pointer.
pub struct Tooltip(SharedString);

impl Render for Tooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<'_, Self>) -> impl IntoElement {
        div()
            .max_w(px(360.0))
            .px(px(8.0))
            .py(px(5.0))
            .rounded(theme::SMALL_RADIUS)
            .border_1()
            .border_color(theme::LINE)
            .bg(theme::RAISED)
            .text_color(theme::TEXT)
            .text_size(theme::SMALL_TEXT_SIZE)
            .child(self.0.clone())
    }
}

/// What `.tooltip()` takes to show `text` under the pointer.
pub fn tooltip(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView {
    let text = text.into();
    move |_, cx| cx.new(|_| Tooltip(text.clone())).into()
}
