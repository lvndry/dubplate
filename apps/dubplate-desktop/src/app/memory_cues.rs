//! Memory cues on the decks, as a CDJ and rekordbox's FLX4 layout play them: CUE/LOOP CALL
//! calls the memory cue before or after the playhead, which becomes the cue point with the deck
//! paused there; SHIFT + CUE/LOOP CALL ▶ keeps the cue point, or the loop that is on, as a
//! memory cue, and SHIFT + CUE/LOOP CALL ◀ deletes the one at the cue point. With a loop on,
//! CUE/LOOP CALL halves or doubles it instead.

use dubplate_engine::DeckId;
use dubplate_rekordbox::{MEMORY_CUE_LIMIT, MemoryCue, TrackCues, cue_milliseconds};
use gpui::Context;

use super::DubplateApp;
use super::notice::NoticePlace;
use crate::commands::Direction;
use crate::transport;

/// How close, in milliseconds, a memory cue is to count as at the playhead or the cue point.
const AT_POINT_MS: u32 = 1;

impl DubplateApp {
    /// CUE/LOOP CALL ◀ ([`Direction::Down`]) or ▶: halves or doubles the loop that is on, else
    /// calls the memory cue before or after the playhead.
    pub(super) fn cue_loop_call(
        &mut self,
        deck: DeckId,
        direction: Direction,
        cx: &mut Context<'_, Self>,
    ) {
        if self.deck(deck).visuals().is_none() {
            return;
        }
        if self.loop_state(deck).is_some_and(|state| state.active) {
            self.engine_command(cx, |controller| match direction {
                Direction::Down => controller.halve_loop(deck),
                Direction::Up => controller.double_loop(deck),
            });
            return;
        }
        let playhead_ms = cue_milliseconds(self.deck_state(deck).position_seconds);
        let Some(seconds) = neighbour_memory_cue(&self.deck(deck).cues, playhead_ms, direction)
            .map(MemoryCue::start_seconds)
        else {
            return;
        };
        self.decks[deck.index()].cue_preview = false;
        self.engine_command(cx, |controller| {
            transport::call_point(controller, deck, seconds)
        });
    }

    /// MEMORY: keeps the loop that is on as a saved memory loop, else the cue point as a memory
    /// cue, up to [`MEMORY_CUE_LIMIT`] per track.
    pub(super) fn store_memory_cue(&mut self, deck: DeckId, cx: &mut Context<'_, Self>) {
        if self.deck(deck).visuals().is_none() {
            return;
        }
        let active_loop = self
            .loop_state(deck)
            .filter(|state| state.active)
            .and_then(|state| Some((state.start_seconds?, state.end_seconds?)));
        let cue = match active_loop {
            Some((start, end)) => MemoryCue {
                loop_end_ms: Some(cue_milliseconds(end)),
                ..MemoryCue::at(cue_milliseconds(start))
            },
            None => MemoryCue::at(cue_milliseconds(self.deck_state(deck).cue_point_seconds)),
        };
        let cues = &self.deck(deck).cues;
        if cues
            .memory_cues()
            .iter()
            .any(|kept| kept.start_ms == cue.start_ms && kept.loop_end_ms == cue.loop_end_ms)
        {
            return;
        }
        if cues.memory_cues().len() >= MEMORY_CUE_LIMIT {
            self.show_notice(
                NoticePlace::Deck(deck),
                format!(
                    "This track has {MEMORY_CUE_LIMIT} memory cues, as many as a CDJ calls: \
                     delete one first (SHIFT + CUE/LOOP CALL \u{25c0} at its point)."
                ),
                cx,
            );
            return;
        }
        self.edit_cues(deck, |cues| cues.insert_memory_cue(cue), cx);
    }

    /// DELETE: removes the memory cues at the cue point.
    pub(super) fn delete_memory_cue(&mut self, deck: DeckId, cx: &mut Context<'_, Self>) {
        if self.deck(deck).visuals().is_none() {
            return;
        }
        let cue_point_ms = cue_milliseconds(self.deck_state(deck).cue_point_seconds);
        let at_cue_point = |cue: &MemoryCue| cue.start_ms.abs_diff(cue_point_ms) <= AT_POINT_MS;
        if !self.deck(deck).cues.memory_cues().iter().any(at_cue_point) {
            self.show_notice(
                NoticePlace::Deck(deck),
                "No memory cue at the cue point: call one with CUE/LOOP CALL first.",
                cx,
            );
            return;
        }
        self.edit_cues(deck, |cues| cues.remove_memory_cues(at_cue_point), cx);
    }
}

/// The memory cue the CUE/LOOP CALL arrow calls from `playhead_ms`: the last one before it for
/// ◀ ([`Direction::Down`]), the first one after it for ▶. One at the playhead is not called
/// again.
fn neighbour_memory_cue(
    cues: &TrackCues,
    playhead_ms: u32,
    direction: Direction,
) -> Option<&MemoryCue> {
    let memory = cues.memory_cues();
    match direction {
        Direction::Down => memory
            .iter()
            .rev()
            .find(|cue| cue.start_ms.saturating_add(AT_POINT_MS) < playhead_ms),
        Direction::Up => memory
            .iter()
            .find(|cue| cue.start_ms > playhead_ms.saturating_add(AT_POINT_MS)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cues(starts: &[u32]) -> TrackCues {
        TrackCues::new(
            Default::default(),
            starts.iter().map(|start| MemoryCue::at(*start)).collect(),
        )
    }

    fn called(cues: &TrackCues, playhead_ms: u32, direction: Direction) -> Option<u32> {
        neighbour_memory_cue(cues, playhead_ms, direction).map(|cue| cue.start_ms)
    }

    #[test]
    fn the_arrows_call_the_memory_cue_before_or_after_the_playhead() {
        let cues = cues(&[10_000, 30_000, 60_000]);
        assert_eq!(called(&cues, 20_000, Direction::Down), Some(10_000));
        assert_eq!(called(&cues, 20_000, Direction::Up), Some(30_000));
        assert_eq!(called(&cues, 5_000, Direction::Down), None);
        assert_eq!(called(&cues, 70_000, Direction::Up), None);
    }

    #[test]
    fn a_deck_paused_on_a_memory_cue_steps_to_the_next_one() {
        let cues = cues(&[10_000, 30_000, 60_000]);
        assert_eq!(called(&cues, 30_000, Direction::Up), Some(60_000));
        assert_eq!(called(&cues, 30_000, Direction::Down), Some(10_000));
        assert_eq!(
            called(&cues, 30_001, Direction::Down),
            Some(10_000),
            "a millisecond of rounding still counts as on the cue"
        );
    }
}
