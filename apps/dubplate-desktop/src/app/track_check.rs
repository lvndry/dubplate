//! The track check above the library in Prepare: the selected set track's waveform with the
//! DJ's grid over it and its phrases, to confirm the grid, nudge it, move its downbeat and put
//! the mix cues on bars. Everything changed here goes to the set store, never to rekordbox.

use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use dubplate_prep::{GridCorrection, SetEntry};
use gpui::prelude::*;
use gpui::{
    ClickEvent, Context, FontWeight, MouseButton, MouseDownEvent, Pixels, SharedString, canvas,
    div, fill, point, px, size,
};

use super::{DubplateApp, HitAreas, lanes};
use crate::collection::clock;
use crate::commands::{Command, SetEdit};
use crate::controls::{Orientation, fraction_at};
use crate::fonts::MONO_FAMILY;
use crate::prep::{TrackDetail, track_detail};
use crate::shortcuts::GRID_NUDGE_MS;
use crate::theme;
use crate::waveform::BeatMarks;
use crate::widgets::{InteractiveControl, adjusted_value};

const LANE_HEIGHT: Pixels = px(64.0);
const OVERVIEW_HEIGHT: Pixels = px(26.0);
const CONTROLS_HEIGHT: Pixels = px(26.0);
const ROW_GAP: Pixels = px(6.0);
const OVERVIEW_BAR_SPACING: f32 = 3.0;
const OVERVIEW_BAR_WIDTH: f32 = 2.0;
/// The share of the overview a phrase name needs before the next one is written.
const LABEL_ROOM: f32 = 0.05;
/// Bars in a phrase, how often the overview draws a stronger bar line.
const BARS_PER_PHRASE: usize = 8;
/// Room for the grid button in both its states, so the controls beside it stay put when it
/// changes.
const CONFIRM_WIDTH: Pixels = px(136.0);
/// Room for a mix cue time up to `59:59`, so setting a cue never moves the controls.
const CUE_TIME_WIDTH: Pixels = px(40.0);

impl DubplateApp {
    /// The selected set track's entry and what its analysis holds, once read.
    pub(super) fn selected_detail(&self) -> Option<(SetEntry, Arc<TrackDetail>)> {
        let entry = self
            .set_editor
            .as_ref()?
            .tracks()
            .get(self.set_selection?)
            .map(|entry| (*entry).clone())?;
        let (path, detail) = self.track_detail.as_ref()?;
        (path == &entry.track_path).then(|| (entry, Arc::clone(detail)))
    }

    /// Where the track check stands in the track: the playhead of a deck playing it, or where
    /// the DJ last clicked on its overview.
    pub(super) fn check_playhead(&self, path: &Path) -> f64 {
        dubplate_engine::DeckId::ALL
            .into_iter()
            .find(|deck| {
                self.deck(*deck)
                    .track
                    .as_ref()
                    .and_then(|track| track.track.file_path.as_deref())
                    == Some(path)
            })
            .map_or(self.check_cursor_seconds, |deck| {
                self.deck_state(deck).position_seconds
            })
    }

    /// Reads the selected set track's analysis when it is not the one already read.
    pub(super) fn ensure_track_detail(&mut self, cx: &mut Context<'_, Self>) {
        let Some(path) =
            self.set_editor
                .as_ref()
                .zip(self.set_selection)
                .and_then(|(editor, position)| {
                    editor
                        .tracks()
                        .get(position)
                        .map(|entry| entry.track_path.clone())
                })
        else {
            return;
        };
        let read = self
            .track_detail
            .as_ref()
            .is_some_and(|(read_path, _)| read_path == &path);
        if read || self.detail_pending.as_ref() == Some(&path) {
            return;
        }
        let Some(collection) = self.collection().cloned() else {
            return;
        };
        let Some(index) = collection.find_path(&path) else {
            return;
        };
        let analysis = self.analyses.get(&path).cloned();
        self.detail_pending = Some(path.clone());
        self.check_cursor_seconds = 0.0;
        cx.spawn(async move |this, cx| {
            let detail = cx
                .background_executor()
                .spawn(async move {
                    track_detail(
                        collection.track(index),
                        collection.share_directory(),
                        analysis.as_deref(),
                    )
                })
                .await;
            this.update(cx, |this, cx| {
                if this.detail_pending.as_ref() == Some(&path) {
                    this.detail_pending = None;
                    this.track_detail = Some((path, Arc::new(detail)));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Moves the track check's playhead to where the DJ clicked on the overview.
    fn click_check_overview(&mut self, fraction: f32, duration: f64, cx: &mut Context<'_, Self>) {
        self.check_cursor_seconds = duration * f64::from(fraction);
        cx.notify();
    }
}

pub(super) fn render(app: &DubplateApp, cx: &mut Context<'_, DubplateApp>) -> impl IntoElement {
    let strip = div()
        .flex()
        .flex_col()
        .flex_none()
        .gap(ROW_GAP)
        .px(theme::GUTTER)
        .py(px(10.0))
        .border_b_1()
        .border_color(theme::LINE)
        .bg(theme::PANEL);
    let Some((entry, detail)) = app.selected_detail() else {
        let hint = if app.set_selection.is_some() {
            "Reading the track\u{2026}"
        } else {
            "Select a set track to check its grid and set its mix cues."
        };
        // The same height as the check itself, so the set list below never jumps while a
        // track is read.
        let content_height = CONTROLS_HEIGHT + ROW_GAP * 2.0 + LANE_HEIGHT + OVERVIEW_HEIGHT;
        return strip.child(
            div()
                .h(content_height)
                .flex()
                .items_center()
                .text_color(theme::MUTED)
                .child(hint),
        );
    };
    let correction = GridCorrection::of(&entry);
    let grid = detail.grid.as_ref().map(|grid| correction.apply(grid));
    let marks = Arc::new(grid.as_ref().map(BeatMarks::from_grid).unwrap_or_default());
    let playhead = app.check_playhead(&entry.track_path);
    let duration = app
        .collection()
        .and_then(|collection| {
            collection
                .find_path(&entry.track_path)
                .map(|index| collection.track(index))
        })
        .and_then(|track| track.track.length_seconds)
        .map_or(0.0, f64::from);
    strip
        .child(controls(app, &entry, correction, cx))
        .child(lane(&detail, &marks, playhead, &entry))
        .child(overview(
            app,
            &detail,
            grid.as_ref(),
            &entry,
            playhead,
            duration,
            cx,
        ))
}

fn controls(
    app: &DubplateApp,
    entry: &SetEntry,
    correction: GridCorrection,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<> {
    let title = app
        .collection()
        .and_then(|collection| {
            collection
                .find_path(&entry.track_path)
                .map(|index| collection.track(index))
        })
        .map(|track| format!("{} \u{b7} {}", track.row.title, track.row.artist))
        .unwrap_or_default();
    let cue = |milliseconds: Option<u32>| {
        milliseconds.map_or_else(
            || "--:--".to_owned(),
            |milliseconds| clock(f64::from(milliseconds) / 1000.0),
        )
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.0))
        .h(CONTROLS_HEIGHT)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .font_weight(FontWeight::MEDIUM)
                .child(title),
        )
        .child(small_label("Grid"))
        .child(key_button(
            "grid-earlier",
            "\u{2039}",
            SetEdit::NudgeGrid(-GRID_NUDGE_MS),
            cx,
        ))
        .child(
            div()
                .w(px(54.0))
                .flex()
                .justify_center()
                .font_family(MONO_FAMILY)
                .text_size(theme::SMALL_TEXT_SIZE)
                .child(format!("{:+} ms", correction.offset_ms)),
        )
        .child(key_button(
            "grid-later",
            "\u{203a}",
            SetEdit::NudgeGrid(GRID_NUDGE_MS),
            cx,
        ))
        .child(small_label("Bar"))
        .child(key_button(
            "downbeat-earlier",
            "\u{2039}",
            SetEdit::ShiftDownbeat(-1),
            cx,
        ))
        .child(
            div()
                .w(px(28.0))
                .flex()
                .justify_center()
                .font_family(MONO_FAMILY)
                .text_size(theme::SMALL_TEXT_SIZE)
                .child(format!("+{}", correction.downbeat_shift)),
        )
        .child(key_button(
            "downbeat-later",
            "\u{203a}",
            SetEdit::ShiftDownbeat(1),
            cx,
        ))
        .child(
            div()
                .id("confirm-grid")
                .accessible_button("Confirm beat grid")
                .flex()
                .items_center()
                .justify_center()
                .w(CONFIRM_WIDTH)
                .h(px(24.0))
                .px(px(8.0))
                .rounded(px(5.0))
                .border_1()
                .text_size(theme::SMALL_TEXT_SIZE)
                .cursor_pointer()
                .map(|button| {
                    if entry.grid_confirmed {
                        button.border_color(theme::OK).text_color(theme::OK)
                    } else {
                        button
                            .border_color(theme::ATTENTION)
                            .text_color(theme::ATTENTION)
                    }
                })
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.perform(Command::EditSet(SetEdit::ConfirmGrid), window, cx);
                }))
                .child(if entry.grid_confirmed {
                    "Grid confirmed \u{b7} G"
                } else {
                    "Confirm grid \u{b7} G"
                }),
        )
        .child(key_button("mix-in", "In", SetEdit::MixInAtPlayhead, cx))
        .child(
            div()
                .w(CUE_TIME_WIDTH)
                .flex()
                .justify_end()
                .font_family(MONO_FAMILY)
                .text_size(theme::SMALL_TEXT_SIZE)
                .child(cue(entry.mix_in_ms)),
        )
        .child(key_button("mix-out", "Out", SetEdit::MixOutAtPlayhead, cx))
        .child(
            div()
                .w(CUE_TIME_WIDTH)
                .flex()
                .justify_end()
                .font_family(MONO_FAMILY)
                .text_size(theme::SMALL_TEXT_SIZE)
                .child(cue(entry.mix_out_ms)),
        )
}

fn small_label(text: &'static str) -> impl IntoElement {
    div()
        .text_size(theme::TINY_TEXT_SIZE)
        .text_color(theme::MUTED)
        .child(text)
}

fn key_button(
    id: &'static str,
    label: &'static str,
    edit: SetEdit,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<> {
    div()
        .id(id)
        .accessible_button(label)
        .flex()
        .items_center()
        .justify_center()
        .h(px(24.0))
        .min_w(px(24.0))
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
            this.perform(Command::EditSet(edit), window, cx);
        }))
        .child(label)
}

/// The waveform around the playhead with the corrected grid, to judge the grid by eye, and the
/// mix cues as flags.
fn lane(
    detail: &TrackDetail,
    marks: &Arc<BeatMarks>,
    playhead: f64,
    entry: &SetEntry,
) -> impl IntoElement + use<> {
    let zoomed = detail.zoomed.clone();
    let marks = Arc::clone(marks);
    let cues: Vec<(&'static str, f64)> = [("IN", entry.mix_in_ms), ("OUT", entry.mix_out_ms)]
        .into_iter()
        .filter_map(|(label, milliseconds)| Some((label, f64::from(milliseconds?) / 1000.0)))
        .collect();
    div()
        .relative()
        .h(LANE_HEIGHT)
        .rounded(px(4.0))
        .overflow_hidden()
        .bg(theme::BACKGROUND)
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, (), window, _| {
                    lanes::paint_lane(
                        bounds,
                        zoomed.as_deref(),
                        Some(&marks),
                        playhead,
                        1.0,
                        None,
                        window,
                    );
                    let zoom = zoomed
                        .as_deref()
                        .map_or(crate::waveform::Zoom::DEFAULT, |waveform| waveform.zoom());
                    let centre = f32::from(bounds.size.width) / 2.0;
                    for (_, seconds) in &cues {
                        let x = centre + ((seconds - playhead) * zoom.pixels_per_second()) as f32;
                        if (0.0..f32::from(bounds.size.width)).contains(&x) {
                            let line = gpui::Bounds::new(
                                bounds.origin + point(px(x - 1.0), px(0.0)),
                                size(px(2.0), bounds.size.height),
                            );
                            window.paint_quad(fill(line, theme::ATTENTION));
                        }
                    }
                    let head = gpui::Bounds::new(
                        bounds.origin + point(px(centre - 1.0), px(0.0)),
                        size(px(2.0), bounds.size.height),
                    );
                    window.paint_quad(fill(head, theme::PLAYHEAD));
                },
            )
            .absolute()
            .size_full(),
        )
}

/// The whole track: its waveform, a line on every bar and a stronger one every phrase of eight
/// bars, rekordbox's phrases named along the top, the mix cues and the playhead. A click moves
/// the playhead.
fn overview(
    app: &DubplateApp,
    detail: &TrackDetail,
    grid: Option<&dubplate_rekordbox::analysis::BeatGrid>,
    entry: &SetEntry,
    playhead: f64,
    duration: f64,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<> {
    let bars: Vec<f32> = grid
        .map(|grid| {
            grid.beats
                .iter()
                .filter(|beat| beat.beat_in_bar == 1)
                .map(|beat| beat.time_ms as f32 / 1000.0)
                .collect()
        })
        .unwrap_or_default();
    let phrases: Vec<(f32, SharedString)> = match (&detail.structure, detail.grid.as_ref()) {
        (Some(structure), Some(raw_grid)) => structure
            .phrases
            .iter()
            .filter_map(|phrase| {
                let beat = raw_grid
                    .beats
                    .get(usize::from(phrase.start_beat).checked_sub(1)?)?;
                let seconds =
                    (i64::from(beat.time_ms) + i64::from(entry.grid_offset_ms)) as f32 / 1000.0;
                Some((seconds, phrase.kind.to_string().into()))
            })
            .collect(),
        _ => Vec::new(),
    };
    let mut last_label: Option<f32> = None;
    let labels: Vec<(f32, SharedString)> = phrases
        .iter()
        .filter(|(seconds, _)| {
            let room = last_label.is_none_or(|last| seconds - last >= duration as f32 * LABEL_ROOM);
            if room {
                last_label = Some(*seconds);
            }
            room
        })
        .cloned()
        .collect();
    let cues: Vec<f32> = [entry.mix_in_ms, entry.mix_out_ms]
        .into_iter()
        .flatten()
        .map(|milliseconds| milliseconds as f32 / 1000.0)
        .collect();
    let overview = detail.overview.clone();
    let hit_areas: Rc<HitAreas> = Rc::clone(&app.hit_areas);
    let duration_f32 = duration.max(1.0) as f32;
    let playhead_f32 = playhead as f32;
    div()
        .id("check-overview")
        .accessible_slider(
            "Track check position",
            (playhead / duration.max(1.0)) as f32,
        )
        .aria_orientation(gpui::Orientation::Horizontal)
        .map(|element| {
            crate::widgets::numeric_actions(element, cx, move |this, change, cx| {
                let fraction = (this.check_cursor_seconds / duration.max(1.0)) as f32;
                this.click_check_overview(change.apply(fraction), duration, cx);
            })
        })
        .on_key_down(cx.listener(move |this, event, _, cx| {
            let current = (playhead / duration.max(1.0)) as f32;
            if let Some(fraction) = adjusted_value(event, current, 0.0) {
                this.click_check_overview(fraction, duration, cx);
                cx.stop_propagation();
            }
        }))
        .relative()
        .h(OVERVIEW_HEIGHT)
        .rounded(px(4.0))
        .bg(theme::RAISED)
        .cursor_pointer()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                if let Some(bounds) = this.hit_areas.check_overview.get() {
                    let fraction = fraction_at(event.position, bounds, Orientation::Horizontal);
                    this.click_check_overview(fraction, duration, cx);
                }
                cx.stop_propagation();
            }),
        )
        .child(
            canvas(
                move |bounds, _, _| hit_areas.check_overview.set(Some(bounds)),
                move |bounds, (), window, _| {
                    let width = f32::from(bounds.size.width);
                    let height = f32::from(bounds.size.height);
                    let x_of = |seconds: f32| seconds / duration_f32 * width;
                    if let Some(overview) = &overview {
                        let count = (width / OVERVIEW_BAR_SPACING).floor().max(0.0) as usize;
                        for (index, level) in overview.bars(count).iter().enumerate() {
                            let left = index as f32 * OVERVIEW_BAR_SPACING;
                            let bar_height = (level * (height - 6.0)).max(1.0);
                            let bar = gpui::Bounds::new(
                                bounds.origin + point(px(left), px((height - bar_height) / 2.0)),
                                size(px(OVERVIEW_BAR_WIDTH), px(bar_height)),
                            );
                            window.paint_quad(fill(bar, theme::faded(theme::MUTED, 0.6)));
                        }
                    }
                    for (index, seconds) in bars.iter().enumerate() {
                        let colour = if index % BARS_PER_PHRASE == 0 {
                            theme::PHRASE_LINE
                        } else {
                            theme::BAR_LINE
                        };
                        let line = gpui::Bounds::new(
                            bounds.origin + point(px(x_of(*seconds)), px(0.0)),
                            size(px(1.0), bounds.size.height),
                        );
                        window.paint_quad(fill(line, colour));
                    }
                    for (seconds, _) in &phrases {
                        let tick = gpui::Bounds::new(
                            bounds.origin + point(px(x_of(*seconds)), px(0.0)),
                            size(px(2.0), px(6.0)),
                        );
                        window.paint_quad(fill(tick, theme::TEXT));
                    }
                    for seconds in &cues {
                        let mark = gpui::Bounds::new(
                            bounds.origin + point(px(x_of(*seconds) - 1.0), px(0.0)),
                            size(px(2.0), bounds.size.height),
                        );
                        window.paint_quad(fill(mark, theme::ATTENTION));
                    }
                    let head = gpui::Bounds::new(
                        bounds.origin + point(px(x_of(playhead_f32) - 1.0), px(0.0)),
                        size(px(2.0), bounds.size.height),
                    );
                    window.paint_quad(fill(head, theme::TEXT));
                },
            )
            .absolute()
            .size_full(),
        )
        .children(labels.into_iter().map(move |(seconds, label)| {
            div()
                .absolute()
                .top(px(7.0))
                .left(gpui::relative((seconds / duration_f32).clamp(0.0, 1.0)))
                .ml(px(3.0))
                .text_size(px(10.0))
                .text_color(theme::MUTED)
                .child(label)
        }))
}
