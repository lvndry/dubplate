//! Draggable sliders: a thin track with a thumb, as on the design's faders. Each records where
//! it was drawn so a drag anywhere in the window can follow the pointer.

use std::rc::Rc;

use gpui::prelude::*;
use gpui::{Context, MouseButton, MouseDownEvent, Pixels, Rgba, canvas, div, px, relative};

use super::{DubplateApp, HitAreas};
use crate::controls::{Control, Orientation, Slider};
use crate::theme;
use crate::widgets::{InteractiveControl, adjusted_value, control_label};

const TRACK_THICKNESS: Pixels = px(3.0);
const GRAB_THICKNESS: Pixels = px(16.0);

/// The thumb's size along and across the slider.
#[derive(Debug, Clone, Copy)]
pub(super) struct Thumb {
    pub(super) along: Pixels,
    pub(super) across: Pixels,
}

/// The thumb of the channel faders.
pub(super) const FADER_THUMB: Thumb = Thumb {
    along: px(9.0),
    across: px(21.0),
};
/// The thumb of the crossfader.
pub(super) const CROSSFADER_THUMB: Thumb = Thumb {
    along: px(16.0),
    across: px(13.0),
};
/// The thumb of the tempo and master level sliders.
pub(super) const SMALL_THUMB: Thumb = Thumb {
    along: px(8.0),
    across: px(12.0),
};

/// A slider showing `slider`'s position, with the part from `fill_from` to the thumb drawn in
/// `fill` when given.
pub(super) fn render(
    app: &DubplateApp,
    slider: Slider,
    thumb: Thumb,
    fill: Option<(f32, Rgba)>,
    cx: &mut Context<'_, DubplateApp>,
) -> gpui::Stateful<gpui::Div> {
    let fraction = app.control_fraction(Control::from(slider));
    let orientation = slider.orientation();
    let hit_areas: Rc<HitAreas> = Rc::clone(&app.hit_areas);
    let recorder = canvas(
        move |bounds, _, _| hit_areas.sliders[slider.index()].set(Some(bounds)),
        |_, (), _, _| {},
    )
    .absolute()
    .size_full();
    let track = div().absolute().rounded(px(2.0)).bg(theme::TRACK);
    let filled = fill.map(|(from, colour)| {
        let start = from.min(fraction);
        let span = (from - fraction).abs();
        let bar = div().absolute().rounded(px(2.0)).bg(colour);
        match orientation {
            Orientation::Horizontal => bar
                .left(relative(start))
                .w(relative(span))
                .h(TRACK_THICKNESS)
                .top(relative(0.5))
                .mt(-TRACK_THICKNESS / 2.0),
            Orientation::Vertical => bar
                .bottom(relative(start))
                .h(relative(span))
                .w(TRACK_THICKNESS)
                .left(relative(0.5))
                .ml(-TRACK_THICKNESS / 2.0),
        }
    });
    let knob = div().absolute().rounded(px(2.0)).bg(theme::THUMB);
    let (track, knob) = match orientation {
        Orientation::Horizontal => (
            track
                .left_0()
                .right_0()
                .top(relative(0.5))
                .mt(-TRACK_THICKNESS / 2.0)
                .h(TRACK_THICKNESS),
            knob.left(relative(fraction))
                .ml(-thumb.along / 2.0)
                .top(relative(0.5))
                .mt(-thumb.across / 2.0)
                .w(thumb.along)
                .h(thumb.across),
        ),
        Orientation::Vertical => (
            track
                .top_0()
                .bottom_0()
                .left(relative(0.5))
                .ml(-TRACK_THICKNESS / 2.0)
                .w(TRACK_THICKNESS),
            knob.top(relative(1.0 - fraction))
                .mt(-thumb.along / 2.0)
                .left(relative(0.5))
                .ml(-thumb.across / 2.0)
                .w(thumb.across)
                .h(thumb.along),
        ),
    };
    let base = div()
        .id(("slider", slider.index()))
        .accessible_slider(control_label(Control::from(slider)), fraction)
        .map(|element| accessible_adjustments(element, Control::from(slider), cx))
        .aria_orientation(match orientation {
            Orientation::Horizontal => gpui::Orientation::Horizontal,
            Orientation::Vertical => gpui::Orientation::Vertical,
        })
        .on_key_down(cx.listener(move |this, event, _, cx| {
            let control = Control::from(slider);
            if let Some(value) =
                adjusted_value(event, this.control_fraction(control), control.resting())
            {
                this.set_control(control, value, cx);
                cx.stop_propagation();
            }
        }))
        .relative()
        .flex_none()
        .cursor_pointer()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                this.begin_drag(slider, event.position, event.click_count, cx);
                cx.stop_propagation();
            }),
        )
        .child(recorder)
        .child(track)
        .children(filled)
        .child(knob);
    match orientation {
        Orientation::Horizontal => base.h(GRAB_THICKNESS),
        Orientation::Vertical => base.w(GRAB_THICKNESS.max(thumb.across)),
    }
}

/// Assistive technologies adjust the same control state as the keyboard and pointer.
pub(super) fn accessible_adjustments(
    element: gpui::Stateful<gpui::Div>,
    control: Control,
    cx: &mut Context<'_, DubplateApp>,
) -> gpui::Stateful<gpui::Div> {
    crate::widgets::numeric_actions(element, cx, move |this, change, cx| {
        this.set_control(control, change.apply(this.control_fraction(control)), cx);
    })
}
