//! Rotary knobs: an arc that fills from the left as the knob turns, lit in the deck's colour
//! when it is away from where it rests, and a mark on the ring where something else, such as
//! Auto Gain on TRIM, puts the level. Drag up or down to turn; double-click to reset.

use gpui::prelude::*;
use gpui::{
    Context, MouseButton, MouseDownEvent, PathBuilder, Pixels, Point, Rgba, SharedString, canvas,
    div, point, px,
};

use super::DubplateApp;
use crate::controls::Control;
use crate::theme;
use crate::widgets::{InteractiveControl, adjusted_value, control_label, tooltip};

const STROKE: Pixels = px(1.5);
/// Where the arc starts, in degrees clockwise from three o'clock: seven o'clock.
const START_DEGREES: f32 = 135.0;
/// How far the arc sweeps from fully left to fully right.
const SWEEP_DEGREES: f32 = 270.0;
/// Line segments per full sweep, enough for a smooth arc at this size.
const SEGMENTS: usize = 36;
/// A knob this far from where it rests is drawn in its deck's colour.
const AT_REST: f32 = 0.02;
/// The radius of the mark on the ring.
const MARK_RADIUS: f32 = 2.5;

/// Where a knob's level really is when something besides the knob moves it, and why.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct KnobMark {
    /// Where the knob would sit for the level, from 0.0 to 1.0.
    pub fraction: f32,
    /// What moves it, such as `AUTO +2.5 dB`.
    pub label: SharedString,
}

/// A knob for `control`, `size` across, lit in `accent` when turned.
pub(super) fn render(
    app: &DubplateApp,
    control: Control,
    size: Pixels,
    accent: Rgba,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<> {
    let value = app.control_fraction(control);
    let colour = if (value - control.resting()).abs() > AT_REST {
        accent
    } else {
        theme::TEXT
    };
    let mark = app.knob_mark(control);
    let mark_fraction = mark.as_ref().map(|mark| mark.fraction);
    div()
        .id(SharedString::from(format!("knob-{control:?}")))
        .accessible_slider(control_label(control), value)
        .when_some(mark.map(|mark| mark.label), |knob, label| {
            knob.aria_description(label.clone()).tooltip(tooltip(label))
        })
        .map(|element| super::slider::accessible_adjustments(element, control, cx))
        .on_key_down(cx.listener(move |this, event, _, cx| {
            let control = control;
            if let Some(value) =
                adjusted_value(event, this.control_fraction(control), control.resting())
            {
                this.set_control(control, value, cx);
                cx.stop_propagation();
            }
        }))
        .size(size)
        .flex_none()
        .cursor_ns_resize()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                this.begin_knob_drag(control, event.position, event.click_count, cx);
                cx.stop_propagation();
            }),
        )
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, (), window, _| {
                    let center = bounds.center();
                    let radius = f32::from(bounds.size.width.min(bounds.size.height)) / 2.0
                        - f32::from(STROKE);
                    if let Some(track) = arc(center, radius, 1.0) {
                        window.paint_path(track, theme::TRACK);
                    }
                    if let Some(filled) = arc(center, radius, value) {
                        window.paint_path(filled, colour);
                    }
                    if let Some(mark) = mark_fraction.and_then(|fraction| dot(center, radius, fraction)) {
                        window.paint_path(mark, theme::KNOB_MARK);
                    }
                },
            )
            .size_full(),
        )
}

/// A dot on the ring of `radius` around `center`, at `fraction` of the sweep.
fn dot(center: Point<Pixels>, radius: f32, fraction: f32) -> Option<gpui::Path<Pixels>> {
    let radians = (START_DEGREES + SWEEP_DEGREES * fraction.clamp(0.0, 1.0)).to_radians();
    let at = center + point(px(radius * radians.cos()), px(radius * radians.sin()));
    let mut builder = PathBuilder::fill();
    for step in 0..=SEGMENTS {
        let around = std::f32::consts::TAU * step as f32 / SEGMENTS as f32;
        let edge = at + point(
            px(MARK_RADIUS * around.cos()),
            px(MARK_RADIUS * around.sin()),
        );
        if step == 0 {
            builder.move_to(edge);
        } else {
            builder.line_to(edge);
        }
    }
    builder.build().ok()
}

/// The arc of `radius` from fully left to `fraction` of the sweep, around `center`.
fn arc(center: Point<Pixels>, radius: f32, fraction: f32) -> Option<gpui::Path<Pixels>> {
    let fraction = fraction.clamp(0.0, 1.0);
    let steps = ((SEGMENTS as f32 * fraction).ceil() as usize).max(1);
    let mut builder = PathBuilder::stroke(STROKE);
    for step in 0..=steps {
        let degrees = START_DEGREES + SWEEP_DEGREES * fraction * step as f32 / steps as f32;
        let radians = degrees.to_radians();
        let at = center + point(px(radius * radians.cos()), px(radius * radians.sin()));
        if step == 0 {
            builder.move_to(at);
        } else {
            builder.line_to(at);
        }
    }
    builder.build().ok()
}
