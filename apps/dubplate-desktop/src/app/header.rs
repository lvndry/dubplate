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
use crate::commands::{Command, Mode};
use crate::fonts::MONO_FAMILY;
use crate::set::running_time;
use crate::theme;
use crate::widgets::{InteractiveControl, tooltip};

/// The search field never gets narrower than this: the status beside it gives way first.
const SEARCH_MIN_WIDTH: Pixels = px(170.0);
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
        .border_color(theme::active().line)
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
                .child(
                    svg()
                        .path(LOGO_PATH)
                        .size(px(20.0))
                        .text_color(theme::active().text),
                )
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
                .child(mode("Prepare", Mode::Prepare, app.mode, cx))
                .child(mode("Perform", Mode::Perform, app.mode, cx)),
        )
        .child(
            div()
                .flex()
                .flex_1()
                .min_w(SEARCH_MIN_WIDTH)
                .justify_center()
                .child(search_field(app.browser.read(cx))),
        )
        .child(status(app, cx))
}

/// A mode button.
fn mode(
    label: &'static str,
    mode: Mode,
    shown: Mode,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<> {
    let current = mode == shown;
    div()
        .id(label)
        .accessible_toggle(label, current)
        .flex()
        .items_center()
        .h(px(28.0))
        .px(px(10.0))
        .rounded(theme::SMALL_RADIUS)
        .cursor_pointer()
        .when(current, |button| {
            button
                .bg(theme::active().raised)
                .text_color(theme::active().text)
                .font_weight(FontWeight::MEDIUM)
        })
        .when(!current, |button| {
            button
                .text_color(theme::active().muted)
                .hover(|button| button.text_color(theme::active().text))
        })
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            this.perform(Command::SwitchMode(mode), window, cx);
        }))
        .child(label)
}

fn status(app: &DubplateApp, cx: &mut Context<'_, DubplateApp>) -> impl IntoElement {
    let (library_colour, library_text): (Rgba, SharedString) =
        match (app.sources.files_seen(), &app.library) {
            (Some(files), _) => (
                theme::active().muted,
                format!("Scanning\u{2026} {files} files").into(),
            ),
            (None, LibraryStatus::Reading) => {
                (theme::active().muted, "Opening library\u{2026}".into())
            }
            (None, LibraryStatus::Ready(collection)) => (
                theme::active().ok,
                match app.analysis_waiting() {
                    0 => format!("{} tracks", collection.track_count()),
                    waiting => format!(
                        "{} tracks \u{b7} {waiting} to analyse",
                        collection.track_count()
                    ),
                }
                .into(),
            ),
            (None, LibraryStatus::Unchosen) => (theme::active().muted, "No music yet".into()),
        };
    let (audio_colour, audio_text): (Rgba, SharedString) = match &app.engine {
        Ok(engine) => {
            let stats = engine.controller.stats();
            let latency = stats.output_latency_ms.unwrap_or(stats.buffer_latency_ms);
            (
                theme::active().muted,
                format!("{} \u{b7} {latency:.1} ms", engine.output.device_name()).into(),
            )
        }
        Err(_) => (theme::active().alert, "No audio output".into()),
    };
    let controllers = app.controller_names();
    let (controller_colour, controller_text): (Rgba, SharedString) = if controllers.is_empty() {
        (theme::active().muted, "No controller".into())
    } else {
        let others = controllers.len() - 1;
        let name = controllers.first().copied().unwrap_or_default();
        let text = if others == 0 {
            name.to_owned()
        } else {
            format!("{name} +{others}")
        };
        (theme::active().ok, text.into())
    };
    div()
        .flex()
        .flex_shrink(1.0)
        .min_w_0()
        .items_center()
        .gap(px(10.0))
        .text_size(theme::SMALL_TEXT_SIZE)
        .text_color(theme::active().muted)
        .child(
            div()
                .id("library-status")
                .accessible_button("Music library settings")
                .flex()
                .flex_shrink(1.0)
                .min_w_0()
                .items_center()
                .gap(px(6.0))
                .h(px(28.0))
                .px(px(8.0))
                .rounded(theme::CONTROL_RADIUS)
                .cursor_pointer()
                .hover(|chip| chip.bg(theme::active().raised))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.perform(Command::ToggleLibrary, window, cx);
                }))
                .child(dot(library_colour))
                .child(div().truncate().child(library_text)),
        )
        .child({
            let narrow = app.narrow_window();
            div()
                .id("controller-status")
                .flex()
                .flex_shrink(1.0)
                .min_w_0()
                .items_center()
                .gap(px(6.0))
                .when(narrow, |status| {
                    status.tooltip(tooltip(format!("{controller_text} \u{b7} {audio_text}")))
                })
                .child(dot(controller_colour))
                .when(!narrow, |status| {
                    status
                        .child(div().truncate().child(controller_text.clone()))
                        .child(
                            div()
                                .flex_shrink(1.0)
                                .min_w_0()
                                .truncate()
                                .text_color(audio_colour)
                                .child(audio_text.clone()),
                        )
                })
        })
        .children(preview_pill(app, cx))
        .child(record_button(app, cx))
        .child(header_switch(
            "quantize",
            "Quantize",
            app.quantize(),
            Command::ToggleQuantize,
            cx,
        ))
        .child(header_switch(
            "lock-playing-deck",
            "Lock playing deck",
            app.lock_playing_deck,
            Command::ToggleLockPlayingDeck,
            cx,
        ))
        .child(
            div()
                .id("help-button")
                .accessible_button("Keys: keyboard shortcuts")
                .flex()
                .items_center()
                .gap(px(6.0))
                .h(px(28.0))
                .px(px(10.0))
                .rounded(theme::CONTROL_RADIUS)
                .border_1()
                .border_color(theme::active().line)
                .bg(theme::active().panel)
                .text_color(theme::active().text)
                .cursor_pointer()
                .hover(|button| button.bg(theme::active().raised))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.perform(Command::ToggleHelp, window, cx);
                }))
                .child("Keys")
                .child(
                    div()
                        .font_family(MONO_FAMILY)
                        .text_size(theme::TINY_TEXT_SIZE)
                        .text_color(theme::active().muted)
                        .child("?"),
                ),
        )
}

/// REC: a red dot and the running time while recording, amber once audio was skipped.
fn record_button(app: &DubplateApp, cx: &mut Context<'_, DubplateApp>) -> impl IntoElement {
    let status = app.recording_status();
    let (colour, label): (Rgba, SharedString) = match &status {
        Some(status) if !status.skipped.is_zero() => (
            theme::active().attention,
            format!(
                "REC {} \u{b7} {:.1} s skipped",
                running_time(status.elapsed),
                status.skipped.as_secs_f64()
            )
            .into(),
        ),
        Some(status) => (
            theme::active().alert,
            format!("REC {}", running_time(status.elapsed)).into(),
        ),
        None => (theme::active().muted, "Rec".into()),
    };
    div()
        .id("record")
        .accessible_toggle("Record mix", app.is_recording())
        .flex()
        .items_center()
        .gap(px(6.0))
        .h(px(28.0))
        .px(px(10.0))
        .rounded(theme::CONTROL_RADIUS)
        .border_1()
        .border_color(if status.is_some() {
            theme::faded(colour, 0.6)
        } else {
            theme::active().line
        })
        .text_color(if status.is_some() {
            colour
        } else {
            theme::active().muted
        })
        .when(status.is_some(), |button| button.font_family(MONO_FAMILY))
        .cursor_pointer()
        .hover(|button| button.bg(theme::active().raised))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
            this.perform(Command::ToggleRecording, window, cx);
        }))
        .child(dot(colour))
        .child(label)
}

/// The track playing in the headphones from the library, with a click to stop it.
fn preview_pill(app: &DubplateApp, cx: &mut Context<'_, DubplateApp>) -> Option<impl IntoElement> {
    let index = app.previewed_track()?;
    let title = app.collection()?.track(index).row.title.clone();
    Some(
        div()
            .id("track-preview")
            .accessible_button(format!("Stop previewing {title}"))
            .flex()
            .items_center()
            .gap(px(6.0))
            .max_w(px(220.0))
            .h(px(28.0))
            .px(px(10.0))
            .rounded(theme::CONTROL_RADIUS)
            .border_1()
            .border_color(theme::active().line)
            .cursor_pointer()
            .hover(|pill| pill.bg(theme::active().raised))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.stop_track_preview(cx);
            }))
            .child(dot(theme::active().ok))
            .child(
                div()
                    .flex_none()
                    .text_color(theme::active().muted)
                    .child("Preview"),
            )
            .child(div().min_w_0().truncate().child(title)),
    )
}

/// A switch in the header that applies to both decks, with a dot that is green while it is on.
fn header_switch(
    id: &'static str,
    name: &'static str,
    on: bool,
    command: Command,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<> {
    div()
        .id(id)
        .accessible_toggle(name, on)
        .flex()
        .items_center()
        .gap(px(6.0))
        .h(px(28.0))
        .px(px(10.0))
        .rounded(theme::CONTROL_RADIUS)
        .border_1()
        .border_color(theme::active().line)
        .text_color(if on {
            theme::active().text
        } else {
            theme::active().muted
        })
        .cursor_pointer()
        .hover(|button| button.bg(theme::active().raised))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            this.perform(command, window, cx);
        }))
        .child(dot(if on {
            theme::active().ok
        } else {
            theme::active().muted
        }))
        .child(name)
}

fn dot(colour: Rgba) -> impl IntoElement {
    div().size(px(6.0)).rounded_full().bg(colour)
}
