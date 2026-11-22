//! The Set panel beside the library in Prepare: the set's name, length against its target, tempo
//! range and readiness, its tracks with a seam row between each pair, the backups, and the Next
//! suggestions.

use std::time::Duration;

use dubplate_prep::{SetWarning, Severity, WarningKind, WarningPosition};
use gpui::prelude::*;
use gpui::{
    ClickEvent, Context, FontWeight, MouseButton, PathBuilder, Pixels, SharedString, Window,
    canvas, div, point, px,
};

use super::DubplateApp;
use super::prepare::{NextSuggestions, PrepareFocus, SetRow};
use crate::audition::AUDITION_LENGTH;
use crate::browser::DraggedTrack;
use crate::commands::{Command, Direction, SetEdit};
use crate::energy::{EnergyRange, HIGHEST_ENERGY, LOWEST_ENERGY, set_curve, target_energy};
use crate::fonts::MONO_FAMILY;
use crate::prep::SeamSummary;
use crate::set::running_time;
use crate::shortcuts::{SHORTCUTS, printed_keys};
use crate::theme;
use crate::widgets::{Emphasis, InteractiveControl, button, tooltip};

const PANEL_WIDTH: Pixels = px(420.0);
const ROW_HEIGHT: Pixels = px(40.0);
const CURVE_HEIGHT: Pixels = px(36.0);
const CURVE_STROKE: Pixels = px(1.5);
/// Line segments for the target curve, enough for a smooth line at the panel's width.
const TARGET_SEGMENTS: usize = 60;

/// A set track being dragged to another place in the set.
#[derive(Debug, Clone)]
pub(super) struct DraggedSetTrack {
    position: usize,
    title: SharedString,
}

impl Render for DraggedSetTrack {
    fn render(&mut self, _: &mut Window, _: &mut Context<'_, Self>) -> impl IntoElement {
        div()
            .px(px(10.0))
            .py(px(5.0))
            .rounded(theme::SMALL_RADIUS)
            .border_1()
            .border_color(theme::LINE)
            .bg(theme::RAISED)
            .text_color(theme::TEXT)
            .text_size(theme::SMALL_TEXT_SIZE)
            .font_weight(FontWeight::MEDIUM)
            .child(self.title.clone())
    }
}

pub(super) fn render(app: &DubplateApp, cx: &mut Context<'_, DubplateApp>) -> impl IntoElement {
    let view = &app.prepare_view;
    let rows = &view.rows;
    let seams = &view.seams;
    let warnings = &view.warnings;
    let next = view.next.clone();
    let focused = app.prepare_focus == PrepareFocus::Set;
    let next_cursor = (app.prepare_focus == PrepareFocus::Next)
        .then(|| app.next_cursor.min(next.tracks.len().saturating_sub(1)));
    let mut list = div()
        .id("set-tracks")
        .role(gpui::Role::List)
        .aria_label("Set tracks")
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .track_scroll(&app.set_scroll);
    if rows.is_empty() {
        list = list.child(
            div()
                .px(theme::GUTTER)
                .py(px(16.0))
                .text_color(theme::MUTED)
                .child(
                    "Select tracks in the library and press Enter to add them, or drag them here.",
                ),
        );
    }
    for row in rows {
        let selected = app.set_selection == Some(row.position);
        list = list.child(track_row(row, selected, focused, warnings, rows.len(), cx));
        if let Some(seam) = seams.get(row.position) {
            let audition = app
                .audition
                .as_ref()
                .filter(|audition| audition.after == row.position)
                .map(|audition| audition.started.map(|started| started.elapsed()));
            list = list.child(seam_row(row, seam.as_ref(), warnings, audition, cx));
        }
    }
    div()
        .id("set-panel")
        .flex()
        .flex_col()
        .flex_none()
        .w(PANEL_WIDTH)
        .h_full()
        .border_l_1()
        .border_color(theme::LINE)
        .bg(theme::PANEL)
        .on_drop(cx.listener(|this, dragged: &DraggedTrack, _, cx| {
            let last = this.set_track_count().checked_sub(1);
            this.insert_into_set(dragged.track, last, cx);
        }))
        .child(summary(app, rows, cx))
        .child(list)
        .child(backups(app))
        .child(next_panel(next, next_cursor, cx))
}

fn summary(
    app: &DubplateApp,
    rows: &[SetRow],
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<> {
    let (name, target) = app
        .set_editor
        .as_ref()
        .map(|editor| (editor.set().name.clone(), editor.target()))
        .unwrap_or_default();
    let total: Duration = rows.iter().filter_map(|row| row.duration).sum();
    let bpms: Vec<f32> = rows
        .iter()
        .filter_map(|row| row.track.as_ref()?.bpm)
        .collect();
    let range = match (
        bpms.iter().copied().reduce(f32::min),
        bpms.iter().copied().reduce(f32::max),
    ) {
        (Some(lowest), Some(highest)) => format!("{lowest:.0}\u{2013}{highest:.0} BPM"),
        _ => "No tempo yet".to_owned(),
    };
    let ready = rows.iter().filter(|row| row.ready.is_ready()).count();
    let over = !target.is_zero() && total > target;
    div()
        .flex()
        .flex_col()
        .flex_none()
        .gap(px(4.0))
        .px(theme::GUTTER)
        .py(px(12.0))
        .border_b_1()
        .border_color(theme::LINE)
        .child(
            div()
                .flex()
                .justify_between()
                .items_baseline()
                .child(
                    div()
                        .text_size(px(15.0))
                        .font_weight(FontWeight::MEDIUM)
                        .child(name),
                )
                .child(
                    div()
                        .font_family(MONO_FAMILY)
                        .text_size(theme::SMALL_TEXT_SIZE)
                        .text_color(if over { theme::ATTENTION } else { theme::TEXT })
                        .child(format!(
                            "{} / {}",
                            running_time(total),
                            running_time(target)
                        )),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .text_size(theme::SMALL_TEXT_SIZE)
                .text_color(theme::MUTED)
                .child(div().flex_1().min_w_0().truncate().child(format!(
                    "{} track{} \u{b7} {range} \u{b7} {ready} of {} ready",
                    rows.len(),
                    if rows.len() == 1 { "" } else { "s" },
                    rows.len()
                )))
                .when(!rows.is_empty(), |line| line.child(export_button(cx))),
        )
        .when(!rows.is_empty(), |summary| {
            let range = app.prepare_view.energy_range;
            summary.child(energy_curve(rows, target, range))
        })
}

/// Saves the set as rekordbox XML, as Command-E does.
fn export_button(cx: &mut Context<'_, DubplateApp>) -> impl IntoElement + use<> {
    div()
        .id("export-set")
        .accessible_button("Export to rekordbox")
        .flex_none()
        .px(px(6.0))
        .rounded(theme::SMALL_RADIUS)
        .cursor_pointer()
        .hover(|button| button.bg(theme::RAISED).text_color(theme::TEXT))
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.export_set(cx)))
        .child("Export to rekordbox\u{2026}")
}

/// The set's energy track by track as steps, over the target it should follow, across the
/// target length or the set's when it runs over.
fn energy_curve(rows: &[SetRow], target: Duration, range: EnergyRange) -> impl IntoElement + use<> {
    let tracks: Vec<(Option<Duration>, Option<f32>)> = rows
        .iter()
        .map(|row| {
            (
                row.duration,
                row.track.as_ref().and_then(|track| track.energy),
            )
        })
        .collect();
    let steps = set_curve(&tracks, target);
    let rated = steps.iter().any(|step| step.rating.is_some());
    div()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .pt(px(6.0))
        .child(
            div()
                .flex()
                .justify_between()
                .text_size(theme::TINY_TEXT_SIZE)
                .text_color(theme::MUTED)
                .child("Energy against the target")
                .when(!rated, |caption| caption.child("No ratings yet")),
        )
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, (), window, _| {
                    let inset = f32::from(CURVE_STROKE);
                    let top = f32::from(bounds.origin.y) + inset;
                    let height = f32::from(bounds.size.height) - 2.0 * inset;
                    let left = f32::from(bounds.origin.x);
                    let width = f32::from(bounds.size.width);
                    let at = |fraction: f32, energy: f32| {
                        let level = (energy - LOWEST_ENERGY) / (HIGHEST_ENERGY - LOWEST_ENERGY);
                        point(
                            px(left + width * fraction),
                            px(top + height * (1.0 - level.clamp(0.0, 1.0))),
                        )
                    };
                    let mut target_line = PathBuilder::stroke(px(1.0));
                    for segment in 0..=TARGET_SEGMENTS {
                        let fraction = segment as f32 / TARGET_SEGMENTS as f32;
                        let position = at(fraction, target_energy(fraction, range));
                        if segment == 0 {
                            target_line.move_to(position);
                        } else {
                            target_line.line_to(position);
                        }
                    }
                    if let Ok(path) = target_line.build() {
                        window.paint_path(path, theme::faded(theme::MUTED, 0.5));
                    }
                    if !rated {
                        return;
                    }
                    let mut energy_line = PathBuilder::stroke(CURVE_STROKE);
                    let mut drawing = false;
                    for step in &steps {
                        let Some(rating) = step.rating else {
                            drawing = false;
                            continue;
                        };
                        if drawing {
                            energy_line.line_to(at(step.start, rating));
                        } else {
                            energy_line.move_to(at(step.start, rating));
                        }
                        energy_line.line_to(at(step.end, rating));
                        drawing = true;
                    }
                    if let Ok(path) = energy_line.build() {
                        window.paint_path(path, theme::TEXT);
                    }
                },
            )
            .w_full()
            .h(CURVE_HEIGHT),
        )
}

fn track_row(
    row: &SetRow,
    selected: bool,
    focused: bool,
    warnings: &[SetWarning],
    row_count: usize,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<> {
    let position = row.position;
    let (title, artist, key, bpm, energy) = match &row.track {
        Some(track) => (
            track.title.clone(),
            track.artist.clone(),
            track.key.map(|key| key.camelot()).unwrap_or_default(),
            track.bpm.map(|bpm| format!("{bpm:.1}")).unwrap_or_default(),
            track
                .energy
                .map(|energy| format!("E{energy:.0}"))
                .unwrap_or_default(),
        ),
        None => (
            "Not in the library".to_owned(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        ),
    };
    let duplicate = warnings
        .iter()
        .find(|warning| warning.position == WarningPosition::Track { index: position });
    let readiness = if row.ready.is_ready() {
        "Ready".to_owned()
    } else {
        row.ready.details().replace('\n', ", ")
    };
    let dragged = DraggedSetTrack {
        position,
        title: title.clone().into(),
    };
    div()
        .id(("set-track", position))
        .accessible_button(format!(
            "Track {}: {title} by {artist}. {bpm} BPM. Key {key}. Energy {energy}. {readiness}",
            position + 1
        ))
        .role(gpui::Role::ListItem)
        .aria_selected(selected)
        .aria_position_in_set(position + 1)
        .aria_size_of_set(row_count)
        .on_click(
            cx.listener(move |this, _: &ClickEvent, _, cx| this.select_set_track(position, cx)),
        )
        .flex()
        .items_center()
        .gap(px(10.0))
        .min_h(ROW_HEIGHT)
        .px(theme::GUTTER)
        .cursor_pointer()
        .when(selected, |row| {
            row.bg(theme::SELECTED_ROW)
                .when(focused, |row| row.border_l_2().border_color(theme::TEXT))
        })
        .hover(|row| row.bg(theme::SELECTED_ROW))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, _, cx| this.select_set_track(position, cx)),
        )
        .on_drag(dragged, |dragged, _, _, cx| cx.new(|_| dragged.clone()))
        .drag_over::<DraggedSetTrack>(|style, _, _, _| style.bg(theme::RAISED))
        .drag_over::<DraggedTrack>(|style, _, _, _| style.bg(theme::RAISED))
        .on_drop(cx.listener(move |this, dragged: &DraggedSetTrack, _, cx| {
            this.drop_set_track(dragged.position, position, cx);
            cx.stop_propagation();
        }))
        .on_drop(cx.listener(move |this, dragged: &DraggedTrack, _, cx| {
            this.insert_into_set(dragged.track, Some(position), cx);
            cx.stop_propagation();
        }))
        .child(
            div()
                .w(px(18.0))
                .flex_none()
                .font_family(MONO_FAMILY)
                .text_size(theme::TINY_TEXT_SIZE)
                .text_color(theme::MUTED)
                .child((position + 1).to_string()),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .when(!selected, |text| text.truncate())
                        .font_weight(FontWeight::MEDIUM)
                        .child(title),
                )
                .child(
                    div()
                        .when(!selected, |text| text.truncate())
                        .text_size(theme::SMALL_TEXT_SIZE)
                        .text_color(theme::MUTED)
                        .child(match duplicate {
                            Some(warning) => warning.kind.label(),
                            None => artist,
                        }),
                ),
        )
        .child(
            div()
                .flex()
                .flex_none()
                .flex_col()
                .items_end()
                .gap(px(1.0))
                .child(
                    div()
                        .flex()
                        .gap(px(8.0))
                        .font_family(MONO_FAMILY)
                        .text_size(theme::SMALL_TEXT_SIZE)
                        .child(div().text_color(theme::MUTED).child(energy))
                        .child(key)
                        .child(div().text_color(theme::MUTED).child(bpm)),
                )
                .child(ready_badge(row)),
        )
        .when(selected, |row| {
            row.child(
                div()
                    .flex()
                    .flex_none()
                    .flex_col()
                    .gap(px(2.0))
                    .when(position > 0, |buttons| {
                        buttons.child(
                            button(("set-earlier", position), "↑", Emphasis::Secondary)
                                .aria_label("Move selected track earlier")
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.select_set_track(position, cx);
                                    this.edit_set(SetEdit::Move(Direction::Up), cx);
                                })),
                        )
                    })
                    .when(position + 1 < row_count, |buttons| {
                        buttons.child(
                            button(("set-later", position), "↓", Emphasis::Secondary)
                                .aria_label("Move selected track later")
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.select_set_track(position, cx);
                                    this.edit_set(SetEdit::Move(Direction::Down), cx);
                                })),
                        )
                    }),
            )
        })
}

/// Ready in green, a missing file in red and anything else to fix in amber, always as words,
/// such as "File missing, and 3 more"; hovering it lists every problem. A proposal that can
/// fill a missing cue shows the Y key that accepts it.
fn ready_badge(row: &SetRow) -> impl IntoElement + use<> {
    let colour = match row.ready.problems.first().map(|problem| problem.severity()) {
        None => theme::OK,
        Some(Severity::Blocking) => theme::ALERT,
        Some(Severity::NeedsAttention) => theme::ATTENTION,
    };
    let mut details = row.ready.details();
    if row.has_proposal {
        details.push_str("\n\nY accepts the mix cues proposed from the phrases.");
    }
    div()
        .id(("ready-badge", row.position))
        .flex()
        .items_center()
        .gap(px(5.0))
        .text_size(theme::TINY_TEXT_SIZE)
        .text_color(colour)
        .when(!details.is_empty(), |badge| badge.tooltip(tooltip(details)))
        .child(row.ready.badge())
        .when(row.has_proposal, |badge| {
            badge.child(
                div()
                    .px(px(4.0))
                    .rounded(px(3.0))
                    .border_1()
                    .border_color(theme::LINE)
                    .font_family(MONO_FAMILY)
                    .text_color(theme::TEXT)
                    .child("Y"),
            )
        })
}

/// The thin row between two set tracks: the key move and tempo step, the warnings, and the
/// button that plays the seam in the headphones. `audition` is set while this seam is heard:
/// `None` while its tracks open, then how long it has played.
fn seam_row(
    row: &SetRow,
    seam: Option<&SeamSummary>,
    warnings: &[SetWarning],
    audition: Option<Option<Duration>>,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<> {
    let position = row.position;
    let free = row.free_into_next;
    let shown: Vec<&SetWarning> = warnings
        .iter()
        .filter(|warning| {
            !free
                && warning.position
                    == WarningPosition::Seam {
                        after: row.position,
                    }
        })
        .collect();
    let key_warned = shown
        .iter()
        .any(|warning| matches!(warning.kind, WarningKind::KeyClash { .. }));
    let tempo_warned = shown
        .iter()
        .any(|warning| matches!(warning.kind, WarningKind::TempoGap { .. }));
    let seam_warnings: Vec<String> = shown.iter().map(|warning| warning.kind.label()).collect();
    let line = seam
        .map(|seam| seam.line(key_warned, tempo_warned))
        .unwrap_or_default();
    div()
        .flex()
        .items_center()
        .gap(px(8.0))
        .h(px(20.0))
        .pl(px(42.0))
        .pr(theme::GUTTER)
        .text_size(theme::TINY_TEXT_SIZE)
        .text_color(theme::MUTED)
        .child(div().w(px(10.0)).h(px(1.0)).bg(theme::LINE))
        .child(div().truncate().child(line))
        .child(div().flex_1())
        .map(|seam_row| {
            if free {
                seam_row.child(div().text_color(theme::TEXT).child("Free"))
            } else if seam_warnings.is_empty() {
                seam_row
            } else {
                seam_row.child(
                    div()
                        .truncate()
                        .text_color(theme::ATTENTION)
                        .child(seam_warnings.join(" \u{b7} ")),
                )
            }
        })
        .child(
            div()
                .id(("seam-audition", position))
                .accessible_toggle(
                    format!("Listen to transition after track {}", position + 1),
                    audition.is_some(),
                )
                .flex_none()
                .px(px(6.0))
                .rounded(theme::SMALL_RADIUS)
                .cursor_pointer()
                .font_family(MONO_FAMILY)
                .text_color(if audition.is_some() {
                    theme::TEXT
                } else {
                    theme::MUTED
                })
                .when(audition.is_some(), |button| button.bg(theme::RAISED))
                .hover(|button| button.bg(theme::RAISED).text_color(theme::TEXT))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.toggle_audition_of(position, cx);
                }))
                .child(match audition {
                    None => "\u{25b6} Listen".to_owned(),
                    Some(None) => "Opening\u{2026}".to_owned(),
                    Some(Some(elapsed)) => format!(
                        "\u{25a0} {}/{}",
                        running_time(elapsed),
                        running_time(AUDITION_LENGTH)
                    ),
                }),
        )
}

fn backups(app: &DubplateApp) -> impl IntoElement {
    let titles = app.prepare_view.backups.clone();
    div().when(!titles.is_empty(), |section| {
        section
            .flex()
            .flex_col()
            .flex_none()
            .gap(px(2.0))
            .px(theme::GUTTER)
            .py(px(8.0))
            .border_t_1()
            .border_color(theme::LINE)
            .child(
                div()
                    .text_size(theme::TINY_TEXT_SIZE)
                    .text_color(theme::MUTED)
                    .child("Backups"),
            )
            .children(
                titles
                    .into_iter()
                    .map(|title| div().text_size(theme::SMALL_TEXT_SIZE).child(title)),
            )
    })
}

/// The suggestions after the selected set track. `cursor` is the one Enter inserts, while the
/// keys are on the panel.
fn next_panel(
    next: NextSuggestions,
    cursor: Option<usize>,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<> {
    let NextSuggestions {
        pending,
        after,
        direction,
        tracks: next,
    } = next;
    let heading = match (&after, direction) {
        (Some(title), Some(direction)) => {
            format!("Next after {title} \u{b7} {}", direction.label())
        }
        (Some(title), None) => format!("Next after {title}"),
        (None, _) => "Next".to_owned(),
    };
    let mut panel = div()
        .id("next-suggestions")
        .role(gpui::Role::Group)
        .aria_label("Next track suggestions")
        .max_h(px(220.0))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .flex_none()
        .gap(px(2.0))
        .px(theme::GUTTER)
        .py(px(10.0))
        .border_t_1()
        .border_color(theme::LINE)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .pb(px(4.0))
                .text_size(theme::TINY_TEXT_SIZE)
                .text_color(theme::MUTED)
                .child(div().flex_1().min_w_0().truncate().child(heading))
                .when(cursor.is_some() && !next.is_empty(), |heading| {
                    heading.child(
                        div()
                            .flex_none()
                            .text_color(theme::TEXT)
                            .child("\u{2191}\u{2193} choose \u{b7} \u{21a9} insert"),
                    )
                }),
        );
    if pending && next.is_empty() {
        panel = panel.child(
            div()
                .text_size(theme::SMALL_TEXT_SIZE)
                .text_color(theme::MUTED)
                .child("Finding matching tracks…"),
        );
    } else if after.is_none() {
        panel = panel.child(
            div()
                .text_size(theme::SMALL_TEXT_SIZE)
                .text_color(theme::MUTED)
                .child("Suggestions follow the selected set track."),
        );
    } else if next.is_empty() {
        panel = panel.child(
            div()
                .text_size(theme::SMALL_TEXT_SIZE)
                .text_color(theme::MUTED)
                .child("Nothing in the library mixes well after it."),
        );
    }
    for (rank, suggestion) in next.into_iter().enumerate() {
        let path = suggestion.path.clone();
        let highlighted = cursor == Some(rank);
        panel = panel.child(
            div()
                .id(("next-track", rank))
                .relative()
                .accessible_button(format!(
                    "Add {} by {}. {}",
                    suggestion.title, suggestion.artist, suggestion.reason
                ))
                .when(rank < 5, |button| {
                    button.aria_keyshortcuts(format!("Alt+{}", rank + 1))
                })
                .aria_description(if highlighted {
                    "Selected next suggestion. Enter inserts it."
                } else {
                    "Enter inserts this suggestion."
                })
                .flex()
                .flex_col()
                .px(px(6.0))
                .py(px(4.0))
                .rounded(theme::SMALL_RADIUS)
                .cursor_pointer()
                .when(highlighted, |row| row.bg(theme::RAISED))
                .hover(|row| row.bg(theme::RAISED))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.insert_suggestion(&path, cx);
                }))
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap(px(6.0))
                        .min_w_0()
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .child(suggestion.title),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_color(theme::MUTED)
                                .child(suggestion.artist),
                        )
                        .child(
                            div()
                                .flex_none()
                                .font_family(MONO_FAMILY)
                                .text_size(theme::TINY_TEXT_SIZE)
                                .text_color(if highlighted {
                                    theme::TEXT
                                } else {
                                    theme::MUTED
                                })
                                .children(insert_key(rank)),
                        ),
                )
                .child(
                    div()
                        .text_size(theme::TINY_TEXT_SIZE)
                        .text_color(theme::MUTED)
                        .child(suggestion.reason),
                ),
        );
    }
    panel
}

/// The key that inserts the suggestion at `rank`, as the help prints it: `⌥1` for the first.
fn insert_key(rank: usize) -> Option<String> {
    let command = Command::EditSet(SetEdit::InsertSuggestion(rank));
    SHORTCUTS
        .iter()
        .find(|shortcut| shortcut.command == command)
        .map(|shortcut| printed_keys(shortcut.keystroke).replace(' ', ""))
}
