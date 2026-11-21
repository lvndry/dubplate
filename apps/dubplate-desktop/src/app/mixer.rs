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

pub(super) fn render(app: &DubplateApp, cx: &mut Context<'_, DubplateApp>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .w(MIXER_WIDTH)
        .gap(px(10.0))
        .px(theme::GUTTER)
        .py(px(12.0))
        .bg(theme::BACKGROUND)
        .border_l_1()
        .border_r_1()
        .border_color(theme::LINE)
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(label("Master"))
                        .child(meter(&app.master_meter, MeterDirection::Across)),
                )
                .child(slider::render(app, Slider::MasterLevel, SMALL_THUMB, None, cx).w_full()),
        )
        .child(knobs(app, cx))
        .child(
            div()
                .flex()
                .flex_1()
                .justify_center()
                .gap(px(30.0))
                .min_h(px(70.0))
                .child(channel(app, DeckId::A, cx))
                .child(channel(app, DeckId::B, cx)),
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

/// A row of the knob grid: its name, and the knob it is on each deck.
type KnobRow = (&'static str, fn(DeckId) -> Control);

/// Trim, the three EQ bands and the filter, deck A on the left and deck B on the right.
fn knobs(app: &DubplateApp, cx: &mut Context<'_, DubplateApp>) -> impl IntoElement {
    let rows: [KnobRow; 5] = [
        ("Trim", Control::Trim),
        ("High", |deck| Control::Eq(deck, EqBand::High)),
        ("Mid", |deck| Control::Eq(deck, EqBand::Mid)),
        ("Low", |deck| Control::Eq(deck, EqBand::Low)),
        ("Filter", Control::Filter),
    ];
    let mut grid = div().flex().flex_col().gap(px(6.0));
    for (name, control) in rows {
        let mut row = div().flex().justify_between();
        for deck in DeckId::ALL {
            row = row.child(div().w(px(88.0)).child(knob::render(
                app,
                control(deck),
                format!("{name} {}", deck_letter(deck)).into(),
                theme::deck_colour(deck),
                cx,
            )));
        }
        grid = grid.child(row);
    }
    grid
}

fn channel(app: &DubplateApp, deck: DeckId, cx: &mut Context<'_, DubplateApp>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(6.0))
        .child(
            div()
                .flex()
                .flex_1()
                .gap(px(6.0))
                .child(meter(&app.deck_meters[deck.index()], MeterDirection::Up))
                .child(
                    slider::render(app, Slider::ChannelFader(deck), FADER_THUMB, None, cx).h_full(),
                ),
        )
        .child(deck_label(deck))
}

/// Which way a meter's segments run from quiet to loud.
enum MeterDirection {
    Up,
    Across,
}

fn meter(meter: &PeakMeter, direction: MeterDirection) -> impl IntoElement {
    let lit = meter.lit_segments();
    let segments = (0..SEGMENTS).map(move |segment| {
        let colour = if segment >= lit {
            theme::METER_OFF
        } else if segment >= HOT_FROM {
            theme::METER_HOT
        } else {
            theme::METER_ON
        };
        div().size(SEGMENT_SIZE).rounded(px(1.0)).bg(colour)
    });
    let column = div().flex().gap(SEGMENT_GAP);
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
