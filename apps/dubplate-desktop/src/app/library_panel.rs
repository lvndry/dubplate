//! The settings, over the window: the music folders with their tracks and last scan,
//! rekordbox's collection as a separate choice, and the sound card Dubplate plays on.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use gpui::base::FocusTrapElement;
use gpui::prelude::*;
use gpui::{ClickEvent, Context, FontWeight, MouseButton, Rgba, SharedString, div, px};

use super::DubplateApp;
use crate::collection::folder_name;
use crate::commands::Command;
use crate::library::RekordboxSource;
use crate::theme;
use crate::widgets::InteractiveControl;
use crate::widgets::{Emphasis, button};
use dubplate_engine::OutputDevice;

pub(super) fn render(app: &DubplateApp, cx: &mut Context<'_, DubplateApp>) -> impl IntoElement {
    let mut output_rows = Vec::new();
    for (position, device) in app.output_devices().iter().enumerate() {
        output_rows.push(output_row(app, position, device, cx));
    }
    let mut folder_rows = Vec::new();
    for (position, folder) in app.sources.folders().iter().enumerate() {
        folder_rows.push(folder_row(app, position, folder, cx));
    }
    div()
        .id("library-settings")
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(theme::SCRIM)
        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
            this.perform(Command::ToggleLibrary, window, cx);
        }))
        .child(
            div()
                .id("library-card")
                .role(gpui::Role::Dialog)
                .aria_label("Music and audio settings")
                .key_context("Modal")
                .track_focus(&app.modal_focus)
                .tab_group()
                .tab_stop(false)
                .flex()
                .flex_col()
                .gap(px(14.0))
                .w(px(560.0))
                .max_h(px(620.0))
                .overflow_y_scroll()
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
                                .child("Settings"),
                        )
                        .child(
                            button("close-settings", "Close", Emphasis::Secondary).on_click(
                                cx.listener(|this, _: &ClickEvent, window, cx| {
                                    this.perform(Command::ToggleLibrary, window, cx)
                                }),
                            ),
                        )
                        .child(
                            div()
                                .text_size(theme::SMALL_TEXT_SIZE)
                                .text_color(theme::MUTED)
                                .child("Dubplate never changes your files"),
                        ),
                )
                .child(section_title("Music folders"))
                .children(folder_rows)
                .when(app.sources.folders().is_empty(), |card| {
                    card.child(
                        div()
                            .text_color(theme::MUTED)
                            .child("No folders yet. Add the folders that hold your tracks."),
                    )
                })
                .child(div().flex().child(
                    button("add-folders", "Add folders\u{2026}", Emphasis::Primary).on_click(
                        cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.perform(Command::AddMusicFolders, window, cx);
                        }),
                    ),
                ))
                .child(section_title("rekordbox"))
                .child(rekordbox_row(app, cx))
                .child(section_title("Decks"))
                .child(key_lock_row(app, cx))
                .child(fader_echo_row(app, cx))
                .child(section_title("Sound card"))
                .children(output_rows)
                .focus_trap("settings-focus-trap", &app.modal_focus),
        )
}

fn section_title(text: &'static str) -> impl IntoElement {
    div()
        .pt(px(4.0))
        .text_size(theme::SMALL_TEXT_SIZE)
        .text_color(theme::MUTED)
        .child(text)
}

fn folder_row(
    app: &DubplateApp,
    position: usize,
    folder: &Path,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<> {
    let (detail, detail_colour): (SharedString, Rgba) = if app.sources.is_scanning(folder) {
        ("Scanning\u{2026}".into(), theme::MUTED)
    } else if let Some(problem) = app.sources.failure(folder) {
        (problem.to_owned().into(), theme::ALERT)
    } else if let Some(scan) = app.sources.scan(folder) {
        let mut text = format!(
            "{} \u{b7} scanned {}",
            track_count(scan.tracks.len()),
            ago(scan.scanned_at_ms, SystemTime::now())
        );
        if !scan.problems.is_empty() {
            text.push_str(&format!(
                " \u{b7} {} file{} skipped",
                scan.problems.len(),
                if scan.problems.len() == 1 { "" } else { "s" }
            ));
        }
        (text.into(), theme::MUTED)
    } else {
        ("Not scanned yet".into(), theme::MUTED)
    };
    let rescanned = folder.to_path_buf();
    let removed = folder.to_path_buf();
    div()
        .id(("music-folder-settings", position))
        .role(gpui::Role::Group)
        .aria_label(format!("Music folder {}. {detail}", folder.display()))
        .flex()
        .items_center()
        .gap(px(12.0))
        .py(px(8.0))
        .border_t_1()
        .border_color(theme::ROW_LINE)
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap(px(2.0))
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .truncate()
                        .child(folder_name(folder)),
                )
                .child(
                    div()
                        .text_size(theme::SMALL_TEXT_SIZE)
                        .text_color(theme::MUTED)
                        .truncate()
                        .child(abbreviated(folder)),
                )
                .child(
                    div()
                        .text_size(theme::SMALL_TEXT_SIZE)
                        .text_color(detail_colour)
                        .truncate()
                        .child(detail),
                ),
        )
        .child(
            button(("rescan-folder", position), "Rescan", Emphasis::Secondary)
                .aria_label(format!("Rescan {}", folder_name(folder)))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.rescan_music_folder(rescanned.clone(), cx);
                })),
        )
        .child(
            button(("remove-folder", position), "Remove", Emphasis::Secondary)
                .aria_label(format!("Remove {}", folder_name(folder)))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.remove_music_folder(&removed, cx);
                })),
        )
}

fn rekordbox_row(app: &DubplateApp, cx: &mut Context<'_, DubplateApp>) -> impl IntoElement + use<> {
    let (detail, colour, action, import): (SharedString, Rgba, &'static str, bool) = match &app
        .sources
        .rekordbox
    {
        RekordboxSource::Off => (
            "Not imported. Dubplate reads a copy of rekordbox's library and never writes to it."
                .into(),
            theme::MUTED,
            "Import",
            true,
        ),
        RekordboxSource::Reading => (
            "Reading a copy of the library\u{2026}".into(),
            theme::MUTED,
            "Remove",
            false,
        ),
        RekordboxSource::Ready(library) => (
            track_count(library.track_count()).into(),
            theme::MUTED,
            "Remove",
            false,
        ),
        RekordboxSource::Failed(problem) => {
            (problem.clone().into(), theme::ALERT, "Try again", true)
        }
    };
    div()
        .id("rekordbox-settings")
        .role(gpui::Role::Group)
        .aria_label(format!("rekordbox collection. {detail}"))
        .flex()
        .items_center()
        .gap(px(12.0))
        .py(px(8.0))
        .border_t_1()
        .border_color(theme::ROW_LINE)
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap(px(2.0))
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .child("rekordbox collection"),
                )
                .child(
                    div()
                        .text_size(theme::SMALL_TEXT_SIZE)
                        .text_color(colour)
                        .child(detail),
                ),
        )
        .child(
            button("rekordbox-import", action, Emphasis::Secondary)
                .aria_label(format!("{action} rekordbox collection"))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.set_rekordbox_import(import, cx);
                })),
        )
}

/// A sound card: its name, its outputs, and whether Dubplate plays on it. A card with four or
/// more outputs, such as a DDJ-FLX4, carries the headphones on outputs 3 and 4.
fn output_row(
    app: &DubplateApp,
    position: usize,
    device: &OutputDevice,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<> {
    let current = app.current_output() == Some(device.name.as_str());
    let mut detail = format!(
        "{} outputs \u{b7} {:.1} kHz",
        device.channel_count,
        f64::from(device.sample_rate) / 1000.0
    );
    if device.has_headphone_output() {
        detail.push_str(" \u{b7} headphones on 3 and 4");
    }
    if device.is_default {
        detail.push_str(" \u{b7} system default");
    }
    let name = device.name.clone();
    div()
        .id(("sound-card", position))
        .accessible_toggle(format!("{name}. {detail}"), current)
        .flex()
        .items_center()
        .gap(px(10.0))
        .py(px(6.0))
        .px(px(6.0))
        .rounded(theme::SMALL_RADIUS)
        .cursor_pointer()
        .hover(|row| row.bg(theme::RAISED))
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.choose_output(name.clone(), cx);
        }))
        .child(
            div()
                .size(px(8.0))
                .flex_none()
                .rounded_full()
                .border_1()
                .border_color(if current { theme::OK } else { theme::LINE })
                .when(current, |dot| dot.bg(theme::OK)),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .min_w_0()
                .child(
                    div()
                        .truncate()
                        .when(current, |name| name.font_weight(FontWeight::MEDIUM))
                        .child(device.name.clone()),
                )
                .child(
                    div()
                        .text_size(theme::SMALL_TEXT_SIZE)
                        .text_color(theme::MUTED)
                        .child(detail),
                ),
        )
}

/// Whether new tracks start with key lock on, as rekordbox's Master Tempo default.
fn key_lock_row(app: &DubplateApp, cx: &mut Context<'_, DubplateApp>) -> impl IntoElement + use<> {
    let on = app.settings.key_lock_on_new_tracks;
    div()
        .id("key-lock-settings")
        .flex()
        .items_center()
        .gap(px(12.0))
        .py(px(8.0))
        .border_t_1()
        .border_color(theme::ROW_LINE)
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap(px(2.0))
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .child("Key lock on new tracks"),
                )
                .child(
                    div()
                        .text_size(theme::SMALL_TEXT_SIZE)
                        .text_color(theme::MUTED)
                        .child(
                            "Each track you load keeps its pitch when its tempo moves. A deck's \
                             Key lock button still changes it for the track on that deck.",
                        ),
                ),
        )
        .child(
            button(
                "key-lock-default",
                if on { "On" } else { "Off" },
                if on {
                    Emphasis::Primary
                } else {
                    Emphasis::Secondary
                },
            )
            .accessible_toggle("Key lock on new tracks", on)
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.perform(Command::SetKeyLockOnNewTracks(!on), window, cx);
            })),
        )
}

/// Whether a fader closed fast echoes the outgoing deck, the afro house echo out.
fn fader_echo_row(
    app: &DubplateApp,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<> {
    let on = app.settings.fader_echo;
    div()
        .id("fader-echo-settings")
        .flex()
        .items_center()
        .gap(px(12.0))
        .py(px(8.0))
        .border_t_1()
        .border_color(theme::ROW_LINE)
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap(px(2.0))
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .child("Echo out on the faders"),
                )
                .child(
                    div()
                        .text_size(theme::SMALL_TEXT_SIZE)
                        .text_color(theme::MUTED)
                        .child(
                            "Close a channel fader or the crossfader fast and the outgoing deck \
                             echoes on the half beat as it goes, ringing on after the fader is \
                             shut. A fader left part way stops echoing.",
                        ),
                ),
        )
        .child(
            button(
                "fader-echo",
                if on { "On" } else { "Off" },
                if on {
                    Emphasis::Primary
                } else {
                    Emphasis::Secondary
                },
            )
            .accessible_toggle("Echo out on the faders", on)
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.perform(Command::SetFaderEcho(!on), window, cx);
            })),
        )
}

fn track_count(count: usize) -> String {
    format!("{count} track{}", if count == 1 { "" } else { "s" })
}

/// `folder` with the home folder written as `~`.
fn abbreviated(folder: &Path) -> String {
    let home: Option<PathBuf> = dirs::home_dir();
    match home.and_then(|home| folder.strip_prefix(&home).ok().map(Path::to_path_buf)) {
        Some(inside) => format!("~/{}", inside.display()),
        None => folder.display().to_string(),
    }
}

/// How long before `now` the time `milliseconds` after 1970 was, in words.
fn ago(milliseconds: u64, now: SystemTime) -> String {
    let now_ms = now
        .duration_since(UNIX_EPOCH)
        .map(|since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or_default();
    let minutes = now_ms.saturating_sub(milliseconds) / 60_000;
    match minutes {
        0 => "just now".to_owned(),
        1..60 => format!("{minutes} min ago"),
        60..1440 => format!("{} h ago", minutes / 60),
        _ => format!("{} days ago", minutes / 1440),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn scan_times_read_in_words() {
        let now = UNIX_EPOCH + Duration::from_secs(10 * 86_400);
        let at = |seconds_before: u64| {
            u64::try_from(
                (now - Duration::from_secs(seconds_before))
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_millis(),
            )
            .unwrap()
        };
        assert_eq!(ago(at(20), now), "just now");
        assert_eq!(ago(at(5 * 60), now), "5 min ago");
        assert_eq!(ago(at(3 * 3600), now), "3 h ago");
        assert_eq!(ago(at(2 * 86_400), now), "2 days ago");
        assert_eq!(
            ago(at(0) + 5_000, now),
            "just now",
            "a clock that went back"
        );
    }

    #[test]
    fn track_counts_read_in_words() {
        assert_eq!(track_count(1), "1 track");
        assert_eq!(track_count(1204), "1204 tracks");
    }
}
