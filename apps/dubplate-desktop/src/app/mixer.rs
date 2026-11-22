//! The mixer between the decks: the master level and meter, each channel's trim, EQ and filter
//! knobs, a meter and a fader per channel, the crossfader, and the headphone section.

use dubplate_engine::{CrossfaderAssign, DeckId, EqBand, SmartCfx};
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
use crate::widgets::InteractiveControl;

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
        .bg(theme::active().background)
        .border_l_1()
        .border_r_1()
        .border_color(theme::active().line)
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
                .justify_between()
                .when(!app.crossfader().enabled, |switches| switches.opacity(0.4))
                .child(assign_switch(app, DeckId::A, cx))
                .child(assign_switch(app, DeckId::B, cx)),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(side_label(app, CrossfaderAssign::A))
                .child(
                    div()
                        .relative()
                        .flex_1()
                        .when(!crossfader_in_use(app), |slider| slider.opacity(0.4))
                        .when(!app.crossfader().enabled, |slider| {
                            slider.child(
                                div()
                                    .absolute()
                                    .inset_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(theme::TINY_TEXT_SIZE)
                                    .text_color(theme::active().text)
                                    .child("Off"),
                            )
                        })
                        .child(slider::render(
                            app,
                            Slider::Crossfader,
                            CROSSFADER_THUMB,
                            (app.smart_fader_on() && !app.smart_fader_resting())
                                .then_some((0.0, theme::active().text)),
                            cx,
                        )),
                )
                .child(side_label(app, CrossfaderAssign::B)),
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
                        .text_color(theme::active().muted)
                        .child("Smart"),
                )
                .child(cue_button(
                    ("smart-cfx", 0),
                    "CFX",
                    app.smart_cfx_on(),
                    theme::active().text,
                    Command::ToggleSmartCfx,
                    cx,
                ))
                .child(
                    div()
                        .id("smart-cfx-preset")
                        .accessible_button("Choose Smart CFX preset")
                        .aria_expanded(app.smart_cfx_menu)
                        .flex()
                        .items_center()
                        .h(px(20.0))
                        .px(px(6.0))
                        .rounded(px(5.0))
                        .text_size(theme::TINY_TEXT_SIZE)
                        .text_color(theme::active().muted)
                        .cursor_pointer()
                        .hover(|button| {
                            button
                                .bg(theme::active().raised)
                                .text_color(theme::active().text)
                        })
                        .when(app.smart_cfx_menu, |button| {
                            button
                                .bg(theme::active().raised)
                                .text_color(theme::active().text)
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
            theme::active().text,
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
            .border_color(theme::active().line)
            .bg(theme::active().panel)
            .shadow_lg()
            .occlude()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| this.close_smart_cfx_menu(cx)))
            .children(SmartCfx::ALL.into_iter().map(|preset| {
                let chosen = preset == current;
                div()
                    .id(SharedString::from(format!("smart-cfx-{preset}")))
                    .accessible_toggle(preset_label(preset), chosen)
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(26.0))
                    .px(px(8.0))
                    .rounded(px(5.0))
                    .text_size(theme::SMALL_TEXT_SIZE)
                    .cursor_pointer()
                    .when(chosen, |item| {
                        item.bg(theme::active().raised)
                            .font_weight(FontWeight::MEDIUM)
                    })
                    .hover(|item| item.bg(theme::active().raised))
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.perform(Command::ChooseSmartCfx(preset), window, cx);
                        this.close_smart_cfx_menu(cx);
                    }))
                    .child(preset_label(preset))
                    .when(chosen, |item| {
                        item.child(div().text_color(theme::active().muted).child("\u{2713}"))
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
                    theme::active().text,
                    cx,
                ))
                .child(label("Mix"))
                .child(cue_button(
                    ("master-cue", 0),
                    "M",
                    master_cued,
                    theme::active().text,
                    Command::ToggleMasterCue,
                    cx,
                ))
                .child(knob::render(
                    app,
                    Control::HeadphoneLevel,
                    HEADPHONE_KNOB_SIZE,
                    theme::active().text,
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
    let accessible_name = match id.0 {
        "headphone-cue" => format!("Deck {} headphone Cue", if id.1 == 0 { "A" } else { "B" }),
        "master-cue" => "M: Master headphone cue".to_owned(),
        "smart-cfx" => "Smart CFX".to_owned(),
        "smart-fader" => "Smart Fader".to_owned(),
        _ => text.to_owned(),
    };
    div()
        .id(id)
        .accessible_toggle(accessible_name, on)
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
                    .text_color(theme::active().text)
            } else {
                button
                    .border_color(theme::active().line)
                    .text_color(theme::active().muted)
                    .hover(|button| button.text_color(theme::active().text))
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
                .text_color(theme::active().muted)
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
            theme::active().meter_off
        } else if segment >= HOT_FROM {
            theme::active().meter_hot
        } else {
            theme::active().meter_on
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
        .text_color(theme::active().muted)
        .child(text)
}

/// Whether any channel plays through the crossfader.
fn crossfader_in_use(app: &DubplateApp) -> bool {
    let crossfader = app.crossfader();
    crossfader.enabled
        && crossfader
            .assign
            .iter()
            .any(|assign| *assign != CrossfaderAssign::Thru)
}

/// The decks on one side of the crossfader, at its end: the letters of the decks assigned
/// there, or a dash when none is.
fn side_label(app: &DubplateApp, side: CrossfaderAssign) -> impl IntoElement {
    let assign = app.crossfader().assign;
    let decks: Vec<DeckId> = DeckId::ALL
        .into_iter()
        .filter(|deck| assign[deck.index()] == side)
        .collect();
    let mut label = div()
        .flex()
        .justify_center()
        .min_w(px(8.0))
        .font_family(MONO_FAMILY)
        .text_size(theme::TINY_TEXT_SIZE)
        .font_weight(FontWeight::SEMIBOLD);
    if decks.is_empty() {
        label = label.text_color(theme::active().muted).child("\u{2013}");
    }
    label.children(decks.into_iter().map(|deck| {
        div()
            .text_color(theme::deck_colour(deck))
            .child(deck_letter(deck))
    }))
}

/// A channel's crossfader assign switch, as under each channel of a DJM: A, THRU or B. THRU,
/// which takes the channel off the crossfader, is outlined in amber so it never goes unseen.
fn assign_switch(
    app: &DubplateApp,
    deck: DeckId,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<> {
    let current = app.crossfader().assign[deck.index()];
    div()
        .flex()
        .items_center()
        .gap(px(4.0))
        .child(
            div()
                .w(px(3.0))
                .h(px(12.0))
                .rounded(px(1.5))
                .bg(theme::deck_colour(deck)),
        )
        .child(
            div()
                .id(("crossfader-assign", deck.index()))
                .role(gpui::Role::RadioGroup)
                .aria_label(format!("Deck {} crossfader assign", deck_letter(deck)))
                .flex()
                .h(px(18.0))
                .p(px(1.0))
                .gap(px(1.0))
                .rounded(px(5.0))
                .border_1()
                .border_color(theme::active().line)
                .children(CrossfaderAssign::ALL.into_iter().map(move |assign| {
                    let chosen = assign == current;
                    let colour = match assign {
                        CrossfaderAssign::A => theme::deck_colour(DeckId::A),
                        CrossfaderAssign::Thru => theme::active().attention,
                        CrossfaderAssign::B => theme::deck_colour(DeckId::B),
                    };
                    div()
                        .id(SharedString::from(format!(
                            "crossfader-assign-{}-{assign}",
                            deck_letter(deck)
                        )))
                        .accessible_toggle(
                            format!("Deck {} on {assign}", deck_letter(deck)),
                            chosen,
                        )
                        .flex()
                        .items_center()
                        .px(px(4.0))
                        .rounded(px(4.0))
                        .border_1()
                        .font_family(MONO_FAMILY)
                        .text_size(px(9.0))
                        .cursor_pointer()
                        .map(|segment| {
                            if chosen {
                                segment
                                    .border_color(theme::faded(colour, 0.6))
                                    .bg(theme::faded(colour, 0.18))
                                    .text_color(theme::active().text)
                            } else {
                                segment
                                    .border_color(gpui::transparent_black())
                                    .text_color(theme::active().muted)
                                    .hover(|segment| segment.text_color(theme::active().text))
                            }
                        })
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.perform(Command::SetCrossfaderAssign(deck, assign), window, cx);
                        }))
                        .child(assign.to_string())
                })),
        )
}
