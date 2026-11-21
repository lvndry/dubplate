//! Rotary knobs: an arc that fills from the left as the knob turns, lit in the deck's colour
//! when it is away from where it rests. Drag up or down to turn; double-click to reset.

use gpui::prelude::*;
use gpui::{
    Context, MouseButton, MouseDownEvent, PathBuilder, Pixels, Point, Rgba, SharedString, canvas,
    div, point, px,
};

use super::DubplateApp;
use crate::controls::Control;
use crate::theme;

const KNOB_SIZE: Pixels = px(26.0);
const RADIUS: f32 = 10.0;
const STROKE: Pixels = px(2.0);
/// Where the arc starts, in degrees clockwise from three o'clock: seven o'clock.
const START_DEGREES: f32 = 135.0;
/// How far the arc sweeps from fully left to fully right.
const SWEEP_DEGREES: f32 = 270.0;
/// Line segments per full sweep, enough for a smooth arc at this size.
const SEGMENTS: usize = 36;
/// A knob this far from where it rests is drawn in its deck's colour.
const AT_REST: f32 = 0.02;

/// A knob for `control`, labelled `label`, lit in `accent` when turned.
pub(super) fn render(
    app: &DubplateApp,
    control: Control,
    label: SharedString,
    accent: Rgba,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<> {
    let value = app.control_fraction(control);
    let colour = if (value - control.resting()).abs() > AT_REST {
        accent
    } else {
        theme::TEXT
    };
    div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(
            div()
                .id(SharedString::from(format!("knob-{control:?}")))
                .size(KNOB_SIZE)
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
                            if let Some(track) = arc(center, 1.0) {
                                window.paint_path(track, theme::TRACK);
                            }
                            if let Some(filled) = arc(center, value) {
                                window.paint_path(filled, colour);
                            }
                        },
                    )
                    .size_full(),
                ),
        )
        .child(
            div()
                .text_size(theme::TINY_TEXT_SIZE)
                .text_color(theme::MUTED)
                .child(label),
        )
}

/// The arc from fully left to `fraction` of the sweep, around `center`.
fn arc(center: Point<Pixels>, fraction: f32) -> Option<gpui::Path<Pixels>> {
    let fraction = fraction.clamp(0.0, 1.0);
    let steps = ((SEGMENTS as f32 * fraction).ceil() as usize).max(1);
    let mut builder = PathBuilder::stroke(STROKE);
    for step in 0..=steps {
        let degrees = START_DEGREES + SWEEP_DEGREES * fraction * step as f32 / steps as f32;
        let radians = degrees.to_radians();
        let at = center + point(px(RADIUS * radians.cos()), px(RADIUS * radians.sin()));
        if step == 0 {
            builder.move_to(at);
        } else {
            builder.line_to(at);
        }
    }
    builder.build().ok()
}
