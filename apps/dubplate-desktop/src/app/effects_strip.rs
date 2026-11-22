//! The effects strip under the decks: one compact row per effect unit with its effect, which
//! opens the effect picker, its parameter, where it sits, its level and its On button. It
//! shows what the controller's BEAT FX section changes as it changes.

use std::rc::Rc;

use dubplate_engine::{DeckId, EffectTarget, EffectUnitId, EffectUnitState, ReleaseFx};
use gpui::component::input::Input;
use gpui::prelude::*;
use gpui::{
    ClickEvent, Context, FontWeight, MouseButton, MouseDownEvent, Pixels, Rgba, SharedString,
    canvas, deferred, div, px, relative, svg,
};

use super::{DubplateApp, HitAreas, Lever, MenuOpening};
use crate::assets::CHEVRON_DOWN_PATH;
use crate::commands::{Command, Direction, EffectEdit};
use crate::effects::{effect_label, parameter_label, search, target_letter};
use crate::fonts::MONO_FAMILY;
use crate::theme;
use crate::widgets::{InteractiveControl, adjusted_value};

const STRIP_HEIGHT: Pixels = px(46.0);
const UNIT_HEIGHT: Pixels = px(32.0);
const LEVEL_WIDTH: Pixels = px(52.0);
const MENU_WIDTH: Pixels = px(300.0);

pub(super) fn render(app: &DubplateApp, cx: &mut Context<'_, DubplateApp>) -> impl IntoElement {
    let show_focus = app.hardware_connected();
    let mut strip = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(10.0))
        .h(STRIP_HEIGHT)
        .px(theme::GUTTER)
        .border_b_1()
        .border_color(theme::LINE)
        .bg(theme::BACKGROUND);
    for unit in EffectUnitId::ALL {
        if let Some(state) = app.effect_unit(unit) {
            let lever = app.beat_fx_lever().filter(|_| app.effect_focus == unit);
            let focused = show_focus && app.effect_focus == unit;
            strip = strip.child(unit_row(app, unit, state, focused, lever, cx));
        }
    }
    strip
}

fn unit_row(
    app: &DubplateApp,
    unit: EffectUnitId,
    state: EffectUnitState,
    focused: bool,
    lever: Option<Lever>,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<> {
    let menu = app.effect_menu.filter(|menu| menu.unit == unit);
    let menu_open = menu.is_some();
    let accent = match state.target {
        EffectTarget::Deck(deck) => theme::deck_colour(deck),
        EffectTarget::BothDecks | EffectTarget::Master => theme::deck_colour(DeckId::A),
    };
    div()
        .relative()
        .flex()
        .items_center()
        .gap(px(6.0))
        .h(UNIT_HEIGHT)
        .pl(px(10.0))
        .pr(px(4.0))
        .rounded(px(8.0))
        .border_1()
        .border_color(theme::LINE)
        .bg(theme::PANEL)
        .child(unit_label(unit, focused))
        .child(effect_button(unit, &state, menu_open, cx))
        .child(parameter_stepper(unit, &state, cx))
        .child(targets(unit, &state, cx))
        .when_some(lever, |row, lever| row.child(lever_reading(lever)))
        .child(level_bar(app, unit, state.depth, cx))
        .child(on_button(unit, state.enabled, accent, cx))
        .child(release_button(unit, &state, cx))
        .when_some(menu, |row, menu| {
            row.child(picker(app, unit, &state, menu.opening, cx))
        })
}

/// `FX 1`, with a dot when the controller's BEAT FX section drives this unit.
fn unit_label(unit: EffectUnitId, focused: bool) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(px(4.0))
        .text_size(theme::TINY_TEXT_SIZE)
        .text_color(if focused { theme::TEXT } else { theme::MUTED })
        .when(focused, |label| {
            label.child(div().size(px(5.0)).rounded_full().bg(theme::OK))
        })
        .child(unit.to_string())
}

fn effect_button(
    unit: EffectUnitId,
    state: &EffectUnitState,
    menu_open: bool,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<> {
    div()
        .id(("effect-name", unit.index()))
        .accessible_button(format!(
            "Effect {}: {}",
            unit.index() + 1,
            effect_label(state.kind)
        ))
        .aria_expanded(menu_open)
        .flex()
        .items_center()
        .justify_between()
        .gap(px(6.0))
        .h(px(26.0))
        .min_w(px(86.0))
        .px(px(8.0))
        .rounded(px(5.0))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .when(menu_open, |button| button.bg(theme::RAISED))
        .hover(|button| button.bg(theme::RAISED))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            this.toggle_effect_menu(unit, window, cx);
        }))
        .child(effect_label(state.kind))
        .child(
            svg()
                .path(CHEVRON_DOWN_PATH)
                .size(px(10.0))
                .text_color(theme::MUTED),
        )
}

fn parameter_stepper(
    unit: EffectUnitId,
    state: &EffectUnitState,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<> {
    let name = state
        .kind
        .parameter()
        .map_or("parameter", |spec| spec.name)
        .to_lowercase();
    let arrow = |id: &'static str, glyph: &'static str, direction: Direction| {
        let label = match direction {
            Direction::Up => format!("Effect {} {name} up", unit.index() + 1),
            Direction::Down => format!("Effect {} {name} down", unit.index() + 1),
        };
        div()
            .id((id, unit.index()))
            .accessible_button(label)
            .flex()
            .items_center()
            .justify_center()
            .w(px(22.0))
            .h(px(26.0))
            .rounded(px(5.0))
            .text_color(theme::MUTED)
            .cursor_pointer()
            .hover(|button| button.bg(theme::RAISED).text_color(theme::TEXT))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.perform(
                    Command::Effect(unit, EffectEdit::Parameter(direction)),
                    window,
                    cx,
                );
            }))
            .child(glyph)
    };
    div()
        .flex()
        .items_center()
        .child(arrow("beat-shorter", "\u{2039}", Direction::Down))
        .child(
            div()
                .min_w(px(34.0))
                .flex()
                .justify_center()
                .font_family(MONO_FAMILY)
                .text_size(theme::SMALL_TEXT_SIZE)
                .child(parameter_label(state.parameter)),
        )
        .child(arrow("beat-longer", "\u{203a}", Direction::Up))
}

fn targets(
    unit: EffectUnitId,
    state: &EffectUnitState,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<> {
    let current = state.target;
    div()
        .flex()
        .gap(px(2.0))
        .children(EffectTarget::ALL.map(|target| {
            let checked = target == current;
            let underline = match target {
                EffectTarget::Deck(deck) => theme::deck_colour(deck),
                EffectTarget::BothDecks | EffectTarget::Master => theme::TEXT,
            };
            div()
                .id((
                    "effect-target",
                    unit.index() * EffectTarget::ALL.len() + target.index(),
                ))
                .accessible_toggle(
                    format!(
                        "Effect {} target {}",
                        unit.index() + 1,
                        target_letter(target)
                    ),
                    checked,
                )
                .flex()
                .items_center()
                .justify_center()
                .size(px(24.0))
                .rounded(px(5.0))
                .font_family(MONO_FAMILY)
                .text_size(theme::TINY_TEXT_SIZE)
                .cursor_pointer()
                .border_b_2()
                .map(|button| {
                    if checked {
                        button
                            .bg(theme::RAISED)
                            .text_color(theme::TEXT)
                            .border_color(underline)
                    } else {
                        button
                            .text_color(theme::MUTED)
                            .border_color(theme::faded(underline, 0.0))
                            .hover(|button| button.text_color(theme::TEXT))
                    }
                })
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.perform(
                        Command::Effect(unit, EffectEdit::Target(target)),
                        window,
                        cx,
                    );
                }))
                .child(target_letter(target))
        }))
}

/// The level bar. The controller's LEVEL/DEPTH knob sets it the moment it reports, so the bar
/// always shows where the knob sits.
fn level_bar(
    app: &DubplateApp,
    unit: EffectUnitId,
    depth: f32,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<> {
    let hit_areas: Rc<HitAreas> = Rc::clone(&app.hit_areas);
    div()
        .id(("effect-level", unit.index()))
        .accessible_slider(format!("Effect {} depth", unit.index() + 1), depth)
        .aria_orientation(gpui::Orientation::Horizontal)
        .map(|element| {
            crate::widgets::numeric_actions(element, cx, move |this, change, cx| {
                let current = this.effect_unit(unit).map_or(depth, |state| state.depth);
                this.edit_effect(unit, EffectEdit::Depth(change.apply(current)), cx);
            })
        })
        .on_key_down(cx.listener(move |this, event, _, cx| {
            let current = this.effect_unit(unit).map_or(depth, |state| state.depth);
            if let Some(value) = adjusted_value(event, current, 0.5) {
                this.edit_effect(unit, EffectEdit::Depth(value), cx);
                cx.stop_propagation();
            }
        }))
        .relative()
        .flex()
        .items_center()
        .w(LEVEL_WIDTH)
        .h(px(16.0))
        .cursor_ew_resize()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                this.begin_effect_level_drag(unit, event.position, window, cx);
                cx.stop_propagation();
            }),
        )
        .child(
            canvas(
                move |bounds, _, _| hit_areas.effect_levels[unit.index()].set(Some(bounds)),
                |_, (), _, _| {},
            )
            .absolute()
            .size_full(),
        )
        .child(
            div()
                .relative()
                .w_full()
                .h(px(3.0))
                .rounded(px(2.0))
                .bg(theme::TRACK)
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .bottom_0()
                        .w(relative(depth.clamp(0.0, 1.0)))
                        .rounded(px(2.0))
                        .bg(theme::TEXT),
                ),
        )
}

fn on_button(
    unit: EffectUnitId,
    enabled: bool,
    accent: Rgba,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<> {
    div()
        .id(("effect-on", unit.index()))
        .accessible_toggle(format!("Effect {} On", unit.index() + 1), enabled)
        .flex()
        .items_center()
        .h(px(24.0))
        .px(px(8.0))
        .rounded(px(5.0))
        .text_size(theme::SMALL_TEXT_SIZE)
        .cursor_pointer()
        .map(|button| {
            if enabled {
                button.bg(accent).text_color(theme::ON_DECK)
            } else {
                button.bg(theme::RAISED).text_color(theme::MUTED)
            }
        })
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            this.perform(Command::Effect(unit, EffectEdit::Toggle), window, cx);
        }))
        .child("On")
}

/// The Release FX that SHIFT + BEAT FX ON plays on this unit; a click picks the next one. It
/// lights while the release is held.
fn release_button(
    unit: EffectUnitId,
    state: &EffectUnitState,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<> {
    let name = release_label(state.release);
    div()
        .id(("effect-release", unit.index()))
        .accessible_button(format!("Effect {} release: {name}", unit.index() + 1))
        .flex()
        .items_center()
        .h(px(24.0))
        .px(px(6.0))
        .rounded(px(5.0))
        .text_size(theme::TINY_TEXT_SIZE)
        .cursor_pointer()
        .map(|button| {
            if state.releasing {
                button.bg(theme::ATTENTION).text_color(theme::ON_DECK)
            } else {
                button
                    .text_color(theme::MUTED)
                    .hover(|button| button.bg(theme::RAISED).text_color(theme::TEXT))
            }
        })
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            this.perform(Command::Effect(unit, EffectEdit::NextRelease), window, cx);
        }))
        .child(name)
}

/// A Release FX's name on the strip.
fn release_label(release: ReleaseFx) -> &'static str {
    match release {
        ReleaseFx::Echo => "Echo out",
        ReleaseFx::BackSpin => "Back spin",
        ReleaseFx::VinylBrake => "Vinyl brake",
    }
}

/// Where the controller's CH SELECT lever sits, next to the targets of the unit it drives, so
/// a target that differs from the lever shows at once.
fn lever_reading(lever: Lever) -> impl IntoElement {
    let reading = match lever {
        Lever::At(target) => target_letter(target),
        Lever::Unreported => "?",
    };
    div()
        .id("beat-fx-lever")
        .aria_label(match lever {
            Lever::At(target) => format!("CH SELECT lever on {}", target_letter(target)),
            Lever::Unreported => "CH SELECT lever not reported yet".to_owned(),
        })
        .flex()
        .items_center()
        .gap(px(3.0))
        .text_size(theme::TINY_TEXT_SIZE)
        .text_color(theme::MUTED)
        .child("lever")
        .child(
            div()
                .font_family(MONO_FAMILY)
                .text_color(theme::TEXT)
                .child(reading),
        )
}

/// The effect picker. Opened with the pointer it has a search field and stays open; opened by
/// FX SELECT it only shows the effects with the chosen one highlighted.
fn picker(
    app: &DubplateApp,
    unit: EffectUnitId,
    state: &EffectUnitState,
    opening: MenuOpening,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<> {
    let searching = opening == MenuOpening::Pointer;
    let query = if searching {
        app.effect_search.read(cx).value().to_string()
    } else {
        String::new()
    };
    let current = state.kind;
    let mut groups = div().flex().flex_col().gap(px(8.0));
    let found = search(&query);
    if found.is_empty() {
        groups = groups.child(
            div()
                .px(px(2.0))
                .text_size(theme::SMALL_TEXT_SIZE)
                .text_color(theme::MUTED)
                .child("No effect has that name."),
        );
    }
    for (name, effects) in found {
        let mut grid = div().flex().flex_wrap().gap(px(2.0));
        for kind in effects {
            let chosen = kind == current;
            grid = grid.child(
                div()
                    .id(SharedString::from(format!(
                        "pick-{}-{}",
                        unit.index(),
                        kind.name()
                    )))
                    .accessible_toggle(effect_label(kind), chosen)
                    .flex()
                    .items_center()
                    .w(px(92.0))
                    .h(px(28.0))
                    .px(px(8.0))
                    .rounded(px(5.0))
                    .text_size(theme::SMALL_TEXT_SIZE)
                    .cursor_pointer()
                    .when(chosen, |button| {
                        button.bg(theme::RAISED).font_weight(FontWeight::MEDIUM)
                    })
                    .when(chosen && !searching, |button| {
                        button.border_1().border_color(theme::TEXT)
                    })
                    .hover(|button| button.bg(theme::RAISED))
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.perform(Command::Effect(unit, EffectEdit::Kind(kind)), window, cx);
                        this.close_effect_menu(cx);
                    }))
                    .child(effect_label(kind)),
            );
        }
        groups = groups.child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .px(px(2.0))
                        .text_size(theme::TINY_TEXT_SIZE)
                        .text_color(theme::MUTED)
                        .child(name),
                )
                .child(grid),
        );
    }
    deferred(
        div()
            .id(("effect-menu", unit.index()))
            .absolute()
            .left_0()
            .top(px(38.0))
            .w(MENU_WIDTH)
            .p(px(8.0))
            .flex()
            .flex_col()
            .gap(px(8.0))
            .rounded(px(10.0))
            .border_1()
            .border_color(theme::LINE)
            .bg(theme::PANEL)
            .shadow_lg()
            .occlude()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| this.close_effect_menu(cx)))
            .when(searching, |picker| {
                picker.child(
                    div()
                        .h(px(30.0))
                        .px(px(8.0))
                        .flex()
                        .items_center()
                        .rounded(px(6.0))
                        .border_1()
                        .border_color(theme::LINE)
                        .bg(theme::BACKGROUND)
                        .child(Input::new(&app.effect_search).appearance(false)),
                )
            })
            .child(groups),
    )
    .with_priority(1)
}
