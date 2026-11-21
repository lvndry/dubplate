//! A deck: what is loaded, the whole-track waveform, the transport and tempo, and the hot cue
//! pads.

use std::rc::Rc;

use dubplate_engine::DeckId;
use dubplate_rekordbox::HotCuePad;
use gpui::prelude::*;
use gpui::{
    ClickEvent, Context, FontWeight, MouseButton, MouseDownEvent, Pixels, SharedString, canvas,
    div, fill, point, px, size, svg,
};

use super::drop_target::accept_drops;
use super::slider::{self, SMALL_THUMB};
use super::{DeckFrame, DubplateApp, HitAreas};
use crate::assets::{PAUSE_ICON_PATH, PLAY_ICON_PATH};
use crate::collection::{CollectionTrack, TrackOrigin, clock};
use crate::controls::{Orientation, Slider, fraction_at, tempo_fraction};
use crate::deck::DeckContent;
use crate::deck::deck_letter;
use crate::fonts::MONO_FAMILY;
use crate::shortcuts::Command;
use crate::theme;

const OVERVIEW_HEIGHT: Pixels = px(22.0);
const OVERVIEW_BAR_SPACING: f32 = 3.0;
const OVERVIEW_BAR_WIDTH: f32 = 2.0;
const BUTTON_HEIGHT: Pixels = px(34.0);
const PAD_HEIGHT: Pixels = px(38.0);
const ALL_PADS: [HotCuePad; 8] = [
    HotCuePad::A,
    HotCuePad::B,
    HotCuePad::C,
    HotCuePad::D,
    HotCuePad::E,
    HotCuePad::F,
    HotCuePad::G,
    HotCuePad::H,
];

pub(super) fn render(
    app: &DubplateApp,
    deck: DeckId,
    frame: &DeckFrame,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement {
    let entry = app.deck(deck).track.as_deref();
    let panel = div()
        .id(("deck", deck.index()))
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .gap(px(10.0))
        .px(theme::GUTTER)
        .py(px(12.0))
        .bg(theme::PANEL)
        .child(title_row(app, deck, frame, entry))
        .child(overview(app, deck, frame, entry, cx))
        .child(transport(app, deck, frame, cx))
        .child(pads(deck, entry, cx));
    accept_drops(panel, deck, cx)
}

fn title_row(
    app: &DubplateApp,
    deck: DeckId,
    frame: &DeckFrame,
    entry: Option<&CollectionTrack>,
) -> impl IntoElement {
    let notice = app.deck_notice(deck);
    let (title, subtitle, subtitle_colour): (SharedString, SharedString, _) =
        match (&app.deck(deck).content, entry) {
            (_, entry) if notice.is_some() => (
                entry.map_or_else(|| "No track".into(), |entry| entry.row.title.clone()),
                notice.cloned().unwrap_or_default(),
                theme::ALERT,
            ),
            (DeckContent::Failed(message), Some(entry)) => (
                entry.row.title.clone(),
                message.clone().into(),
                theme::ALERT,
            ),
            (DeckContent::Loading, Some(entry)) => (
                entry.row.title.clone(),
                "Loading\u{2026}".into(),
                theme::MUTED,
            ),
            (_, Some(entry)) if entry.origin == TrackOrigin::File => (
                entry.row.title.clone(),
                "Audio file, not in rekordbox".into(),
                theme::MUTED,
            ),
            (_, Some(entry)) => (
                entry.row.title.clone(),
                entry.row.artist.clone(),
                theme::MUTED,
            ),
            (_, None) => ("No track".into(), "".into(), theme::MUTED),
        };
    let tempo = frame.state.tempo;
    let bpm = entry
        .and_then(|entry| entry.track.bpm)
        .map_or_else(|| "--.--".to_owned(), |bpm| format!("{:.2}", bpm * tempo));
    let pitch = format!("{:+.1}%", (tempo - 1.0) * 100.0).replace('-', "\u{2212}");
    let remaining = frame.duration_seconds.map_or_else(
        || "\u{2212}-:--".to_owned(),
        |duration| {
            format!(
                "\u{2212}{}",
                clock((duration - frame.playhead_seconds).max(0.0))
            )
        },
    );
    let key = entry.map(|entry| entry.row.key.clone()).unwrap_or_default();
    div()
        .flex()
        .justify_between()
        .items_start()
        .gap(px(12.0))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(10.0))
                .min_w_0()
                .child(deck_badge(deck))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .min_w_0()
                        .child(
                            div()
                                .text_size(px(14.0))
                                .font_weight(FontWeight::MEDIUM)
                                .truncate()
                                .child(title),
                        )
                        .child(div().truncate().text_color(subtitle_colour).child(subtitle)),
                ),
        )
        .child(
            div()
                .flex()
                .flex_none()
                .items_baseline()
                .gap(px(12.0))
                .font_family(MONO_FAMILY)
                .text_size(theme::SMALL_TEXT_SIZE)
                .text_color(theme::MUTED)
                .child(key)
                .child(pitch)
                .child(remaining)
                .child(
                    div()
                        .font_family(crate::fonts::UI_FAMILY)
                        .text_size(px(26.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme::TEXT)
                        .child(bpm),
                ),
        )
}

fn deck_badge(deck: DeckId) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(24.0))
        .rounded(theme::SMALL_RADIUS)
        .bg(theme::deck_colour(deck))
        .text_color(theme::ON_DECK)
        .font_family(MONO_FAMILY)
        .text_size(theme::SMALL_TEXT_SIZE)
        .font_weight(FontWeight::SEMIBOLD)
        .child(deck_letter(deck))
}

fn overview(
    app: &DubplateApp,
    deck: DeckId,
    frame: &DeckFrame,
    entry: Option<&CollectionTrack>,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement {
    let bars = app
        .deck(deck)
        .visuals()
        .and_then(|visuals| visuals.overview.clone());
    let progress = frame
        .duration_seconds
        .filter(|duration| *duration > 0.0)
        .map(|duration| (frame.playhead_seconds / duration).clamp(0.0, 1.0) as f32);
    let cue_marks: Vec<(f32, usize)> = match (entry, frame.duration_seconds) {
        (Some(entry), Some(duration)) if duration > 0.0 => entry
            .hot_cues
            .iter()
            .map(|cue| {
                let pad_index = ALL_PADS.iter().position(|pad| *pad == cue.pad).unwrap_or(0);
                ((cue.seconds / duration).clamp(0.0, 1.0) as f32, pad_index)
            })
            .collect(),
        _ => Vec::new(),
    };
    let colour = theme::deck_colour(deck);
    let hit_areas: Rc<HitAreas> = Rc::clone(&app.hit_areas);
    div()
        .id(("overview", deck.index()))
        .relative()
        .h(OVERVIEW_HEIGHT)
        .px(px(3.0))
        .rounded(px(4.0))
        .bg(theme::RAISED)
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
        .child(
            canvas(
                move |bounds, _, _| hit_areas.overviews[deck.index()].set(Some(bounds)),
                move |bounds, (), window, _| {
                    let width = f32::from(bounds.size.width);
                    let height = f32::from(bounds.size.height);
                    if let Some(bars) = &bars {
                        let bar_count = (width / OVERVIEW_BAR_SPACING).floor().max(0.0) as usize;
                        let mut bars = bars.borrow_mut();
                        for (bar_index, level) in bars.bars(bar_count).iter().enumerate() {
                            let left = bar_index as f32 * OVERVIEW_BAR_SPACING;
                            let played = progress.is_some_and(|progress| {
                                (left + OVERVIEW_BAR_WIDTH) / width <= progress
                            });
                            let bar_height = (level * (height - 6.0)).max(1.0);
                            let bar = gpui::Bounds::new(
                                bounds.origin + point(px(left), px((height - bar_height) / 2.0)),
                                size(px(OVERVIEW_BAR_WIDTH), px(bar_height)),
                            );
                            let opacity = if played { 0.3 } else { 0.9 };
                            window.paint_quad(fill(bar, theme::faded(colour, opacity)));
                        }
                    }
                    for (fraction, pad_index) in &cue_marks {
                        let mark = gpui::Bounds::new(
                            bounds.origin + point(px(fraction * width - 1.0), px(0.0)),
                            size(px(2.0), px(5.0)),
                        );
                        window.paint_quad(fill(mark, theme::HOT_CUE_TINTS[*pad_index]));
                    }
                    if let Some(progress) = progress {
                        let head = gpui::Bounds::new(
                            bounds.origin + point(px(progress * width - 1.0), px(0.0)),
                            size(px(2.0), bounds.size.height),
                        );
                        window.paint_quad(fill(head, theme::TEXT));
                    }
                },
            )
            .size_full(),
        )
}

fn transport(
    app: &DubplateApp,
    deck: DeckId,
    frame: &DeckFrame,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement {
    let playing = frame.state.playing;
    let colour = theme::deck_colour(deck);
    div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(
            button(("cue", deck.index()))
                .px(px(12.0))
                .bg(theme::RAISED)
                .border_1()
                .border_color(theme::LINE)
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.perform(Command::Cue(deck), window, cx);
                }))
                .child("Cue"),
        )
        .child(
            button(("play", deck.index()))
                .w(px(44.0))
                .when(playing, |play| play.bg(colour).text_color(theme::ON_DECK))
                .when(!playing, |play| {
                    play.bg(theme::RAISED).border_1().border_color(theme::LINE)
                })
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.perform(Command::TogglePlay(deck), window, cx);
                }))
                .child(
                    svg()
                        .path(if playing {
                            PAUSE_ICON_PATH
                        } else {
                            PLAY_ICON_PATH
                        })
                        .size(px(13.0))
                        .text_color(if playing { theme::ON_DECK } else { theme::TEXT }),
                ),
        )
        .child(
            div()
                .flex()
                .flex_1()
                .items_center()
                .gap(px(10.0))
                .ml(px(10.0))
                .child(
                    div()
                        .text_size(theme::SMALL_TEXT_SIZE)
                        .text_color(theme::MUTED)
                        .child("Tempo"),
                )
                .child(
                    slider::render(
                        app,
                        Slider::Tempo(deck),
                        SMALL_THUMB,
                        Some((tempo_fraction(1.0), colour)),
                        cx,
                    )
                    .flex_1(),
                )
                .child(
                    div()
                        .w(px(52.0))
                        .flex()
                        .justify_end()
                        .font_family(MONO_FAMILY)
                        .text_size(theme::SMALL_TEXT_SIZE)
                        .text_color(theme::MUTED)
                        .child("\u{b1}8%"),
                ),
        )
}

fn button(id: (&'static str, usize)) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .h(BUTTON_HEIGHT)
        .rounded(theme::CONTROL_RADIUS)
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

fn pads(
    deck: DeckId,
    entry: Option<&CollectionTrack>,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement {
    div()
        .flex()
        .gap(px(4.0))
        .children(ALL_PADS.iter().enumerate().map(|(pad_index, pad)| {
            let cue = entry.and_then(|entry| entry.hot_cue(*pad));
            let pad = *pad;
            let tint = theme::HOT_CUE_TINTS[pad_index];
            div()
                .id(("pad", deck.index() * ALL_PADS.len() + pad_index))
                .flex()
                .flex_col()
                .justify_center()
                .flex_1()
                .min_w_0()
                .gap(px(1.0))
                .h(PAD_HEIGHT)
                .px(px(8.0))
                .rounded(theme::SMALL_RADIUS)
                .border_1()
                .border_color(theme::LINE)
                .text_size(theme::SMALL_TEXT_SIZE)
                .overflow_hidden()
                .map(|pad_button| match cue {
                    Some(_) => pad_button
                        .bg(theme::RAISED)
                        .cursor_pointer()
                        .hover(|pad_button| pad_button.border_color(tint))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.perform(Command::HotCue(deck, pad), window, cx);
                        })),
                    None => pad_button.border_dashed().text_color(theme::MUTED),
                })
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(5.0))
                        .child(
                            div()
                                .size(px(6.0))
                                .rounded(px(2.0))
                                .when(cue.is_some(), |dot| dot.bg(tint))
                                .when(cue.is_none(), |dot| {
                                    dot.border_1().border_color(theme::LINE)
                                }),
                        )
                        .child(pad.letter().to_string()),
                )
                .child(
                    div()
                        .font_family(MONO_FAMILY)
                        .text_size(theme::TINY_TEXT_SIZE)
                        .text_color(theme::MUTED)
                        .child(cue.map(|cue| clock(cue.seconds)).unwrap_or_default()),
                )
        }))
}
