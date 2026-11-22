//! The mixer between the decks: the master level and meter, each channel's trim, EQ and filter
//! knobs, a meter and a fader per channel, the crossfader, and the headphone section.

use dubplate_engine::SmartCfx;
use dubplate_engine::{DeckId, EqBand};
use gpui::prelude::*;
use gpui::{ClickEvent, Context, FontWeight, MouseButton, Pixels, SharedString, deferred, div, px};

use super::slider::{self, CROSSFADER_THUMB, FADER_THUMB, SMALL_THUMB};
use super::smart::preset_label;
use super::{DubplateApp, knob};
use crate::commands::Command;
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
const HEADPHONE_KNOB_SIZE: Pixels = px(16.0);

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
                .child(
                    slider::render(
                        app,
                        Slider::Crossfader,
                        CROSSFADER_THUMB,
                        app.smart_fader_on().then_some((0.0, theme::TEXT)),
                        cx,
                    )
                    .flex_1(),
                )
                .child(deck_label(DeckId::B)),
        )
        .child(smart_buttons(app, cx))
        .child(headphones(app, cx))
}

/// SMART CFX with its preset, which opens the picker, and SMART FADER, which also lights the
/// crossfader's track while it is on.
fn smart_buttons(app: &DubplateApp, cx: &mut Context<'_, DubplateApp>) -> impl IntoElement {
    let preset = app.smart_cfx_preset();
    div()
        .relative()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(4.0))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(theme::TINY_TEXT_SIZE)
                        .text_color(theme::MUTED)
                        .child("Smart"),
                )
                .child(cue_button(
                    ("smart-cfx", 0),
                    "CFX",
                    app.smart_cfx_on(),
                    theme::TEXT,
                    Command::ToggleSmartCfx,
                    cx,
                ))
                .child(
                    div()
                        .id("smart-cfx-preset")
                        .flex()
                        .items_center()
                        .h(px(20.0))
                        .px(px(6.0))
                        .rounded(px(5.0))
                        .text_size(theme::TINY_TEXT_SIZE)
                        .text_color(theme::MUTED)
                        .cursor_pointer()
                        .hover(|button| button.bg(theme::RAISED).text_color(theme::TEXT))
                        .when(app.smart_cfx_menu, |button| {
                            button.bg(theme::RAISED).text_color(theme::TEXT)
                        })
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.toggle_smart_cfx_menu(cx);
                        }))
                        .child(format!("{} \u{25be}", preset_label(preset))),
                ),
        )
        .child(cue_button(
            ("smart-fader", 0),
            "Fader",
            app.smart_fader_on(),
            theme::TEXT,
            Command::ToggleSmartFader,
            cx,
        ))
        .when(app.smart_cfx_menu, |row| {
            row.child(smart_cfx_menu(preset, cx))
        })
}

/// The Smart CFX presets, the current one marked.
fn smart_cfx_menu(current: SmartCfx, cx: &mut Context<'_, DubplateApp>) -> impl IntoElement {
    deferred(
        div()
            .id("smart-cfx-menu")
            .absolute()
            .left_0()
            .bottom(px(24.0))
            .w(px(140.0))
            .p(px(4.0))
            .flex()
            .flex_col()
            .gap(px(1.0))
            .rounded(px(8.0))
            .border_1()
            .border_color(theme::LINE)
            .bg(theme::PANEL)
            .shadow_lg()
            .occlude()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| this.close_smart_cfx_menu(cx)))
            .children(SmartCfx::ALL.into_iter().map(|preset| {
                let chosen = preset == current;
                div()
                    .id(SharedString::from(format!("smart-cfx-{preset}")))
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(26.0))
                    .px(px(8.0))
                    .rounded(px(5.0))
                    .text_size(theme::SMALL_TEXT_SIZE)
                    .cursor_pointer()
                    .when(chosen, |item| {
                        item.bg(theme::RAISED).font_weight(FontWeight::MEDIUM)
                    })
                    .hover(|item| item.bg(theme::RAISED))
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.perform(Command::ChooseSmartCfx(preset), window, cx);
                        this.close_smart_cfx_menu(cx);
                    }))
                    .child(preset_label(preset))
                    .when(chosen, |item| {
                        item.child(div().text_color(theme::MUTED).child("\u{2713}"))
                    })
            })),
    )
}

/// The headphone section: each channel's CUE on its side, and between them the MIXING knob,
/// MASTER CUE and the LEVEL knob.
fn headphones(app: &DubplateApp, cx: &mut Context<'_, DubplateApp>) -> impl IntoElement {
    let settings = app.headphones();
    let cued = |deck| settings.is_some_and(|settings| settings.is_cued(deck));
    let master_cued = settings.is_some_and(|settings| settings.master_cue);
    div()
        .flex()
        .items_center()
        .justify_between()
        .child(cue_button(
            ("headphone-cue", 0),
            "Cue",
            cued(DeckId::A),
            theme::deck_colour(DeckId::A),
            Command::ToggleHeadphoneCue(DeckId::A),
            cx,
        ))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(4.0))
                .child(knob::render(
                    app,
                    Control::HeadphoneMix,
                    HEADPHONE_KNOB_SIZE,
                    theme::TEXT,
                    cx,
                ))
                .child(label("Mix"))
                .child(cue_button(
                    ("master-cue", 0),
                    "M",
                    master_cued,
                    theme::TEXT,
                    Command::ToggleMasterCue,
                    cx,
                ))
                .child(knob::render(
                    app,
                    Control::HeadphoneLevel,
                    HEADPHONE_KNOB_SIZE,
                    theme::TEXT,
                    cx,
                ))
                .child(label("Lvl")),
        )
        .child(cue_button(
            ("headphone-cue", 1),
            "Cue",
            cued(DeckId::B),
            theme::deck_colour(DeckId::B),
            Command::ToggleHeadphoneCue(DeckId::B),
            cx,
        ))
}

/// A small headphone button, outlined in `colour` while on.
fn cue_button(
    id: (&'static str, usize),
    text: &'static str,
    on: bool,
    colour: gpui::Rgba,
    command: Command,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .h(px(20.0))
        .px(px(6.0))
        .rounded(px(5.0))
        .border_1()
        .text_size(theme::TINY_TEXT_SIZE)
        .cursor_pointer()
        .map(|button| {
            if on {
                button
                    .border_color(colour)
                    .bg(theme::faded(colour, 0.18))
                    .text_color(theme::TEXT)
            } else {
                button
                    .border_color(theme::LINE)
                    .text_color(theme::MUTED)
                    .hover(|button| button.text_color(theme::TEXT))
            }
        })
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            this.perform(command, window, cx);
        }))
        .child(text)
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
