//! The deck overview: the whole track in its three bands, as rekordbox and the CDJ-3000 show
//! it, with the hot cues, memory cues, the set's mix cues, saved loops, minute ticks, the last
//! 30 seconds and a phrase strip. That part is a cached view drawn again only when the track
//! or its marks change; the played part, the playhead, the active loop and the pointer are
//! drawn over it each frame.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use dubplate_engine::DeckId;
use dubplate_rekordbox::{HotCuePad, TrackCues};
use gpui::prelude::*;
use gpui::{
    Bounds, Context, FontWeight, MouseButton, MouseDownEvent, MouseMoveEvent, PathBuilder, Pixels,
    Rgba, StyleRefinement, Window, canvas, div, fill, point, px, relative, size,
};

use super::{DeckFrame, DubplateApp};
use crate::collection::{CollectionTrack, clock};
use crate::controls::{Orientation, fraction_at};
use crate::deck::{OverviewBars, deck_letter};
use crate::fonts::MONO_FAMILY;
use crate::phrases::{PhraseSpan, Section, phrase_at};
use crate::theme;
use crate::waveform::{BandLevels, LaneLoop, LoopLook};
use crate::widgets::{InteractiveControl, adjusted_value, tooltip};

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
const PLAYHEAD_WIDTH: f32 = 2.0;
/// The half width of the small triangles that cap the playhead top and bottom, as rekordbox
/// draws them, so it stands out over loud bars.
const PLAYHEAD_CAP: f32 = 4.0;

/// What the cached part of a deck's overview draws. Built each frame without allocating and
/// compared with what is drawn, so the view is painted again only when something changed.
#[derive(Clone)]
pub(super) struct OverviewContent {
    bars: Option<Rc<RefCell<OverviewBars>>>,
    track: Arc<CollectionTrack>,
    phrases: Arc<[PhraseSpan]>,
    cues: Arc<TrackCues>,
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
            && Arc::ptr_eq(&self.cues, &other.cues)
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
        if self.take(content) {
            cx.notify();
        }
    }

    /// Keeps `content`, returning whether it differs from what is shown and so needs drawing.
    fn take(&mut self, content: Option<OverviewContent>) -> bool {
        let changed = self.content != content;
        if changed {
            self.content = content;
        }
        changed
    }
}

impl Render for DeckOverview {
    fn render(&mut self, _: &mut Window, _: &mut Context<'_, Self>) -> impl IntoElement {
        let Some(content) = self.content.clone() else {
            return div().size_full();
        };
        let letters: Vec<(f32, HotCuePad, Rgba, String)> = content
            .cues
            .hot_cues()
            .map(|(pad, cue)| {
                (
                    (cue.start_seconds() / content.duration).clamp(0.0, 1.0) as f32,
                    pad,
                    theme::hot_cue_colour(pad, cue),
                    cue.name.clone(),
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
            .children(letters.into_iter().map(|(fraction, pad, tint, name)| {
                div()
                    .id(("overview-hot-cue", pad.index()))
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
                    .when(!name.is_empty(), |letter| letter.tooltip(tooltip(name)))
                    .child(pad.letter().to_string())
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

    /// Where the playhead's line starts at `progress` through the track: kept whole inside the
    /// overview, so it shows at the very start and the very end, where the rounded corners clip.
    fn playhead_left(&self, progress: f32) -> f32 {
        let inset = PLAYHEAD_CAP;
        let centre = (progress.clamp(0.0, 1.0) * self.width).clamp(inset, self.width - inset);
        centre - PLAYHEAD_WIDTH / 2.0
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
    for (start_ms, end_ms) in content.cues.loops() {
        let left = area.x_of(seconds(start_ms));
        let width = (area.x_of(seconds(end_ms)) - left).max(1.0);
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
    for cue in content.cues.memory_cues() {
        if cue.loop_end_ms.is_none() {
            paint_triangle(
                &area,
                area.x_of(cue.start_seconds()),
                1.0,
                theme::memory_cue_colour(cue),
                window,
            );
        }
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

fn seconds(milliseconds: u32) -> f64 {
    f64::from(milliseconds) / 1000.0
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
    cue_point: Option<f64>,
    pulse: Option<(f64, f32)>,
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
        paint_playhead(&area, area.playhead_left(progress), window);
    }
    if let Some(cue_point) = motion.cue_point {
        let x = area.x_of(cue_point);
        let origin = area.bounds.origin;
        let mut triangle = PathBuilder::fill();
        triangle.move_to(origin + point(px(x - 4.0), px(area.bands_bottom)));
        triangle.line_to(origin + point(px(x + 4.0), px(area.bands_bottom)));
        triangle.line_to(origin + point(px(x), px(area.bands_bottom - 6.0)));
        triangle.close();
        if let Ok(path) = triangle.build() {
            window.paint_path(path, theme::CUE_POINT);
        }
    }
    if let Some((seconds, strength)) = motion.pulse {
        let half_width = 2.0 + 5.0 * (1.0 - strength);
        window.paint_quad(fill(
            area.band_span(area.x_of(seconds) - half_width, half_width * 2.0),
            theme::faded(theme::TEXT, 0.5 * strength),
        ));
    }
    if let Some(hover) = motion.hover {
        window.paint_quad(fill(
            area.band_span(hover * area.width, 1.0),
            theme::faded(theme::TEXT, 0.6),
        ));
    }
}

/// The playhead: a line the overview's height with a small triangle at each end.
fn paint_playhead(area: &OverviewArea, left: f32, window: &mut Window) {
    let height = f32::from(area.bounds.size.height);
    window.paint_quad(fill(
        area.rect(left, 0.0, PLAYHEAD_WIDTH, height),
        theme::PLAYHEAD,
    ));
    let centre = left + PLAYHEAD_WIDTH / 2.0;
    let origin = area.bounds.origin;
    for (edge, tip) in [(0.0, PLAYHEAD_CAP), (height, height - PLAYHEAD_CAP)] {
        let mut cap = PathBuilder::fill();
        cap.move_to(origin + point(px(centre - PLAYHEAD_CAP), px(edge)));
        cap.line_to(origin + point(px(centre + PLAYHEAD_CAP), px(edge)));
        cap.line_to(origin + point(px(centre), px(tip)));
        cap.close();
        if let Ok(path) = cap.build() {
            window.paint_path(path, theme::PLAYHEAD);
        }
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
                    cues: Arc::clone(&side.cues),
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
        cue_point: visuals.map(|_| frame.state.cue_point_seconds),
        pulse: side
            .marker_pulse
            .and_then(|pulse| Some((pulse.seconds, pulse.strength(Instant::now())?))),
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
            .top_0()
            .left_0()
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

    #[test]
    fn the_playhead_shows_whole_at_the_start_and_the_end_and_moves_between() {
        let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(600.0), px(42.0)));
        let area = OverviewArea::new(bounds, 300.0);
        let at_start = area.playhead_left(0.0);
        let at_end = area.playhead_left(1.0);
        assert!(
            at_start - PLAYHEAD_CAP + PLAYHEAD_WIDTH / 2.0 >= 0.0,
            "{at_start}"
        );
        assert!(
            at_end + PLAYHEAD_WIDTH / 2.0 + PLAYHEAD_CAP <= 600.0,
            "{at_end}"
        );
        assert_eq!(area.playhead_left(0.5), 299.0);
        assert!(area.playhead_left(0.25) < area.playhead_left(0.5));
    }

    #[test]
    fn a_moving_playhead_keeps_the_cached_bars() {
        let content = OverviewContent {
            bars: None,
            track: Arc::new(CollectionTrack::from_file(std::path::Path::new("a.mp3"))),
            phrases: Arc::default(),
            cues: Arc::default(),
            mix_in: None,
            mix_out: None,
            duration: 300.0,
        };
        let mut overview = DeckOverview::default();
        assert!(
            overview.take(Some(content.clone())),
            "the first content is drawn"
        );
        assert!(
            !overview.take(Some(content.clone())),
            "the same track and marks are not drawn again as the playhead moves"
        );
        let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(600.0), px(42.0)));
        let area = OverviewArea::new(bounds, content.duration);
        assert_ne!(area.playhead_left(0.1), area.playhead_left(0.2));
    }
}
