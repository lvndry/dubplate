//! The mixer between the decks: the master level and meter, each channel's trim, EQ and filter
//! knobs, a meter and a fader per channel, and the crossfader.

use dubplate_engine::{DeckId, EqBand};
use gpui::prelude::*;
use gpui::{Context, FontWeight, Pixels, div, px};

use super::slider::{self, CROSSFADER_THUMB, FADER_THUMB, SMALL_THUMB};
use super::{DubplateApp, knob};
use crate::controls::{Control, Slider};
use crate::deck::deck_letter;
use crate::fonts::MONO_FAMILY;
use crate::meter::{HOT_FROM, PeakMeter, SEGMENTS};
use crate::theme;

const MIXER_WIDTH: Pixels = px(216.0);
const SEGMENT_SIZE: Pixels = px(4.0);
const SEGMENT_GAP: Pixels = px(2.0);
/// The master meter's segments in the header, smaller so the master slider keeps its length.
const HEADER_SEGMENT_SIZE: Pixels = px(3.0);
const HEADER_SEGMENT_GAP: Pixels = px(1.5);
const KNOB_SIZE: Pixels = px(18.0);
const KNOB_GAP: Pixels = px(5.0);
const KNOB_ROWS: usize = 5;

/// A knob of a channel strip: its name, and the control it turns on a deck.
type StripKnob = (&'static str, fn(DeckId) -> Control);

/// The knobs of a channel strip, top to bottom.
const STRIP: [StripKnob; KNOB_ROWS] = [
    ("Trim", Control::Trim),
    ("High", high),
    ("Mid", mid),
    ("Low", low),
    ("Filter", Control::Filter),
];

fn high(deck: DeckId) -> Control {
    Control::Eq(deck, EqBand::High)
}

fn mid(deck: DeckId) -> Control {
    Control::Eq(deck, EqBand::Mid)
}

fn low(deck: DeckId) -> Control {
    Control::Eq(deck, EqBand::Low)
}

/// The mixer, as tall as the decks beside it, laid out like rekordbox's two-deck view: the
/// master in the header; each channel's knobs in a column on its outer side with the names
/// shared between the two; each channel's fader and meter beside its knobs; the crossfader
/// right under the faders.
pub(super) fn render(app: &DubplateApp, cx: &mut Context<'_, DubplateApp>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .w(MIXER_WIDTH)
        .gap(px(8.0))
        .px(theme::GUTTER)
        .py(px(12.0))
        .bg(theme::BACKGROUND)
        .border_l_1()
        .border_r_1()
        .border_color(theme::LINE)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(label("Master"))
                .child(slider::render(app, Slider::MasterLevel, SMALL_THUMB, None, cx).flex_1())
                .child(meter(&app.master_meter, MeterDirection::Across)),
        )
        .child(
            div()
                .flex()
                .justify_between()
                .child(knob_column(app, DeckId::A, cx))
                .child(channel(app, DeckId::A, cx))
                .child(names())
                .child(channel(app, DeckId::B, cx))
                .child(knob_column(app, DeckId::B, cx)),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(deck_label(DeckId::A))
                .child(slider::render(app, Slider::Crossfader, CROSSFADER_THUMB, None, cx).flex_1())
                .child(deck_label(DeckId::B)),
        )
}

/// A channel's trim, EQ and filter knobs, top to bottom.
fn knob_column(
    app: &DubplateApp,
    deck: DeckId,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<> {
    let mut column = div().flex().flex_col().gap(KNOB_GAP);
    for (_, control) in STRIP {
        column = column.child(knob::render(
            app,
            control(deck),
            KNOB_SIZE,
            theme::deck_colour(deck),
            cx,
        ));
    }
    column
}

/// The knobs' names, between the two channels, each beside its row.
fn names() -> impl IntoElement {
    let mut column = div().flex().flex_col().items_center().gap(KNOB_GAP);
    for (name, _) in STRIP {
        column = column.child(
            div()
                .h(KNOB_SIZE)
                .flex()
                .items_center()
                .text_size(theme::TINY_TEXT_SIZE)
                .text_color(theme::MUTED)
                .child(name),
        );
    }
    column
}

/// The height of the knob column, which the faders match.
fn strip_height() -> Pixels {
    KNOB_SIZE * KNOB_ROWS as f32 + KNOB_GAP * (KNOB_ROWS - 1) as f32
}

fn channel(app: &DubplateApp, deck: DeckId, cx: &mut Context<'_, DubplateApp>) -> impl IntoElement {
    div()
        .flex()
        .gap(px(5.0))
        .h(strip_height())
        .child(meter(&app.deck_meters[deck.index()], MeterDirection::Up))
        .child(slider::render(app, Slider::ChannelFader(deck), FADER_THUMB, None, cx).h_full())
}

/// Which way a meter's segments run from quiet to loud.
enum MeterDirection {
    Up,
    Across,
}

fn meter(meter: &PeakMeter, direction: MeterDirection) -> impl IntoElement {
    let lit = meter.lit_segments();
    let (size, gap) = match direction {
        MeterDirection::Up => (SEGMENT_SIZE, SEGMENT_GAP),
        MeterDirection::Across => (HEADER_SEGMENT_SIZE, HEADER_SEGMENT_GAP),
    };
    let segments = (0..SEGMENTS).map(move |segment| {
        let colour = if segment >= lit {
            theme::METER_OFF
        } else if segment >= HOT_FROM {
            theme::METER_HOT
        } else {
            theme::METER_ON
        };
        div().size(size).rounded(px(1.0)).bg(colour)
    });
    let column = div().flex().gap(gap);
    match direction {
        MeterDirection::Up => column.flex_col_reverse().justify_start(),
        MeterDirection::Across => column.flex_row(),
    }
    .children(segments)
}

fn label(text: &'static str) -> impl IntoElement {
    div()
        .text_size(theme::TINY_TEXT_SIZE)
        .text_color(theme::MUTED)
        .child(text)
}

fn deck_label(deck: DeckId) -> impl IntoElement {
    div()
        .font_family(MONO_FAMILY)
        .text_size(theme::TINY_TEXT_SIZE)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::deck_colour(deck))
        .child(deck_letter(deck))
}
