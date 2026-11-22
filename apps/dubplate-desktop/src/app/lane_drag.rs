//! Moving a deck by dragging its waveform on the lanes, through the same jog commands as a
//! controller's platter: a stopped deck scrubs under the hand, a playing deck is nudged as by
//! the jog's edge and never jumps, and Shift searches. On release a stopped deck lands on the
//! nearest beat when quantize is on.

use std::time::Duration;

use dubplate_engine::{DeckId, NUDGE_SECONDS_PER_REVOLUTION, SCRATCH_SECONDS_PER_REVOLUTION};
use gpui::{Context, Pixels, Point, Window};

use super::{Drag, DubplateApp};
use crate::commands::{Command, JogMotion};
use crate::waveform::Zoom;

/// How long a released scrub takes to come to rest before a stopped deck lands on the beat.
const SCRUB_SETTLE: Duration = Duration::from_millis(80);

/// A waveform being dragged.
#[derive(Debug, Clone, Copy)]
pub(super) struct LaneDrag {
    deck: DeckId,
    motion: JogMotion,
    last_x: Pixels,
    /// The part of a tick the pointer moved but no jog has sent yet.
    carried_ticks: f64,
}

impl LaneDrag {
    /// The jog ticks for the pointer moving to `x`, at `pixels_per_track_second` on the lane,
    /// with the drag that carries the rest. Dragging the waveform right goes back in the track.
    fn moved_to(
        self,
        x: Pixels,
        pixels_per_track_second: f64,
        ticks_per_revolution: u32,
    ) -> (i32, Self) {
        let track_seconds = -f64::from(f32::from(x - self.last_x)) / pixels_per_track_second;
        let revolutions = track_seconds / seconds_per_revolution(self.motion);
        let ticks = revolutions * f64::from(ticks_per_revolution) + self.carried_ticks;
        let whole = ticks.trunc();
        (
            whole as i32,
            Self {
                last_x: x,
                carried_ticks: ticks - whole,
                ..self
            },
        )
    }
}

/// The track seconds one revolution of the wheel moves for `motion`, so the waveform follows the
/// pointer when scrubbing or nudging. A search uses the scratch rate, so its own faster wheel
/// runs ahead of the pointer.
fn seconds_per_revolution(motion: JogMotion) -> f64 {
    match motion {
        JogMotion::Scratch | JogMotion::Search => SCRATCH_SECONDS_PER_REVOLUTION,
        JogMotion::Nudge => NUDGE_SECONDS_PER_REVOLUTION,
    }
}

impl DubplateApp {
    /// Whether the pointer holds `deck`'s waveform.
    pub(super) fn is_dragging_lane(&self, deck: DeckId) -> bool {
        matches!(self.dragging, Some(Drag::Lane(drag)) if drag.deck == deck)
    }

    /// Starts dragging `deck`'s waveform: a hand on the platter of a stopped deck, the jog's edge
    /// of a playing one, or a search with Shift.
    pub(super) fn begin_lane_drag(
        &mut self,
        deck: DeckId,
        position: Point<Pixels>,
        searching: bool,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if self.deck(deck).visuals().is_none() {
            return;
        }
        let motion = if searching {
            JogMotion::Search
        } else if self.deck_state(deck).playing {
            JogMotion::Nudge
        } else {
            JogMotion::Scratch
        };
        if motion == JogMotion::Scratch {
            self.perform(Command::JogTouch(deck, true), window, cx);
        }
        self.dragging = Some(Drag::Lane(LaneDrag {
            deck,
            motion,
            last_x: position.x,
            carried_ticks: 0.0,
        }));
    }

    /// Turns the jog by as far as the pointer moved along the lane.
    pub(super) fn drag_lane(
        &mut self,
        drag: LaneDrag,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(ticks_per_revolution) = self
            .engine
            .as_ref()
            .ok()
            .map(|engine| engine.controller.jog_ticks_per_revolution())
        else {
            return;
        };
        let zoom = self
            .deck(drag.deck)
            .visuals()
            .and_then(|visuals| visuals.zoomed.as_ref())
            .map_or(Zoom::DEFAULT, |zoomed| zoomed.zoom());
        let pixels_per_track_second =
            zoom.pixels_per_track_second(self.deck_state(drag.deck).tempo);
        let (ticks, moved) =
            drag.moved_to(position.x, pixels_per_track_second, ticks_per_revolution);
        self.dragging = Some(Drag::Lane(moved));
        if ticks != 0 {
            self.perform(Command::Jog(drag.deck, drag.motion, ticks), window, cx);
        }
    }

    /// Lets go of the waveform: the hand leaves the platter, and a stopped deck lands on the
    /// nearest beat once the scrub comes to rest, when quantize is on.
    pub(super) fn end_lane_drag(
        &mut self,
        drag: LaneDrag,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let deck = drag.deck;
        if drag.motion == JogMotion::Scratch {
            self.perform(Command::JogTouch(deck, false), window, cx);
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SCRUB_SETTLE).await;
            this.update(cx, |this, cx| this.land_on_beat(deck, cx)).ok();
        })
        .detach();
    }

    fn land_on_beat(&mut self, deck: DeckId, cx: &mut Context<'_, Self>) {
        let quantize = self
            .engine
            .as_ref()
            .is_ok_and(|engine| engine.controller.quantize());
        let state = self.deck_state(deck);
        if !quantize || state.playing || self.dragging.is_some() {
            return;
        }
        let landed = self.quantized(deck, state.position_seconds);
        if (landed - state.position_seconds).abs() > f64::EPSILON {
            self.jump(deck, landed, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use gpui::px;

    use super::*;

    fn drag(motion: JogMotion) -> LaneDrag {
        LaneDrag {
            deck: DeckId::A,
            motion,
            last_x: px(100.0),
            carried_ticks: 0.0,
        }
    }

    #[test]
    fn a_scrub_follows_the_pointer_and_carries_part_ticks() {
        let pixels_per_track_second = 100.0;
        let (ticks, moved) =
            drag(JogMotion::Scratch).moved_to(px(90.0), pixels_per_track_second, 720);
        assert_eq!(
            ticks, 40,
            "a tenth of a second forward is 40 ticks at 1.8 s a turn"
        );
        let (ticks, moved) = moved.moved_to(px(90.1), pixels_per_track_second, 720);
        assert_eq!(ticks, 0);
        assert!(
            (moved.carried_ticks + 0.4).abs() < 1e-3,
            "{}",
            moved.carried_ticks
        );
        let (ticks, _) = moved.moved_to(px(90.2), pixels_per_track_second, 720);
        assert_eq!(ticks, 0, "two small moves add up before a tick goes");
    }

    #[test]
    fn a_nudge_moves_the_music_as_far_as_the_pointer() {
        let (ticks, _) = drag(JogMotion::Nudge).moved_to(px(130.0), 100.0, 720);
        assert_eq!(ticks, -720, "0.3 seconds back is one turn of the edge");
    }
}
