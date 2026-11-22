//! The deck overview: the whole track in its three bands, as rekordbox and the CDJ-3000 show
//! it, with the hot cues, memory cues, the set's mix cues, saved loops, minute ticks, the last
//! 30 seconds and a phrase strip. That part is a cached view drawn again only when the track
//! or its marks change; the played part, the playhead, the active loop and the pointer are
//! drawn over it each frame.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use dubplate_engine::DeckId;
use gpui::prelude::*;
use gpui::{
    Bounds, Context, FontWeight, MouseButton, MouseDownEvent, MouseMoveEvent, PathBuilder, Pixels,
    Rgba, StyleRefinement, Window, canvas, div, fill, point, px, relative, size,
};

use super::{DeckFrame, DubplateApp};
use crate::collection::{CollectionTrack, clock};
use crate::controls::{Orientation, fraction_at};
use crate::deck::{HotCues, OverviewBars, deck_letter, pad_position};
use crate::fonts::MONO_FAMILY;
use crate::phrases::{PhraseSpan, Section, phrase_at};
use crate::theme;
use crate::waveform::{BandLevels, LaneLoop, LoopLook};
use crate::widgets::{InteractiveControl, adjusted_value};

/// The overview's height: the cue row, the bands and the phrase strip.
pub(super) const OVERVIEW_HEIGHT: Pixels = px(42.0);
/// The row above the bands where the hot cue letters and memory cues sit.
const CUE_ROW: f32 = 11.0;
/// The phrase strip along the bottom.
const PHRASE_STRIP: f32 = 5.0;
const BAR_SPACING: f32 = 3.0;
const BAR_WIDTH: f32 = 2.0;
/// The end of the track that is tinted, and under which the time left turns to attention.
pub(super) const ENDING_SECONDS: f64 = 30.0;
const SECONDS_PER_MINUTE: f64 = 60.0;
const HOT_CUE_LETTER_WIDTH: f32 = 10.0;
const ACTIVE_LOOP_FILL: f32 = 0.35;

/// What the cached part of a deck's overview draws. Built each frame without allocating and
/// compared with what is drawn, so the view is painted again only when something changed.
#[derive(Clone)]
pub(super) struct OverviewContent {
    bars: Option<Rc<RefCell<OverviewBars>>>,
    track: Arc<CollectionTrack>,
    phrases: Arc<[PhraseSpan]>,
    hot_cues: HotCues,
    mix_in: Option<f64>,
    mix_out: Option<f64>,
    duration: f64,
}

impl PartialEq for OverviewContent {
    fn eq(&self, other: &Self) -> bool {
        let same_bars = match (&self.bars, &other.bars) {
            (Some(first), Some(second)) => Rc::ptr_eq(first, second),
            (None, None) => true,
            _ => false,
        };
        same_bars
            && Arc::ptr_eq(&self.track, &other.track)
            && Arc::ptr_eq(&self.phrases, &other.phrases)
            && self.hot_cues == other.hot_cues
            && self.mix_in == other.mix_in
            && self.mix_out == other.mix_out
            && self.duration == other.duration
    }
}

/// The cached part of one deck's overview.
#[derive(Default)]
pub(super) struct DeckOverview {
    content: Option<OverviewContent>,
}

impl DeckOverview {
    /// Shows `content`, painting again only when it differs from what is shown.
    fn show(&mut self, content: Option<OverviewContent>, cx: &mut Context<'_, Self>) {
        if self.content != content {
            self.content = content;
            cx.notify();
        }
    }
}

impl Render for DeckOverview {
    fn render(&mut self, _: &mut Window, _: &mut Context<'_, Self>) -> impl IntoElement {
        let Some(content) = self.content.clone() else {
            return div().size_full();
        };
        let letters: Vec<(f32, char, Rgba)> = content
            .hot_cues
            .iter()
            .map(|(pad, seconds)| {
                (
                    (seconds / content.duration).clamp(0.0, 1.0) as f32,
                    pad.letter(),
                    theme::HOT_CUE_TINTS[pad_position(pad)],
                )
            })
            .collect();
        div()
            .relative()
            .size_full()
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, (), window, _| paint_content(bounds, &content, window),
                )
                .absolute()
                .size_full(),
            )
            .children(letters.into_iter().map(|(fraction, letter, tint)| {
                div()
                    .absolute()
                    .top_0()
                    .left(relative(fraction))
                    .ml(px(-HOT_CUE_LETTER_WIDTH / 2.0))
                    .w(px(HOT_CUE_LETTER_WIDTH))
                    .h(px(CUE_ROW - 1.0))
                    .rounded(px(2.0))
                    .bg(tint)
                    .flex()
                    .items_center()
                    .justify_center()
                    .font_family(MONO_FAMILY)
                    .text_size(px(9.0))
                    .line_height(px(CUE_ROW - 1.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::ON_DECK)
                    .child(letter.to_string())
            }))
    }
}

/// The colour of a section on the phrase strip.
fn section_colour(section: Section) -> Rgba {
    match section {
        Section::Intro => theme::PHRASE_INTRO,
        Section::Build => theme::PHRASE_BUILD,
        Section::Drop => theme::PHRASE_DROP,
        Section::Breakdown => theme::PHRASE_BREAKDOWN,
        Section::Outro => theme::PHRASE_OUTRO,
        Section::Other => theme::PHRASE_OTHER,
    }
}

/// Where the overview's parts sit, top to bottom.
struct OverviewArea {
    bounds: Bounds<Pixels>,
    width: f32,
    bands_top: f32,
    bands_bottom: f32,
    duration: f64,
}

impl OverviewArea {
    fn new(bounds: Bounds<Pixels>, duration: f64) -> Self {
        let height = f32::from(bounds.size.height);
        Self {
            bounds,
            width: f32::from(bounds.size.width),
            bands_top: CUE_ROW,
            bands_bottom: height - PHRASE_STRIP - 1.0,
            duration: duration.max(1.0),
        }
    }

    fn x_of(&self, seconds: f64) -> f32 {
        (seconds / self.duration).clamp(0.0, 1.0) as f32 * self.width
    }

    fn rect(&self, left: f32, top: f32, width: f32, height: f32) -> Bounds<Pixels> {
        Bounds::new(
            self.bounds.origin + point(px(left), px(top)),
            size(px(width.max(0.0)), px(height.max(0.0))),
        )
    }

    /// A rectangle the full height of the bands.
    fn band_span(&self, left: f32, width: f32) -> Bounds<Pixels> {
        self.rect(
            left,
            self.bands_top,
            width,
            self.bands_bottom - self.bands_top,
        )
    }
}

/// The cached drawing, lowest layer first.
fn paint_content(bounds: Bounds<Pixels>, content: &OverviewContent, window: &mut Window) {
    let area = OverviewArea::new(bounds, content.duration);
    let ending_from = area.x_of(content.duration - ENDING_SECONDS);
    window.paint_quad(fill(
        area.band_span(ending_from, area.width - ending_from),
        theme::ENDING_TINT,
    ));
    for &(start, end) in &content.track.saved_loops {
        let left = area.x_of(start);
        let width = (area.x_of(end) - left).max(1.0);
        window.paint_quad(fill(
            area.band_span(left, width),
            theme::faded(theme::SAVED_LOOP, 0.14),
        ));
        window.paint_quad(fill(
            area.rect(left, area.bands_top, width, 2.0),
            theme::SAVED_LOOP,
        ));
    }
    if let Some(bars) = &content.bars {
        let bar_count = (area.width / BAR_SPACING).floor().max(0.0) as usize;
        paint_band_bars(&area, bars.borrow_mut().bars(bar_count), window);
    }
    let minutes = (content.duration / SECONDS_PER_MINUTE).floor() as usize;
    for minute in 1..=minutes {
        let x = area.x_of(minute as f64 * SECONDS_PER_MINUTE);
        window.paint_quad(fill(
            area.rect(x, area.bands_top, 1.0, 3.0),
            theme::MINUTE_TICK,
        ));
    }
    for (seconds, colour) in [
        (content.mix_in, theme::MIX_IN_MARK),
        (content.mix_out, theme::MIX_OUT_MARK),
    ] {
        if let Some(seconds) = seconds {
            let x = area.x_of(seconds);
            window.paint_quad(fill(area.band_span(x - 0.5, 1.5), colour));
            paint_triangle(&area, x, area.bands_top, colour, window);
        }
    }
    for &seconds in &content.track.memory_cues {
        paint_triangle(&area, area.x_of(seconds), 1.0, theme::MEMORY_CUE, window);
    }
    let strip_top = area.bands_bottom + 1.0;
    for phrase in content.phrases.iter() {
        let left = area.x_of(phrase.start_seconds);
        let width = (area.x_of(phrase.end_seconds) - left - 1.0).max(1.0);
        window.paint_quad(fill(
            area.rect(left, strip_top, width, PHRASE_STRIP),
            section_colour(phrase.section),
        ));
    }
}

/// A small triangle pointing down at `x`, its top edge at `top`.
fn paint_triangle(area: &OverviewArea, x: f32, top: f32, colour: Rgba, window: &mut Window) {
    let origin = area.bounds.origin;
    let mut triangle = PathBuilder::fill();
    triangle.move_to(origin + point(px(x - 3.5), px(top)));
    triangle.line_to(origin + point(px(x + 3.5), px(top)));
    triangle.line_to(origin + point(px(x), px(top + 5.0)));
    triangle.close();
    if let Ok(path) = triangle.build() {
        window.paint_path(path, colour);
    }
}

/// The three bands of each bar, standing on the bottom of the band area: lows behind, highs
/// in front, as rekordbox draws its 3-band overview.
fn paint_band_bars(area: &OverviewArea, bars: &[BandLevels], window: &mut Window) {
    let tallest = area.bands_bottom - area.bands_top;
    for (index, levels) in bars.iter().enumerate() {
        let left = index as f32 * BAR_SPACING;
        for (level, colour) in [
            (levels.low, theme::BAND_LOW),
            (levels.mid, theme::BAND_MID),
            (levels.high, theme::BAND_HIGH),
        ] {
            let height = (level * tallest).max(0.0);
            if height < 0.5 {
                continue;
            }
            window.paint_quad(fill(
                area.rect(left, area.bands_bottom - height, BAR_WIDTH, height),
                colour,
            ));
        }
    }
}

/// What the overview draws over the cached part each frame.
#[derive(Debug, Clone, Copy)]
struct OverviewMotion {
    progress: Option<f32>,
    hover: Option<f32>,
    lane_loop: Option<LaneLoop>,
    duration: f64,
    colour: Rgba,
}

fn paint_motion(bounds: Bounds<Pixels>, motion: OverviewMotion, window: &mut Window) {
    let area = OverviewArea::new(bounds, motion.duration);
    if let Some(lane_loop) = motion
        .lane_loop
        .filter(|lane_loop| lane_loop.look != LoopLook::Pending)
    {
        let left = area.x_of(lane_loop.start_seconds);
        let width = (area.x_of(lane_loop.end_seconds) - left).max(2.0);
        let opacity = if lane_loop.look == LoopLook::Active {
            ACTIVE_LOOP_FILL
        } else {
            ACTIVE_LOOP_FILL / 2.0
        };
        window.paint_quad(fill(
            area.band_span(left, width),
            theme::faded(motion.colour, opacity),
        ));
    }
    if let Some(progress) = motion.progress {
        let played = progress * area.width;
        window.paint_quad(fill(area.band_span(0.0, played), theme::PLAYED_SHADE));
        window.paint_quad(fill(
            area.rect(played - 1.0, 0.0, 2.0, f32::from(bounds.size.height)),
            theme::PLAYHEAD,
        ));
    }
    if let Some(hover) = motion.hover {
        window.paint_quad(fill(
            area.band_span(hover * area.width, 1.0),
            theme::faded(theme::TEXT, 0.6),
        ));
    }
}

impl DubplateApp {
    /// The set's mix-in and mix-out of `track`, when it is in the set, in seconds.
    fn set_mix_cues(&self, track: &CollectionTrack) -> (Option<f64>, Option<f64>) {
        let entry =
            self.set_editor.as_ref().and_then(|editor| {
                editor.set().entries.iter().find(|entry| {
                    track.track.file_path.as_deref() == Some(entry.track_path.as_path())
                })
            });
        let seconds = |milliseconds: Option<u32>| milliseconds.map(|ms| f64::from(ms) / 1000.0);
        entry.map_or((None, None), |entry| {
            (seconds(entry.mix_in_ms), seconds(entry.mix_out_ms))
        })
    }
}

/// The deck overview, which seeks where it is clicked and names the time and phrase under the
/// pointer.
pub(super) fn render(
    app: &DubplateApp,
    deck: DeckId,
    frame: &DeckFrame,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement {
    let side = app.deck(deck);
    let duration = frame.duration_seconds.filter(|duration| *duration > 0.0);
    let visuals = side.visuals();
    let content =
        side.track
            .as_ref()
            .zip(visuals)
            .zip(duration)
            .map(|((track, visuals), duration)| {
                let (mix_in, mix_out) = app.set_mix_cues(track);
                OverviewContent {
                    bars: visuals.overview.clone(),
                    track: Arc::clone(track),
                    phrases: Arc::clone(&visuals.phrases),
                    hot_cues: side.hot_cues,
                    mix_in,
                    mix_out,
                    duration,
                }
            });
    let cached = app.overviews[deck.index()].clone();
    cached.update(cx, |overview, cx| overview.show(content, cx));
    let progress =
        duration.map(|duration| (frame.playhead_seconds / duration).clamp(0.0, 1.0) as f32);
    let motion = OverviewMotion {
        progress,
        hover: side.overview_hover,
        lane_loop: app
            .loop_state(deck)
            .and_then(|state| LaneLoop::of(&state, frame.playhead_seconds)),
        duration: duration.unwrap_or(1.0),
        colour: theme::deck_colour(deck),
    };
    let hover_label = side
        .overview_hover
        .zip(duration)
        .map(|(fraction, duration)| {
            let seconds = f64::from(fraction) * duration;
            let phrase = visuals.and_then(|visuals| phrase_at(&visuals.phrases, seconds));
            let label = match phrase {
                Some(phrase) => format!("{} \u{b7} {}", clock(seconds), phrase.name),
                None => clock(seconds),
            };
            (fraction, label)
        });
    let hit_areas = Rc::clone(&app.hit_areas);
    div()
        .id(("overview", deck.index()))
        .accessible_slider(
            format!("Deck {} track position", deck_letter(deck)),
            progress.unwrap_or_default(),
        )
        .aria_orientation(gpui::Orientation::Horizontal)
        .map(|element| {
            crate::widgets::numeric_actions(element, cx, move |this, change, cx| {
                if let Some(duration) = this
                    .duration_seconds(deck)
                    .filter(|duration| *duration > 0.0)
                    .filter(|_| !this.deck_state(deck).playing)
                {
                    let fraction = (this.deck_state(deck).position_seconds / duration) as f32;
                    this.seek_to_fraction(deck, change.apply(fraction), cx);
                }
            })
        })
        .on_key_down(cx.listener(move |this, event, _, cx| {
            let duration = this.duration_seconds(deck).unwrap_or(0.0);
            // A stray key must never move a playing deck: the DJ would lose the mix.
            if duration <= 0.0 || this.deck_state(deck).playing {
                return;
            }
            let current = (this.deck_state(deck).position_seconds / duration) as f32;
            if let Some(fraction) = adjusted_value(event, current, 0.0) {
                this.seek_to_fraction(deck, fraction, cx);
                cx.stop_propagation();
            }
        }))
        .relative()
        .h(OVERVIEW_HEIGHT)
        .rounded(px(4.0))
        .bg(theme::RAISED)
        .overflow_hidden()
        .cursor_pointer()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                if let Some(bounds) = this.hit_areas.overviews[deck.index()].get() {
                    let fraction = fraction_at(event.position, bounds, Orientation::Horizontal);
                    this.seek_to_fraction(deck, fraction, cx);
                }
                cx.stop_propagation();
            }),
        )
        .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
            let Some(bounds) = this.hit_areas.overviews[deck.index()].get() else {
                return;
            };
            let hover = bounds
                .contains(&event.position)
                .then(|| fraction_at(event.position, bounds, Orientation::Horizontal));
            if this.decks[deck.index()].overview_hover != hover {
                this.decks[deck.index()].overview_hover = hover;
                cx.notify();
            }
        }))
        .child(
            cached
                .cached(StyleRefinement::default().size_full())
                .into_any_element(),
        )
        .child(
            canvas(
                move |bounds, _, _| hit_areas.overviews[deck.index()].set(Some(bounds)),
                move |bounds, (), window, _| paint_motion(bounds, motion, window),
            )
            .absolute()
            .size_full(),
        )
        .children(hover_label.map(|(fraction, label)| {
            div()
                .absolute()
                .bottom(px(PHRASE_STRIP + 2.0))
                .left(relative(fraction))
                .ml(px(4.0))
                .px(px(4.0))
                .rounded(px(3.0))
                .bg(theme::PANEL)
                .border_1()
                .border_color(theme::LINE)
                .font_family(MONO_FAMILY)
                .text_size(px(10.0))
                .text_color(theme::TEXT)
                .whitespace_nowrap()
                .child(label)
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_overview_places_time_across_its_width() {
        let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(600.0), px(42.0)));
        let area = OverviewArea::new(bounds, 300.0);
        assert_eq!(area.x_of(150.0), 300.0);
        assert_eq!(
            area.x_of(400.0),
            600.0,
            "past the end stays on the overview"
        );
        assert_eq!(area.bands_top, CUE_ROW);
        assert_eq!(area.bands_bottom, 42.0 - PHRASE_STRIP - 1.0);
    }
}
