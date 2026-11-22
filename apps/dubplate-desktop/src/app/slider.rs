//! Draggable sliders: a thin track with a thumb, as on the design's faders. Each records where
//! it was drawn so a drag anywhere in the window can follow the pointer.

use std::rc::Rc;

use gpui::prelude::*;
use gpui::{Context, MouseButton, MouseDownEvent, Pixels, Rgba, canvas, div, px, relative};

use super::{DubplateApp, HitAreas};
use crate::controls::{Control, Orientation, Slider};
use crate::theme;

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
/// `fill` when given. A tempo slider whose fader waits to pick up the playing tempo also shows
/// where that fader sits, as an outline.
pub(super) fn render(
    app: &DubplateApp,
    slider: Slider,
    thumb: Thumb,
    fill: Option<(f32, Rgba)>,
    cx: &mut Context<'_, DubplateApp>,
) -> gpui::Stateful<gpui::Div> {
    let (fraction, waiting) = match slider {
        Slider::Tempo(deck) => {
            let view = app.tempo_slider_view(deck);
            (view.thumb, view.waiting_fader)
        }
        _ => (app.control_fraction(Control::from(slider)), None),
    };
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
    let waiting_knob = waiting.map(|waiting| {
        let outline = div()
            .absolute()
            .rounded(px(2.0))
            .border_1()
            .border_color(theme::ATTENTION);
        match orientation {
            Orientation::Horizontal => outline
                .left(relative(waiting))
                .ml(-thumb.along / 2.0)
                .top(relative(0.5))
                .mt(-thumb.across / 2.0)
                .w(thumb.along)
                .h(thumb.across),
            Orientation::Vertical => outline
                .top(relative(1.0 - waiting))
                .mt(-thumb.along / 2.0)
                .left(relative(0.5))
                .ml(-thumb.across / 2.0)
                .w(thumb.across)
                .h(thumb.along),
        }
    });
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
        .children(waiting_knob)
        .child(knob);
    match orientation {
        Orientation::Horizontal => base.h(GRAB_THICKNESS),
        Orientation::Vertical => base.w(GRAB_THICKNESS.max(thumb.across)),
    }
}
