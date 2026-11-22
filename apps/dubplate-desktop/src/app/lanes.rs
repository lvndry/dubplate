//! The two zoomed waveforms, stacked, scrolling past one playhead in the middle, with the beat
//! grid behind them and the hot cues above.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use dubplate_engine::DeckId;
use gpui::prelude::*;
use gpui::{
    Bounds, Context, FontWeight, Pixels, Point, Rgba, Window, canvas, div, fill, point, px,
    relative, size,
};

use super::drop_target::accept_drops;
use super::{DeckFrame, DubplateApp};
use crate::deck::DeckContent;
use crate::deck::deck_letter;
use crate::fonts::MONO_FAMILY;
use crate::frame_stats::FrameStats;
use crate::shortcuts::printed_keys;
use crate::theme;
use crate::waveform::{BandLevels, BeatMarks, LaneView, Zoom, ZoomedWaveform};

const LANE_HEIGHT: Pixels = px(74.0);
const PLAYHEAD_WIDTH: Pixels = px(2.0);
const FLAG_WIDTH: f32 = 14.0;
/// Each zoomed column is one pixel wide: the waveform is resampled to the zoom.
const COLUMN_WIDTH: f32 = 1.0;
/// The quietest band still drawn, in pixels of half height.
const SHORTEST_HALF_HEIGHT: f32 = 0.5;

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
        .bg(theme::BACKGROUND)
        .border_b_1()
        .border_color(theme::LINE)
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
                .bg(theme::PLAYHEAD),
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
    let stats = app.frame_stats.clone();
    let notice = app.deck_notice(deck).map(|notice| notice.to_string());
    let hint_colour = if notice.is_some() {
        theme::ALERT
    } else {
        theme::MUTED
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
    let flags = match content {
        DeckContent::Loaded(_) => app
            .deck(deck)
            .hot_cues
            .iter()
            .map(|(pad, seconds)| {
                let offset = (seconds - playhead_seconds) * zoom.pixels_per_second();
                (pad.letter(), offset as f32)
            })
            .collect(),
        DeckContent::Empty | DeckContent::Loading | DeckContent::Failed(_) => Vec::new(),
    };
    let lane = div()
        .id(("lane", deck.index()))
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
                .bg(theme::CUE_FLAG)
                .text_color(theme::ON_DECK)
                .font_family(MONO_FAMILY)
                .text_size(px(10.0))
                .line_height(px(14.0))
                .font_weight(FontWeight::SEMIBOLD)
                .flex()
                .justify_center()
                .child(letter.to_string())
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
                .text_color(theme::ON_DECK)
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
            low: theme::faded(theme::BAND_LOW, opacity),
            mid: theme::faded(theme::BAND_MID, opacity),
            high: theme::faded(theme::BAND_HIGH, opacity),
        }
    }
}

pub(super) fn paint_lane(
    bounds: Bounds<Pixels>,
    zoomed: Option<&ZoomedWaveform>,
    beats: Option<&BeatMarks>,
    playhead_seconds: f64,
    window: &mut Window,
) {
    let width = f32::from(bounds.size.width);
    let zoom = zoomed.map_or(Zoom::DEFAULT, ZoomedWaveform::zoom);
    let view = LaneView {
        width,
        playhead_seconds,
        zoom,
    };
    if let Some(zoomed) = zoomed {
        paint_waveform(bounds, zoomed, &view, window);
    }
    if let Some(beats) = beats {
        for mark in beats.within(view.seconds_on_screen()) {
            let colour = if mark.downbeat {
                theme::DOWNBEAT_LINE
            } else {
                theme::BEAT_LINE
            };
            let line = Bounds::new(
                bounds.origin + point(px(view.x_of(mark.seconds).floor()), px(0.0)),
                size(px(1.0), bounds.size.height),
            );
            window.paint_quad(fill(line, colour));
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
        let x = visible.first_x + offset as f32;
        let colours = if x + COLUMN_WIDTH <= centre_x {
            &played
        } else {
            &unplayed
        };
        area.paint_column(x, *levels, colours, window);
    }
}

/// Where a lane's bands are drawn: mirrored around its middle line.
struct BandArea {
    origin: Point<Pixels>,
    centre_y: f32,
    tallest: f32,
}

impl BandArea {
    /// Draws one column at `x`, lows behind, highs in front.
    fn paint_column(&self, x: f32, levels: BandLevels, colours: &BandColours, window: &mut Window) {
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
                size(px(COLUMN_WIDTH), px(half_height * 2.0)),
            );
            window.paint_quad(fill(band, colour));
        }
    }
}
