//! The two zoomed waveforms, stacked, scrolling past one playhead in the middle, with the beat
//! grid behind them, the hot cues above and the memory cues as red triangles along the top.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dubplate_engine::{BeatLength, DeckId};
use dubplate_rekordbox::{HotCue, HotCuePad, TrackCues, cue_milliseconds};
use gpui::prelude::*;
use gpui::{
    Bounds, Context, FontWeight, MouseButton, MouseDownEvent, PathBuilder, PinchEvent, Pixels,
    Point, Rgba, SharedString, Window, canvas, div, fill, point, px, relative, size,
};

use super::drop_target::accept_drops;
use super::{DeckFrame, DubplateApp};
use crate::deck::DeckContent;
use crate::deck::deck_letter;
use crate::fonts::MONO_FAMILY;
use crate::frame_stats::FrameStats;
use crate::shortcuts::printed_keys;
use crate::theme;
use crate::waveform::{
    BandLevels, BeatMarks, BeatPosition, LaneLoop, LaneView, LoopLook, Zoom, ZoomedWaveform,
};

const LANE_HEIGHT: Pixels = px(74.0);
/// The lanes' height in a short window, so the library keeps some rows.
const SHORT_LANE_HEIGHT: Pixels = px(50.0);
/// The phase meter's track, in pixels: half of it is half a beat early or late.
const PHASE_TRACK_WIDTH: f32 = 44.0;
/// How close two decks' beats are, in beats, for the phase meter to call them in phase: about
/// 8 ms at 120 BPM, under what an ear hears as a flam.
const IN_PHASE_BEATS: f64 = 0.016;
const PLAYHEAD_WIDTH: Pixels = px(2.0);
/// How long the centre line brightens when the decks' beats lock: about a beat.
pub(super) const PHASE_LOCK_GLOW: Duration = Duration::from_millis(470);
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

/// How many rows of pixels a memory cue's triangle is tall; each row is two pixels narrower.
const MEMORY_CUE_ROWS: u8 = 5;

/// What a deck lane draws over its waveform besides the beats: the loop, the memory cues and
/// the slip shadow.
#[derive(Debug, Clone)]
pub(super) struct LaneMarks {
    pub slicer: Option<dubplate_engine::SlicerState>,
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
    /// The track's cues, for its memory cues. Shared with the deck, so a frame never copies
    /// them.
    pub cues: Option<Arc<TrackCues>>,
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
    let readings = beat_readings(app, frames);
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
        .id("lanes")
        .on_pinch(cx.listener(|this, event: &PinchEvent, _, cx| {
            this.pinch_lanes(event.delta, cx);
        }))
        .child(lane(app, DeckId::A, &frames[0], readings[0], cx))
        .child(lane(app, DeckId::B, &frames[1], readings[1], cx))
        .child(zoom_label(app.lane_zoom))
        .child(centre_line(theme::active().playhead))
        .children(
            app.phase_lock_at
                .map(|at| lock_glow(at.elapsed()))
                .filter(|glow| *glow > 0.0)
                .map(|glow| centre_line(theme::faded(theme::active().ok, glow))),
        )
}

fn centre_line(colour: Rgba) -> impl IntoElement {
    div()
        .absolute()
        .top(px(4.0))
        .bottom(px(4.0))
        .left(relative(0.5))
        .ml(-PLAYHEAD_WIDTH / 2.0)
        .w(PLAYHEAD_WIDTH)
        .bg(colour)
}

/// How strongly the centre line shows the lock `since` the beats locked: green at once, fading
/// back to the playhead's colour over [`PHASE_LOCK_GLOW`].
fn lock_glow(since: Duration) -> f32 {
    (1.0 - since.as_secs_f32() / PHASE_LOCK_GLOW.as_secs_f32()).max(0.0)
}

/// How far the follower's beats are from the master's, in beats, while both decks play: what
/// the phase meter reads.
pub(super) fn phase_now(app: &DubplateApp, frames: &[DeckFrame; 2]) -> Option<f64> {
    if !frames.iter().all(|frame| frame.state.playing) {
        return None;
    }
    beat_readings(app, frames)
        .into_iter()
        .flatten()
        .find_map(|reading| reading.phase)
        .map(|phase| phase.beats)
}

/// Whether the decks count as locked after this frame's `phase`, given whether they were: they
/// lock within [`IN_PHASE_BEATS`] and let go only past twice that, so beats hovering at the
/// edge do not flash the centre line again and again.
pub(super) fn phase_locked(was_locked: bool, phase: Option<f64>) -> bool {
    let limit = if was_locked {
        2.0 * IN_PHASE_BEATS
    } else {
        IN_PHASE_BEATS
    };
    phase.is_some_and(|beats| beats.abs() <= limit)
}

/// The lanes' zoom at their top right, one dot per zoom level, lit up to the current one, so
/// the DJ sees how far in the lanes are zoomed, as rekordbox's enlarged waveform does.
fn zoom_label(zoom: Zoom) -> impl IntoElement {
    let current = Zoom::LEVELS
        .iter()
        .position(|level| *level == zoom)
        .unwrap_or_default();
    div()
        .id("lane-zoom")
        .role(gpui::Role::Meter)
        .aria_label(format!(
            "Waveform zoom {} of {}",
            current + 1,
            Zoom::LEVELS.len()
        ))
        .absolute()
        .top(px(11.0))
        .right(theme::GUTTER + px(6.0))
        .flex()
        .gap(px(2.0))
        .children((0..Zoom::LEVELS.len()).map(move |level| {
            div().size(px(3.0)).rounded_full().bg(if level <= current {
                theme::active().muted
            } else {
                theme::active().meter_off
            })
        }))
}

/// What the beat counter and the phase meter read for one deck in this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
struct BeatReading {
    /// Where the deck is in its bars.
    position: BeatPosition,
    /// How far its beats are from the deck it follows, when it has one to follow.
    phase: Option<Phase>,
}

/// How a deck's beats sit against the deck it is mixed against.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Phase {
    /// In beats, from -0.5 to 0.5, positive when early.
    beats: f64,
    /// Whole beats between the downbeats, from -2 to 2.
    bar_offset: i32,
}

/// The beat counters of both decks, and a phase meter on the deck that is not the tempo
/// master (deck B when neither is), measured against the other deck, as the CDJ-3000 and
/// rekordbox show over their waveforms.
fn beat_readings(app: &DubplateApp, frames: &[DeckFrame; 2]) -> [Option<BeatReading>; 2] {
    let positions = DeckId::ALL.map(|deck| {
        app.deck(deck).visuals().and_then(|visuals| {
            visuals
                .beats
                .position(frames[deck.index()].playhead_seconds)
        })
    });
    let master = DeckId::ALL
        .into_iter()
        .find(|deck| app.tempo_state(*deck).is_some_and(|state| state.is_master))
        .unwrap_or(DeckId::A);
    DeckId::ALL.map(|deck| {
        let position = positions[deck.index()]?;
        let against = positions[master.index()].filter(|_| deck != master);
        Some(BeatReading {
            position,
            phase: against.map(|master_position| Phase {
                beats: position.phase_offset(master_position),
                bar_offset: position.bar_offset(master_position),
            }),
        })
    })
}

/// The beat counter, `17.3` for bar 17 beat 3, and the phase meter beside it: a short track
/// with a centre line and a mark that sits on it when the beats land together, green then,
/// with the beats between the downbeats when the bars do not line up.
fn beat_counter(reading: BeatReading) -> impl IntoElement {
    let in_phase = reading
        .phase
        .is_some_and(|phase| phase.beats.abs() <= IN_PHASE_BEATS);
    div()
        .absolute()
        .top(px(2.0))
        .left(px(4.0))
        .flex()
        .items_center()
        .gap(px(6.0))
        .px(px(4.0))
        .py(px(1.0))
        .rounded(px(3.0))
        .bg(theme::faded(theme::active().background, 0.7))
        .font_family(MONO_FAMILY)
        .text_size(px(10.0))
        .text_color(theme::active().muted)
        .child(SharedString::from(format!(
            "{}.{}",
            reading.position.bar, reading.position.beat
        )))
        .children(reading.phase.map(|phase| {
            let colour = if in_phase {
                theme::active().ok
            } else {
                theme::active().attention
            };
            let offset = (phase.beats as f32 * 2.0).clamp(-1.0, 1.0) * PHASE_TRACK_WIDTH / 2.0;
            div()
                .flex()
                .items_center()
                .gap(px(4.0))
                .child(
                    div()
                        .relative()
                        .w(px(PHASE_TRACK_WIDTH))
                        .h(px(6.0))
                        .rounded(px(3.0))
                        .bg(theme::active().track)
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .bottom_0()
                                .left(px(PHASE_TRACK_WIDTH / 2.0))
                                .w(px(1.0))
                                .bg(theme::active().muted),
                        )
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .size(px(6.0))
                                .rounded_full()
                                .left(px(PHASE_TRACK_WIDTH / 2.0 - 3.0 + offset))
                                .bg(colour),
                        ),
                )
                .when(phase.bar_offset != 0, |meter| {
                    meter.child(div().text_color(theme::active().attention).child(
                        SharedString::from(format!(
                            "{:+} beat{}",
                            phase.bar_offset,
                            if phase.bar_offset.abs() == 1 { "" } else { "s" }
                        )),
                    ))
                })
        }))
}

/// What a pad press just did, just right of the playhead at the top of the lane: the jump, the
/// loop length or the effect held, in the deck's colour.
fn pad_note(text: SharedString, colour: Rgba) -> impl IntoElement {
    div()
        .absolute()
        .top(px(2.0))
        .left(relative(0.5))
        .ml(px(8.0))
        .px(px(5.0))
        .py(px(1.0))
        .rounded(px(3.0))
        .bg(theme::faded(colour, 0.22))
        .border_1()
        .border_color(theme::faded(colour, 0.6))
        .text_color(theme::active().text)
        .font_family(MONO_FAMILY)
        .text_size(px(10.0))
        .font_weight(FontWeight::SEMIBOLD)
        .child(text)
}

fn lane(
    app: &DubplateApp,
    deck: DeckId,
    frame: &DeckFrame,
    reading: Option<BeatReading>,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement {
    let content = &app.deck(deck).content;
    let visuals = app.deck(deck).visuals();
    let zoomed = visuals.and_then(|visuals| visuals.zoomed.clone());
    let beats = visuals.map(|visuals| Arc::clone(&visuals.beats));
    let playhead_seconds = frame.playhead_seconds;
    let tempo = frame.state.tempo;
    let marks = LaneMarks {
        slicer: app.slicer_state(deck).filter(|state| {
            state.enabled && crate::pads::is_slicer_mode(app.pad_modes[deck.index()])
        }),
        lane_loop: app
            .loop_state(deck)
            .and_then(|state| LaneLoop::of(&state, playhead_seconds)),
        slip_shadow: frame.slip_shadow,
        colour: theme::deck_colour(deck),
        cue_point: app
            .deck(deck)
            .visuals()
            .map(|_| frame.state.cue_point_seconds),
        pulse: app
            .deck(deck)
            .marker_pulse
            .and_then(|pulse| Some((pulse.seconds, pulse.strength(Instant::now())?))),
        cues: app
            .deck(deck)
            .visuals()
            .map(|_| Arc::clone(&app.deck(deck).cues)),
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
    let zoom = app.lane_zoom;
    let pixels_per_track_second = zoom.pixels_per_track_second(tempo);
    let lane_colour = marks.colour;
    let loop_label = marks.lane_loop.and_then(|lane_loop| {
        let length = lane_loop.length?;
        let offset = (lane_loop.start_seconds - playhead_seconds) * pixels_per_track_second;
        Some((loop_label(length), offset as f32))
    });
    let flags = match content {
        DeckContent::Loaded(_) => app
            .deck(deck)
            .cues
            .hot_cues()
            .map(|(pad, cue)| {
                let offset = (cue.start_seconds() - playhead_seconds) * pixels_per_track_second;
                (
                    flag_label(pad, cue),
                    theme::hot_cue_colour(pad, cue),
                    offset as f32,
                )
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
        .h(if app.short_window() {
            SHORT_LANE_HEIGHT
        } else {
            LANE_HEIGHT
        })
        .rounded(px(4.0))
        .overflow_hidden()
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, (), window, _| {
                    let started = Instant::now();
                    paint_lane(
                        bounds,
                        LaneContent {
                            deck,
                            zoomed: zoomed.as_deref(),
                            beats: beats.as_deref(),
                            marks: Some(&marks),
                        },
                        LaneView {
                            width: f32::from(bounds.size.width),
                            playhead_seconds,
                            zoom,
                            tempo,
                        },
                        window,
                    );
                    record_paint(stats.as_ref(), started);
                },
            )
            .absolute()
            .size_full(),
        )
        .children(flags.into_iter().map(|(label, colour, offset)| {
            div()
                .absolute()
                .top_0()
                .left(relative(0.5))
                .ml(px(offset - FLAG_WIDTH / 2.0))
                .min_w(px(FLAG_WIDTH))
                .px(px(3.0))
                .rounded(px(3.0))
                .bg(colour)
                .text_color(theme::active().on_deck)
                .font_family(MONO_FAMILY)
                .text_size(px(10.0))
                .line_height(px(14.0))
                .font_weight(FontWeight::SEMIBOLD)
                .flex()
                .justify_center()
                .whitespace_nowrap()
                .child(label)
        }))
        .children(loop_label.map(|(label, offset)| {
            div()
                .absolute()
                .bottom(px(2.0))
                .left(relative(0.5))
                .ml(px(offset + LOOP_EDGE_WIDTH + 2.0))
                .px(px(4.0))
                .rounded(px(3.0))
                .bg(lane_colour)
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
        )
        .children(reading.map(beat_counter))
        .children(
            app.pad_notes[deck.index()]
                .as_ref()
                .filter(|note| note.shows(Instant::now()))
                .map(|note| pad_note(note.text.clone(), theme::deck_colour(deck))),
        );
    accept_drops(lane, deck, cx)
}

fn record_paint(stats: Option<&Rc<RefCell<FrameStats>>>, started: Instant) {
    if let Some(stats) = stats {
        stats.borrow_mut().record_lane_paint(started.elapsed());
    }
}

/// What a lane draws: the waveform, the beat grid and the marks over them.
pub(super) struct LaneContent<'content> {
    pub deck: DeckId,
    pub zoomed: Option<&'content ZoomedWaveform>,
    pub beats: Option<&'content BeatMarks>,
    pub marks: Option<&'content LaneMarks>,
}

pub(super) fn paint_lane(
    bounds: Bounds<Pixels>,
    content: LaneContent<'_>,
    view: LaneView,
    window: &mut Window,
) {
    let LaneContent {
        deck,
        zoomed,
        beats,
        marks,
    } = content;
    if let Some(marks) = marks
        && let Some(lane_loop) = marks.lane_loop
    {
        paint_loop_band(bounds, &view, lane_loop, marks.colour, window);
    }
    if let Some(zoomed) = zoomed {
        paint_waveform(bounds, zoomed, &view, deck, window);
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
        if let Some(s) = &marks.slicer
            && let (Some(start), Some(end)) = (s.domain_start_seconds, s.domain_end_seconds)
        {
            let count = dubplate_engine::SlicerSlice::COUNT;
            let step = (end - start) / f64::from(count);
            for index in 0..count {
                let slice = LaneLoop {
                    start_seconds: start + step * f64::from(index),
                    end_seconds: start + step * f64::from(index + 1),
                    look: LoopLook::Active,
                    length: None,
                };
                if s.slice.is_some_and(|slice| slice.index() == index) {
                    paint_loop_band(bounds, &view, slice, marks.colour, window);
                }
                paint_loop_edges(
                    bounds,
                    &view,
                    slice,
                    theme::faded(marks.colour, 0.5),
                    window,
                );
            }
        }
        if let Some(cues) = &marks.cues {
            for (x, colour) in memory_cue_marks(cues, &view) {
                paint_memory_cue(bounds, x, colour, window);
            }
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

/// What a hot cue's flag reads: its pad's letter, and its name when it has one.
fn flag_label(pad: HotCuePad, cue: &HotCue) -> SharedString {
    if cue.name.is_empty() {
        SharedString::new_static(PAD_LETTERS[pad.index()])
    } else {
        format!("{} {}", pad.letter(), cue.name).into()
    }
}

/// The pad letters as text, so a flag without a name never allocates.
const PAD_LETTERS: [&str; HotCuePad::ALL.len()] = ["A", "B", "C", "D", "E", "F", "G", "H"];

/// Where each memory cue on screen goes across the lane, and its colour: its own, else red for
/// a memory cue and the saved loop colour for a memory loop.
fn memory_cue_marks<'a>(
    cues: &'a TrackCues,
    view: &'a LaneView,
) -> impl Iterator<Item = (f32, Rgba)> + 'a {
    let on_screen = view.seconds_on_screen();
    cues.memory_cues_between(
        cue_milliseconds(on_screen.start),
        cue_milliseconds(on_screen.end),
    )
    .iter()
    .map(|cue| {
        (
            view.x_of(cue.start_seconds()),
            theme::memory_cue_colour(cue),
        )
    })
}

/// A memory cue: a small triangle hanging from the lane's top edge, pointing down at `x`, made
/// of one quad per row so painting it never allocates.
fn paint_memory_cue(bounds: Bounds<Pixels>, x: f32, colour: Rgba, window: &mut Window) {
    for row in 0..MEMORY_CUE_ROWS {
        let half_width = f32::from(MEMORY_CUE_ROWS - row) - 0.5;
        let line = Bounds::new(
            bounds.origin + point(px(x - half_width), px(f32::from(row))),
            size(px(half_width * 2.0), px(1.0)),
        );
        window.paint_quad(fill(line, colour));
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
    deck: DeckId,
    window: &mut Window,
) {
    let width = f32::from(bounds.size.width);
    let height = f32::from(bounds.size.height);
    let theme = theme::active();
    let unplayed = theme.waveform_bands(deck);
    let played = unplayed.at_opacity(theme.played_opacity);
    let centre_x = width / 2.0;
    let area = BandArea {
        origin: bounds.origin,
        centre_y: height / 2.0,
        tallest: height / 2.0 - 2.0,
    };
    let columns = zoomed.columns();
    let visible = view.visible_columns_of(columns.len(), zoomed.zoom());
    if visible.column_width >= 1.0 {
        for (offset, levels) in columns[visible.range].iter().enumerate() {
            let x = visible.first_x + offset as f32 * visible.column_width;
            let colours = if x + visible.column_width <= centre_x {
                &played
            } else {
                &unplayed
            };
            area.paint_column(x, visible.column_width, *levels, colours, window);
        }
        return;
    }
    // Zoomed out past the waveform's own resolution, each pixel shows the loudest of the
    // columns under it, so peaks stay visible and the quads painted stay one per pixel.
    let columns_per_pixel = 1.0 / visible.column_width;
    let first_column = visible.range.start as f32 - visible.first_x * columns_per_pixel;
    for pixel in 0..width.ceil() as usize {
        let start = first_column + pixel as f32 * columns_per_pixel;
        let from = start.max(0.0) as usize;
        let to = ((start + columns_per_pixel).ceil().max(0.0) as usize).min(columns.len());
        let Some(levels) = columns
            .get(from..to)
            .and_then(|under| under.iter().copied().reduce(BandLevels::max))
        else {
            continue;
        };
        let x = pixel as f32;
        let colours = if x + 1.0 <= centre_x {
            &played
        } else {
            &unplayed
        };
        area.paint_column(x, 1.0, levels, colours, window);
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
        colours: &theme::BandColours,
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

#[cfg(test)]
mod tests {
    use dubplate_rekordbox::{MemoryCue, MemoryCueColor, PaletteColor};

    use super::*;

    #[test]
    fn the_beats_lock_once_and_let_go_only_well_past_the_edge() {
        assert!(!phase_locked(false, None));
        assert!(!phase_locked(false, Some(0.02)));
        assert!(phase_locked(false, Some(-0.01)));
        assert!(
            phase_locked(true, Some(0.02)),
            "hovering at the edge stays locked"
        );
        assert!(!phase_locked(true, Some(0.04)));
        assert!(!phase_locked(true, None), "a deck stopping lets go");
    }

    #[test]
    fn the_lock_glow_fades_over_about_a_beat() {
        assert_eq!(lock_glow(Duration::ZERO), 1.0);
        assert!(lock_glow(Duration::from_millis(235)) > 0.49);
        assert_eq!(lock_glow(PHASE_LOCK_GLOW), 0.0);
    }

    #[test]
    fn only_the_memory_cues_on_screen_are_drawn_where_they_fall() {
        let cues = TrackCues::new(
            Default::default(),
            vec![
                MemoryCue::at(10_000),
                MemoryCue::at(31_000),
                MemoryCue {
                    loop_end_ms: Some(34_000),
                    ..MemoryCue::at(33_000)
                },
                MemoryCue::at(90_000),
            ],
        );
        let view = LaneView {
            width: 1_000.0,
            playhead_seconds: 30.0,
            zoom: Zoom::new(100.0),
            tempo: 1.0,
        };
        let marks: Vec<(f32, Rgba)> = memory_cue_marks(&cues, &view).collect();
        assert_eq!(
            marks,
            [
                (600.0, theme::active().memory_cue),
                (800.0, theme::active().saved_loop)
            ],
            "five seconds either side of the playhead fit; a memory loop takes the loop colour"
        );
        let mut coloured = cues.clone();
        coloured.insert_memory_cue(MemoryCue {
            color: Some(MemoryCueColor::Blue),
            ..MemoryCue::at(29_000)
        });
        let blue = memory_cue_marks(&coloured, &view).next().unwrap();
        assert_eq!(
            blue,
            (400.0, theme::rekordbox_colour(MemoryCueColor::Blue.rgb()))
        );
    }

    #[test]
    fn a_flag_reads_its_letter_and_its_name_in_the_cue_s_colour() {
        let plain = HotCue::at(1_000);
        assert_eq!(flag_label(HotCuePad::C, &plain).as_ref(), "C");
        let named = HotCue {
            name: "Vocal".to_owned(),
            color: PaletteColor::from_index(42),
            ..HotCue::at(1_000)
        };
        assert_eq!(flag_label(HotCuePad::C, &named).as_ref(), "C Vocal");
        assert_eq!(
            theme::hot_cue_colour(HotCuePad::C, &named),
            theme::rekordbox_colour(PaletteColor::from_index(42).unwrap().rgb())
        );
        assert_eq!(
            theme::hot_cue_colour(HotCuePad::A, &plain),
            theme::rekordbox_colour(PaletteColor::pad_default(HotCuePad::A).rgb()),
            "a cue set in Dubplate takes rekordbox's default for its pad"
        );
    }
}
