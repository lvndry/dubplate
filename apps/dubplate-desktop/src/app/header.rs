//! The header: the mark, the modes, the search field, and the state of the library and the
//! audio output.

use gpui::component::InteractiveElementExt;
use gpui::prelude::*;
use gpui::{
    ClickEvent, Context, FontWeight, MouseButton, Pixels, Rgba, SharedString, div, px, svg,
};

use super::DubplateApp;
use crate::assets::LOGO_PATH;
use crate::browser::{LibraryStatus, search_field};
use crate::commands::Command;
use crate::fonts::MONO_FAMILY;
use crate::theme;

/// Room for the window's close, minimise and zoom buttons, which sit over the header.
const WINDOW_BUTTONS_WIDTH: Pixels = px(68.0);

pub(super) fn render(app: &DubplateApp, cx: &mut Context<'_, DubplateApp>) -> impl IntoElement {
    div()
        .id("header")
        .flex()
        .flex_none()
        .items_center()
        .gap(px(18.0))
        .h(px(44.0))
        .pl(theme::GUTTER + WINDOW_BUTTONS_WIDTH)
        .pr(theme::GUTTER)
        .border_b_1()
        .border_color(theme::LINE)
        .on_double_click(|_, window, _| window.titlebar_double_click())
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, _| this.header_drag_armed = true),
        )
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, _, _, _| this.header_drag_armed = false),
        )
        .on_mouse_down_out(cx.listener(|this, _, _, _| this.header_drag_armed = false))
        .on_mouse_move(cx.listener(|this, _, window, _| {
            if this.header_drag_armed {
                this.header_drag_armed = false;
                window.start_window_move();
            }
        }))
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(8.0))
                .w(px(120.0))
                .child(svg().path(LOGO_PATH).size(px(20.0)).text_color(theme::TEXT))
                .child(
                    div()
                        .text_size(px(19.0))
                        .font_weight(FontWeight::MEDIUM)
                        .line_height(px(19.0))
                        .child("dubplate"),
                ),
        )
        .child(
            div()
                .flex()
                .flex_none()
                .gap(px(2.0))
                .child(mode("Prepare", false))
                .child(mode("Perform", true))
                .child(mode("Edit", false)),
        )
        .child(
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .justify_center()
                .child(search_field(app.browser.read(cx))),
        )
        .child(status(app, cx))
}

fn mode(label: &'static str, current: bool) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .h(px(28.0))
        .px(px(10.0))
        .rounded(theme::SMALL_RADIUS)
        .when(current, |mode| {
            mode.bg(theme::RAISED)
                .text_color(theme::TEXT)
                .font_weight(FontWeight::MEDIUM)
        })
        .when(!current, |mode| mode.text_color(theme::MUTED))
        .child(label)
}

fn status(app: &DubplateApp, cx: &mut Context<'_, DubplateApp>) -> impl IntoElement {
    let (library_colour, library_text): (Rgba, SharedString) =
        match (app.sources.files_seen(), &app.library) {
            (Some(files), _) => (
                theme::MUTED,
                format!("Scanning\u{2026} {files} files").into(),
            ),
            (None, LibraryStatus::Reading) => (theme::MUTED, "Opening library\u{2026}".into()),
            (None, LibraryStatus::Ready(collection)) => (
                theme::OK,
                format!("{} tracks", collection.track_count()).into(),
            ),
            (None, LibraryStatus::Unchosen) => (theme::MUTED, "No music yet".into()),
        };
    let (audio_colour, audio_text): (Rgba, SharedString) = match &app.engine {
        Ok(engine) => {
            let stats = engine.controller.stats();
            let latency = stats.output_latency_ms.unwrap_or(stats.buffer_latency_ms);
            (
                theme::MUTED,
                format!("{} \u{b7} {latency:.1} ms", engine.output.device_name()).into(),
            )
        }
        Err(_) => (theme::ALERT, "No audio output".into()),
    };
    let dropouts = app.dropouts();
    let controllers = app.controller_names();
    let (controller_colour, controller_text): (Rgba, SharedString) = if controllers.is_empty() {
        (theme::MUTED, "No controller".into())
    } else {
        let others = controllers.len() - 1;
        let name = controllers.first().copied().unwrap_or_default();
        let text = if others == 0 {
            name.to_owned()
        } else {
            format!("{name} +{others}")
        };
        (theme::OK, text.into())
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(14.0))
        .text_size(theme::SMALL_TEXT_SIZE)
        .text_color(theme::MUTED)
        .child(
            div()
                .id("library-status")
                .flex()
                .items_center()
                .gap(px(6.0))
                .h(px(28.0))
                .px(px(8.0))
                .rounded(theme::CONTROL_RADIUS)
                .cursor_pointer()
                .hover(|chip| chip.bg(theme::RAISED))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.perform(Command::ToggleLibrary, window, cx);
                }))
                .child(dot(library_colour))
                .child(library_text),
        )
        .child(
            div()
                .id("controller-status")
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(dot(controller_colour))
                .child(controller_text),
        )
        .child(
            div()
                .font_family(MONO_FAMILY)
                .text_color(audio_colour)
                .child(audio_text),
        )
        .when(dropouts > 0, |status| {
            status.child(
                div()
                    .font_family(MONO_FAMILY)
                    .text_color(theme::ALERT)
                    .child(format!(
                        "{dropouts} dropout{}",
                        if dropouts == 1 { "" } else { "s" }
                    )),
            )
        })
        .child(
            div()
                .id("lock-playing-deck")
                .flex()
                .items_center()
                .gap(px(6.0))
                .h(px(28.0))
                .px(px(10.0))
                .rounded(theme::CONTROL_RADIUS)
                .border_1()
                .border_color(theme::LINE)
                .text_color(if app.lock_playing_deck {
                    theme::TEXT
                } else {
                    theme::MUTED
                })
                .cursor_pointer()
                .hover(|button| button.bg(theme::RAISED))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.toggle_lock_playing_deck(cx);
                }))
                .child(dot(if app.lock_playing_deck {
                    theme::OK
                } else {
                    theme::MUTED
                }))
                .child("Lock playing deck"),
        )
        .child(
            div()
                .id("help-button")
                .flex()
                .items_center()
                .gap(px(6.0))
                .h(px(28.0))
                .px(px(10.0))
                .rounded(theme::CONTROL_RADIUS)
                .border_1()
                .border_color(theme::LINE)
                .bg(theme::PANEL)
                .text_color(theme::TEXT)
                .cursor_pointer()
                .hover(|button| button.bg(theme::RAISED))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.perform(Command::ToggleHelp, window, cx);
                }))
                .child("Keys")
                .child(
                    div()
                        .font_family(MONO_FAMILY)
                        .text_size(theme::TINY_TEXT_SIZE)
                        .text_color(theme::MUTED)
                        .child("?"),
                ),
        )
}

fn dot(colour: Rgba) -> impl IntoElement {
    div().size(px(6.0)).rounded_full().bg(colour)
}
