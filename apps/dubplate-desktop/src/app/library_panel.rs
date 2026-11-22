//! The settings, over the window: the music folders with their tracks and last scan,
//! rekordbox's collection as a separate choice, and the sound card Dubplate plays on.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use gpui::base::FocusTrapElement;
use gpui::prelude::*;
use gpui::{
    Bounds, ClickEvent, Context, FontWeight, MouseButton, Pixels, Rgba, SharedString, Window,
    canvas, div, fill, point, px, size,
};

use super::DubplateApp;
use super::faders::{channel_fader_curve_label, crossfader_curve_label};
use crate::collection::folder_name;
use crate::commands::{AutoCueChoice, Command, PadFxEdit};
use crate::library::RekordboxSource;
use crate::pad_fx::PadFxLayer;
use crate::theme;
use crate::widgets::InteractiveControl;
use crate::widgets::{Emphasis, button};
use dubplate_engine::{ChannelFaderCurve, CrossfaderCurve, OutputDevice};

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
                .child(section_title("Sound card"))
                .children(output_rows)
                .child(section_title("Decks"))
                .child(setting_switch(
                    "auto-cue-saved",
                    "Cue at the first memory or hot cue",
                    "A track with saved cues loads at the earliest one.",
                    app.settings.auto_cue.saved_cues_first,
                    Command::ToggleAutoCue(AutoCueChoice::SavedCuesFirst),
                    cx,
                ))
                .child(setting_switch(
                    "auto-cue-sound",
                    "Cue at the first sound",
                    "Otherwise a track loads at its first sound, on the beat it starts with, not at 0:00.",
                    app.settings.auto_cue.first_sound,
                    Command::ToggleAutoCue(AutoCueChoice::FirstSound),
                    cx,
                ))
                .child(section_title("Mixer"))
                .child(crossfader_row(app.crossfader().enabled, cx))
                .child(curve_row(
                    "Crossfader curve",
                    CrossfaderCurve::ALL.map(|curve| CurveChoice {
                        name: crossfader_curve_label(curve),
                        chosen: curve == app.crossfader().curve,
                        picture: CurvePicture::Crossfader(curve),
                        command: Command::SetCrossfaderCurve(curve),
                    }),
                    cx,
                ))
                .child(curve_row(
                    "Channel fader curve",
                    ChannelFaderCurve::ALL.map(|curve| CurveChoice {
                        name: channel_fader_curve_label(curve),
                        chosen: curve == app.channel_fader_curve(),
                        picture: CurvePicture::ChannelFader(curve),
                        command: Command::SetChannelFaderCurve(curve),
                    }),
                    cx,
                ))
                .child(
                    div()
                        .text_size(theme::SMALL_TEXT_SIZE)
                        .text_color(theme::MUTED)
                        .child(
                            "Each channel's A, THRU, B switch is under its fader. THRU takes the channel off the crossfader.",
                        ),
                )
                .child(section_title("Pad FX"))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(div().text_size(theme::SMALL_TEXT_SIZE).child(format!(
                            "{}{} \u{b7} 32 pads",
                            app.pad_fx.preset().label(),
                            if app.pad_fx.edited() { ", edited" } else { "" }
                        )))
                        .child(
                            button("edit-pad-fx", "Edit Pad FX\u{2026}", Emphasis::Secondary)
                                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                    this.perform(
                                        Command::PadFx(PadFxEdit::Toggle(PadFxLayer::Fx1)),
                                        window,
                                        cx,
                                    );
                                })),
                        ),
                )
                .focus_trap("settings-focus-trap", &app.modal_focus),
        )
}

/// A setting that is on or off: its name and what it does, and a switch at the end of the row.
fn setting_switch(
    id: &'static str,
    name: &'static str,
    detail: &'static str,
    on: bool,
    command: Command,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<> {
    div()
        .id(id)
        .accessible_toggle(format!("{name}. {detail}"), on)
        .flex()
        .items_center()
        .gap(px(12.0))
        .py(px(8.0))
        .border_t_1()
        .border_color(theme::ROW_LINE)
        .cursor_pointer()
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            this.perform(command, window, cx);
        }))
/// Whether the crossfader is in use, with what Off means. rekordbox has no single switch for
/// it; DJs set both channels' crossfader assign to THRU, which Off does at once.
fn crossfader_row(on: bool, cx: &mut Context<'_, DubplateApp>) -> impl IntoElement + use<> {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .py(px(6.0))
        .border_t_1()
        .border_color(theme::ROW_LINE)
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap(px(2.0))
                .child(div().child(name))
                .child(
                    div()
                        .text_size(theme::SMALL_TEXT_SIZE)
                        .text_color(theme::MUTED)
                        .child(detail),
                .gap(px(2.0))
                .child(div().text_size(theme::SMALL_TEXT_SIZE).child("Crossfader"))
                .child(
                    div()
                        .text_size(theme::TINY_TEXT_SIZE)
                        .text_color(theme::MUTED)
                        .child("Off: both channels play as on THRU, so the crossfader moves without changing the sound."),
                ),
        )
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .w(px(30.0))
                .h(px(18.0))
                .p(px(2.0))
                .rounded_full()
                .bg(if on { theme::OK } else { theme::TRACK })
                .when(on, |track| track.justify_end())
                .child(div().size(px(14.0)).rounded_full().bg(theme::THUMB)),
        )
}

                .id("crossfader-switch")
                .role(gpui::Role::RadioGroup)
                .aria_label("Crossfader")
                .flex()
                .flex_none()
                .gap(px(4.0))
                .children([(true, "On"), (false, "Off")].map(|(choice, name)| {
                    let chosen = choice == on;
                    div()
                        .id(SharedString::from(format!("crossfader-{name}")))
                        .accessible_toggle(format!("Crossfader {name}"), chosen)
                        .flex()
                        .justify_center()
                        .w(px(44.0))
                        .py(px(4.0))
                        .rounded(theme::SMALL_RADIUS)
                        .border_1()
                        .text_size(theme::SMALL_TEXT_SIZE)
                        .cursor_pointer()
                        .map(|option| {
                            if chosen {
                                option.border_color(theme::LINE).bg(theme::RAISED)
                            } else {
                                option
                                    .border_color(gpui::transparent_black())
                                    .text_color(theme::MUTED)
                                    .hover(|option| option.bg(theme::RAISED))
                            }
                        })
                        .when(!chosen, |option| {
                            option.on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.perform(Command::ToggleCrossfader, window, cx);
                            }))
                        })
                        .child(name)
                })),
        )
}

/// The points a curve picture is drawn from, across its width.
const CURVE_SAMPLES: usize = 28;
const CURVE_PICTURE_WIDTH: Pixels = px(30.0);
const CURVE_PICTURE_HEIGHT: Pixels = px(14.0);
const CURVE_DOT: Pixels = px(1.5);

/// What a curve option draws.
#[derive(Clone, Copy)]
enum CurvePicture {
    /// Both sides of the crossfader, A falling and B rising.
    Crossfader(CrossfaderCurve),
    /// A channel fader's level rising with its position.
    ChannelFader(ChannelFaderCurve),
}

impl CurvePicture {
    /// The levels drawn at `position`, each from 0.0 to 1.0.
    fn levels(self, position: f32) -> [Option<f32>; 2] {
        match self {
            Self::Crossfader(curve) => {
                let (left, right) = curve.gains(position);
                [Some(left), Some(right)]
            }
            Self::ChannelFader(curve) => [Some(curve.gain(position)), None],
        }
    }
}

/// One option of a curve setting.
struct CurveChoice {
    name: &'static str,
    chosen: bool,
    picture: CurvePicture,
    command: Command,
}

/// A curve setting: its name, then its options side by side, each a small drawing of the curve
/// above its name, the chosen one raised.
fn curve_row(
    title: &'static str,
    choices: [CurveChoice; 3],
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<> {
    div()
        .id(title)
        .role(gpui::Role::RadioGroup)
        .aria_label(title)
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .py(px(6.0))
        .border_t_1()
        .border_color(theme::ROW_LINE)
        .child(div().text_size(theme::SMALL_TEXT_SIZE).child(title))
        .child(
            div()
                .flex()
                .gap(px(4.0))
                .children(choices.into_iter().map(|choice| {
                    let command = choice.command;
                    let picture = choice.picture;
                    let colour = if choice.chosen {
                        theme::TEXT
                    } else {
                        theme::MUTED
                    };
                    div()
                        .id(SharedString::from(format!("curve-{title}-{}", choice.name)))
                        .accessible_toggle(format!("{title}: {}", choice.name), choice.chosen)
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(3.0))
                        .w(px(64.0))
                        .py(px(5.0))
                        .rounded(theme::SMALL_RADIUS)
                        .border_1()
                        .cursor_pointer()
                        .map(|option| {
                            if choice.chosen {
                                option.border_color(theme::LINE).bg(theme::RAISED)
                            } else {
                                option
                                    .border_color(gpui::transparent_black())
                                    .hover(|option| option.bg(theme::RAISED))
                            }
                        })
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.perform(command, window, cx);
                        }))
                        .child(
                            canvas(
                                |_, _, _| {},
                                move |bounds, (), window, _| {
                                    paint_curve(bounds, picture, colour, window);
                                },
                            )
                            .w(CURVE_PICTURE_WIDTH)
                            .h(CURVE_PICTURE_HEIGHT),
                        )
                        .child(
                            div()
                                .text_size(theme::TINY_TEXT_SIZE)
                                .text_color(colour)
                                .child(choice.name),
                        )
                })),
        )
}

/// Draws a curve as a row of small dots, level up, position across.
fn paint_curve(bounds: Bounds<Pixels>, picture: CurvePicture, colour: Rgba, window: &mut Window) {
    let travel_x = bounds.size.width - CURVE_DOT;
    let travel_y = bounds.size.height - CURVE_DOT;
    for sample in 0..CURVE_SAMPLES {
        let position = sample as f32 / (CURVE_SAMPLES - 1) as f32;
        for level in picture.levels(position).into_iter().flatten() {
            let origin = bounds.origin + point(travel_x * position, travel_y * (1.0 - level));
            window.paint_quad(fill(
                Bounds::new(origin, size(CURVE_DOT, CURVE_DOT)),
                colour,
            ));
        }
    }
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
