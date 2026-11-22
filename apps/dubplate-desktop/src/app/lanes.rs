//! The two zoomed waveforms, stacked, scrolling past one playhead in the middle, with the beat
//! grid behind them and the hot cues above.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use dubplate_engine::{BeatLength, DeckId};
use gpui::prelude::*;
use gpui::{
    Bounds, Context, FontWeight, MouseButton, MouseDownEvent, PathBuilder, Pixels, Point, Rgba,
    SharedString, Window, canvas, div, fill, point, px, relative, size,
};

use super::drop_target::accept_drops;
use super::{DeckFrame, DubplateApp};
use crate::deck::DeckContent;
use crate::deck::deck_letter;
use crate::fonts::MONO_FAMILY;
use crate::frame_stats::FrameStats;
use crate::shortcuts::printed_keys;
use crate::theme;
use crate::waveform::{BandLevels, BeatMarks, LaneLoop, LaneView, LoopLook, Zoom, ZoomedWaveform};

const LANE_HEIGHT: Pixels = px(74.0);
const PLAYHEAD_WIDTH: Pixels = px(2.0);
const FLAG_WIDTH: f32 = 14.0;
/// The quietest band still drawn, in pixels of half height.
const SHORTEST_HALF_HEIGHT: f32 = 0.5;
/// How much of the deck's colour fills an active loop, a loop waiting for LOOP OUT, and the
/// outline of a loop set but not playing.
const ACTIVE_LOOP_FILL: f32 = 0.22;
const PENDING_LOOP_FILL: f32 = 0.12;
const SET_LOOP_OUTLINE: f32 = 0.5;
const LOOP_EDGE_WIDTH: f32 = 2.0;
/// How strongly the slip shadow playhead shows.
const SLIP_SHADOW_OPACITY: f32 = 0.55;

/// What a deck lane draws over its waveform besides the beats: the loop and the slip shadow.
#[derive(Debug, Clone, Copy)]
pub(super) struct LaneMarks {
    /// The deck's loop, when it has one.
    pub lane_loop: Option<LaneLoop>,
    /// Where a slipping deck would be now.
    pub slip_shadow: Option<f64>,
    /// The deck's colour.
    pub colour: Rgba,
    /// The CUE button's point, in seconds.
    pub cue_point: Option<f64>,
    /// A cue just set, in seconds, and how strongly it still pulses.
    pub pulse: Option<(f64, f32)>,
}

/// The loop length as its label reads, such as `4` or `1/2`, without allocating for the
/// lengths the pads and buttons make.
fn loop_label(length: BeatLength) -> SharedString {
    let label = match length {
        BeatLength::SIXTEENTH => "1/16",
        BeatLength::EIGHTH => "1/8",
        BeatLength::QUARTER => "1/4",
        BeatLength::HALF => "1/2",
        BeatLength::ONE => "1",
        BeatLength::TWO => "2",
        BeatLength::FOUR => "4",
        BeatLength::EIGHT => "8",
        BeatLength::SIXTEEN => "16",
        BeatLength::THIRTY_TWO => "32",
        other => return other.to_string().into(),
    };
    SharedString::new_static(label)
}

pub(super) fn render(
    app: &DubplateApp,
    frames: &[DeckFrame; 2],
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement {
    div()
        .relative()
        .flex()
        .flex_col()
        .flex_none()
        .gap(px(6.0))
        .px(theme::GUTTER)
        .py(px(8.0))
        .bg(theme::active().background)
        .border_b_1()
        .border_color(theme::active().line)
        .child(lane(app, DeckId::A, &frames[0], cx))
        .child(lane(app, DeckId::B, &frames[1], cx))
        .child(
            div()
                .absolute()
                .top(px(4.0))
                .bottom(px(4.0))
                .left(relative(0.5))
                .ml(-PLAYHEAD_WIDTH / 2.0)
                .w(PLAYHEAD_WIDTH)
                .bg(theme::active().playhead),
        )
}

fn lane(
    app: &DubplateApp,
    deck: DeckId,
    frame: &DeckFrame,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement {
    let content = &app.deck(deck).content;
    let visuals = app.deck(deck).visuals();
    let zoomed = visuals.and_then(|visuals| visuals.zoomed.clone());
    let beats = visuals.map(|visuals| Arc::clone(&visuals.beats));
    let playhead_seconds = frame.playhead_seconds;
    let tempo = frame.state.tempo;
    let marks = LaneMarks {
        lane_loop: app
            .loop_state(deck)
            .and_then(|state| LaneLoop::of(&state, playhead_seconds)),
        slip_shadow: app.slip_shadow(deck),
        colour: theme::deck_colour(deck),
        cue_point: app
            .deck(deck)
            .visuals()
            .map(|_| frame.state.cue_point_seconds),
        pulse: app
            .deck(deck)
            .marker_pulse
            .and_then(|pulse| Some((pulse.seconds, pulse.strength(Instant::now())?))),
    };
    let stats = app.frame_stats.clone();
    let notice = app.deck_notice(deck).map(|notice| notice.to_string());
    let hint_colour = if notice.is_some() {
        theme::active().alert
    } else {
        theme::active().muted
    };
    let hint = notice.or_else(|| match content {
        DeckContent::Empty => Some(format!(
            "Select a track and press {} to load it on deck {}",
            printed_keys(match deck {
                DeckId::A => "shift-left",
                DeckId::B => "shift-right",
            }),
            deck_letter(deck)
        )),
        DeckContent::Loading => Some("Loading\u{2026}".to_owned()),
        DeckContent::Failed(message) => Some(message.clone()),
        DeckContent::Loaded(visuals) if visuals.zoomed.is_none() => Some(
            visuals
                .analysis_problem
                .clone()
                .unwrap_or_else(|| "rekordbox saved no waveform for this track".to_owned()),
        ),
        DeckContent::Loaded(_) => None,
    });
    let zoom = zoomed
        .as_ref()
        .map_or(Zoom::DEFAULT, |waveform| waveform.zoom());
    let pixels_per_track_second = zoom.pixels_per_track_second(tempo);
    let loop_label = marks.lane_loop.and_then(|lane_loop| {
        let length = lane_loop.length?;
        let offset = (lane_loop.start_seconds - playhead_seconds) * pixels_per_track_second;
        Some((loop_label(length), offset as f32))
    });
    let flags = match content {
        DeckContent::Loaded(_) => app
            .deck(deck)
            .hot_cues
            .iter()
            .map(|(pad, seconds)| {
                let offset = (seconds - playhead_seconds) * pixels_per_track_second;
                (pad.letter(), offset as f32)
            })
            .collect(),
        DeckContent::Empty | DeckContent::Loading | DeckContent::Failed(_) => Vec::new(),
    };
    let lane = div()
        .id(("lane", deck.index()))
        .when(matches!(content, DeckContent::Loaded(_)), |lane| {
            let held = app.is_dragging_lane(deck);
            lane.when(held, |lane| lane.cursor_grabbing())
                .when(!held, |lane| lane.cursor_grab())
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        this.begin_lane_drag(
                            deck,
                            event.position,
                            event.modifiers.shift,
                            window,
                            cx,
                        );
                        cx.stop_propagation();
                    }),
                )
        })
        .relative()
        .h(LANE_HEIGHT)
        .rounded(px(4.0))
        .overflow_hidden()
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, (), window, _| {
                    let started = Instant::now();
                    paint_lane(
                        bounds,
                        zoomed.as_deref(),
                        beats.as_deref(),
                        playhead_seconds,
                        tempo,
                        Some(&marks),
                        window,
                    );
                    record_paint(stats.as_ref(), started);
                },
            )
            .absolute()
            .size_full(),
        )
        .children(flags.into_iter().map(|(letter, offset)| {
            div()
                .absolute()
                .top_0()
                .left(relative(0.5))
                .ml(px(offset - FLAG_WIDTH / 2.0))
                .min_w(px(FLAG_WIDTH))
                .px(px(3.0))
                .rounded(px(3.0))
                .bg(theme::active().cue_flag)
                .text_color(theme::active().on_deck)
                .font_family(MONO_FAMILY)
                .text_size(px(10.0))
                .line_height(px(14.0))
                .font_weight(FontWeight::SEMIBOLD)
                .flex()
                .justify_center()
                .child(letter.to_string())
        }))
        .children(loop_label.map(|(label, offset)| {
            div()
                .absolute()
                .bottom(px(2.0))
                .left(relative(0.5))
                .ml(px(offset + LOOP_EDGE_WIDTH + 2.0))
                .px(px(4.0))
                .rounded(px(3.0))
                .bg(marks.colour)
                .text_color(theme::active().on_deck)
                .font_family(MONO_FAMILY)
                .text_size(px(10.0))
                .line_height(px(14.0))
                .font_weight(FontWeight::SEMIBOLD)
                .child(label)
        }))
        .children(hint.map(|hint| {
            div()
                .absolute()
                .size_full()
                .flex()
                .items_center()
                .pl(px(28.0))
                .text_size(theme::SMALL_TEXT_SIZE)
                .text_color(hint_colour)
                .child(hint)
        }))
        .child(
            div()
                .absolute()
                .left_0()
                .bottom(px(2.0))
                .px(px(5.0))
                .py(px(1.0))
                .rounded(px(3.0))
                .bg(theme::deck_colour(deck))
                .text_color(theme::active().on_deck)
                .font_family(MONO_FAMILY)
                .text_size(px(10.0))
                .font_weight(FontWeight::SEMIBOLD)
                .child(deck_letter(deck)),
        );
    accept_drops(lane, deck, cx)
}

fn record_paint(stats: Option<&Rc<RefCell<FrameStats>>>, started: Instant) {
    if let Some(stats) = stats {
        stats.borrow_mut().record_lane_paint(started.elapsed());
    }
}

/// The colours of the three bands, unplayed or played.
struct BandColours {
    low: Rgba,
    mid: Rgba,
    high: Rgba,
}

impl BandColours {
    fn at_opacity(opacity: f32) -> Self {
        Self {
            low: theme::faded(theme::active().band_low, opacity),
            mid: theme::faded(theme::active().band_mid, opacity),
            high: theme::faded(theme::active().band_high, opacity),
        }
    }
}

pub(super) fn paint_lane(
    bounds: Bounds<Pixels>,
    zoomed: Option<&ZoomedWaveform>,
    beats: Option<&BeatMarks>,
    playhead_seconds: f64,
    tempo: f64,
    marks: Option<&LaneMarks>,
    window: &mut Window,
) {
    let width = f32::from(bounds.size.width);
    let zoom = zoomed.map_or(Zoom::DEFAULT, ZoomedWaveform::zoom);
    let view = LaneView {
        width,
        playhead_seconds,
        zoom,
        tempo,
    };
    if let Some(marks) = marks
        && let Some(lane_loop) = marks.lane_loop
    {
        paint_loop_band(bounds, &view, lane_loop, marks.colour, window);
    }
    if let Some(zoomed) = zoomed {
        paint_waveform(bounds, zoomed, &view, window);
    }
    if let Some(beats) = beats {
        for mark in beats.within(view.seconds_on_screen()) {
            let colour = if mark.downbeat {
                theme::active().downbeat_line
            } else {
                theme::active().beat_line
            };
            let line = Bounds::new(
                bounds.origin + point(px(view.x_of(mark.seconds).floor()), px(0.0)),
                size(px(1.0), bounds.size.height),
            );
            window.paint_quad(fill(line, colour));
        }
    }
    if let Some(marks) = marks {
        if let Some(lane_loop) = marks.lane_loop {
            paint_loop_edges(bounds, &view, lane_loop, marks.colour, window);
        }
        if let Some(cue_point) = marks.cue_point {
            paint_cue_point(bounds, view.x_of(cue_point), window);
        }
        if let Some((seconds, strength)) = marks.pulse {
            paint_pulse(bounds, view.x_of(seconds), strength, window);
        }
        if let Some(shadow) = marks.slip_shadow {
            let line = Bounds::new(
                bounds.origin + point(px(view.x_of(shadow).floor()), px(0.0)),
                size(px(1.0), bounds.size.height),
            );
            window.paint_quad(fill(
                line,
                theme::faded(theme::active().playhead, SLIP_SHADOW_OPACITY),
            ));
        }
    }
}

/// The CUE button's point: a line with a small orange triangle standing on the lane's bottom.
pub(super) fn paint_cue_point(bounds: Bounds<Pixels>, x: f32, window: &mut Window) {
    let height = f32::from(bounds.size.height);
    let line = Bounds::new(
        bounds.origin + point(px(x.floor()), px(0.0)),
        size(px(1.0), bounds.size.height),
    );
    window.paint_quad(fill(line, theme::faded(theme::active().cue_point, 0.55)));
    let mut triangle = PathBuilder::fill();
    triangle.move_to(bounds.origin + point(px(x - 5.0), px(height)));
    triangle.line_to(bounds.origin + point(px(x + 5.0), px(height)));
    triangle.line_to(bounds.origin + point(px(x), px(height - 7.0)));
    triangle.close();
    if let Ok(path) = triangle.build() {
        window.paint_path(path, theme::active().cue_point);
    }
}

/// A glow where a cue was just set, fading with `strength` from 1.0 to 0.0.
pub(super) fn paint_pulse(bounds: Bounds<Pixels>, x: f32, strength: f32, window: &mut Window) {
    let half_width = 3.0 + 6.0 * (1.0 - strength);
    let glow = Bounds::new(
        bounds.origin + point(px(x - half_width), px(0.0)),
        size(px(half_width * 2.0), bounds.size.height),
    );
    window.paint_quad(fill(
        glow,
        theme::faded(theme::active().text, 0.45 * strength),
    ));
}

/// The tint behind an active loop or one waiting for LOOP OUT, under the waveform.
fn paint_loop_band(
    bounds: Bounds<Pixels>,
    view: &LaneView,
    lane_loop: LaneLoop,
    colour: Rgba,
    window: &mut Window,
) {
    let opacity = match lane_loop.look {
        LoopLook::Active => ACTIVE_LOOP_FILL,
        LoopLook::Pending => PENDING_LOOP_FILL,
        LoopLook::Set => return,
    };
    let Some(span) = lane_loop.span(view) else {
        return;
    };
    let band = Bounds::new(
        bounds.origin + point(px(span.start), px(0.0)),
        size(px(span.end - span.start), bounds.size.height),
    );
    window.paint_quad(fill(band, theme::faded(colour, opacity)));
}

/// The loop's in and out edges over the waveform: solid for an active loop, the in edge alone
/// while LOOP OUT is awaited, and a dim outline for a loop set but not playing.
fn paint_loop_edges(
    bounds: Bounds<Pixels>,
    view: &LaneView,
    lane_loop: LaneLoop,
    colour: Rgba,
    window: &mut Window,
) {
    let height = bounds.size.height;
    let edge = |x: f32, width: f32, colour: Rgba, window: &mut Window| {
        let line = Bounds::new(
            bounds.origin + point(px(x), px(0.0)),
            size(px(width), height),
        );
        window.paint_quad(fill(line, colour));
    };
    let in_x = view.x_of(lane_loop.start_seconds);
    let out_x = view.x_of(lane_loop.end_seconds);
    let on_lane = |x: f32| (0.0..view.width).contains(&x);
    match lane_loop.look {
        LoopLook::Active => {
            for x in [in_x, out_x - LOOP_EDGE_WIDTH] {
                if on_lane(x) {
                    edge(x, LOOP_EDGE_WIDTH, colour, window);
                }
            }
        }
        LoopLook::Pending => {
            if on_lane(in_x) {
                edge(in_x, LOOP_EDGE_WIDTH, colour, window);
            }
        }
        LoopLook::Set => {
            let dim = theme::faded(colour, SET_LOOP_OUTLINE);
            for x in [in_x, out_x - 1.0] {
                if on_lane(x) {
                    edge(x, 1.0, dim, window);
                }
            }
            if let Some(span) = lane_loop.span(view) {
                for y in [0.0, f32::from(height) - 1.0] {
                    let line = Bounds::new(
                        bounds.origin + point(px(span.start), px(y)),
                        size(px(span.end - span.start), px(1.0)),
                    );
                    window.paint_quad(fill(line, dim));
                }
            }
        }
    }
}

/// The waveform's columns, the part already played dimmed.
fn paint_waveform(
    bounds: Bounds<Pixels>,
    zoomed: &ZoomedWaveform,
    view: &LaneView,
    window: &mut Window,
) {
    let width = f32::from(bounds.size.width);
    let height = f32::from(bounds.size.height);
    let unplayed = BandColours::at_opacity(1.0);
    let played = BandColours::at_opacity(theme::PLAYED_OPACITY);
    let centre_x = width / 2.0;
    let area = BandArea {
        origin: bounds.origin,
        centre_y: height / 2.0,
        tallest: height / 2.0 - 2.0,
    };
    let visible = view.visible_columns(zoomed.columns().len());
    for (offset, levels) in zoomed.columns()[visible.range].iter().enumerate() {
        let x = visible.first_x + offset as f32 * visible.column_width;
        let colours = if x + visible.column_width <= centre_x {
            &played
        } else {
            &unplayed
        };
        area.paint_column(x, visible.column_width, *levels, colours, window);
    }
}

/// Where a lane's bands are drawn: mirrored around its middle line.
struct BandArea {
    origin: Point<Pixels>,
    centre_y: f32,
    tallest: f32,
}

impl BandArea {
    /// Draws one column `width` wide at `x`, lows behind, highs in front.
    fn paint_column(
        &self,
        x: f32,
        width: f32,
        levels: BandLevels,
        colours: &BandColours,
        window: &mut Window,
    ) {
        for (level, colour) in [
            (levels.low, colours.low),
            (levels.mid, colours.mid),
            (levels.high, colours.high),
        ] {
            let half_height = level * self.tallest;
            if half_height < SHORTEST_HALF_HEIGHT {
                continue;
            }
            let band = Bounds::new(
                self.origin + point(px(x), px(self.centre_y - half_height)),
                size(px(width), px(half_height * 2.0)),
            );
            window.paint_quad(fill(band, colour));
        }
    }
}
