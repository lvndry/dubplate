//! A playhead that glides at the screen's frame rate. The engine publishes its position once
//! per audio buffer, which is a few milliseconds of steps; the clock runs on its own between
//! them and is pulled gently back to the engine, so the waveform scrolls without jitter.

use std::time::Instant;

use dubplate_engine::DeckState;

/// Further than this from the engine, in seconds, and the clock jumps: a seek or a cue.
const JUMP_SECONDS: f64 = 0.15;
/// How much of its distance to the engine the clock closes each frame.
const CORRECTION: f64 = 0.1;

/// The position shown for one deck.
#[derive(Debug, Clone, Default)]
pub struct PlayheadClock {
    shown_seconds: f64,
    last_frame: Option<Instant>,
}

impl PlayheadClock {
    /// The position to draw at `now` for a deck in `state`.
    pub fn position(&mut self, state: &DeckState, slipping: bool, now: Instant) -> f64 {
        let engine = state.position_seconds;
        // A short roll can wrap every frame. Follow the audible playhead through the
        // roll and its landing, then resume smoothing from the new position.
        let previous = self.last_frame;
        self.last_frame = (!slipping).then_some(now);
        let Some(last_frame) = previous.filter(|_| state.playing && !slipping) else {
            self.shown_seconds = engine;
            return engine;
        };
        let elapsed = (now - last_frame).as_secs_f64();
        let predicted = self.shown_seconds + elapsed * state.tempo;
        let error = engine - predicted;
        self.shown_seconds = if error.abs() > JUMP_SECONDS {
            engine
        } else {
            predicted + error * CORRECTION
        };
        self.shown_seconds
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn state(position_seconds: f64, playing: bool) -> DeckState {
        DeckState {
            loaded: true,
            playing,
            position_seconds,
            cue_point_seconds: 0.0,
            tempo: 1.0,
        }
    }

    #[test]
    fn a_stopped_deck_shows_the_engine_position() {
        let mut clock = PlayheadClock::default();
        let now = Instant::now();
        assert_eq!(clock.position(&state(12.5, false), false, now), 12.5);
        assert_eq!(clock.position(&state(3.0, false), false, now), 3.0);
    }

    #[test]
    fn between_engine_updates_the_playhead_keeps_moving_smoothly() {
        let mut clock = PlayheadClock::default();
        let start = Instant::now();
        let frame = Duration::from_micros(16_667);
        let buffer_seconds = 0.0107;
        let mut previous = clock.position(&state(10.0, true), false, start);
        for frame_number in 1..120 {
            let now = start + frame * frame_number;
            let true_seconds = 10.0 + (frame * frame_number).as_secs_f64();
            let engine = 10.0 + (true_seconds - 10.0 - (true_seconds - 10.0) % buffer_seconds);
            let shown = clock.position(&state(engine, true), false, now);
            let step = shown - previous;
            assert!(
                step > 0.010 && step < 0.024,
                "step {step} at frame {frame_number}"
            );
            assert!((shown - true_seconds).abs() < 0.02);
            previous = shown;
        }
    }

    #[test]
    fn a_short_roll_draws_the_audible_playhead_through_wrap_and_release() {
        let mut clock = PlayheadClock::default();
        let start = Instant::now();
        let frame = Duration::from_millis(16);
        clock.position(&state(10.0, true), false, start);
        // The roll wraps inside a 1/16-beat slice, below the normal seek threshold.
        for (tick, position) in [(1, 9.985), (2, 9.970), (3, 9.986)] {
            assert_eq!(
                clock.position(&state(position, true), true, start + frame * tick),
                position
            );
        }
        assert_eq!(
            clock.position(&state(10.064, true), false, start + frame * 4),
            10.064
        );
    }

    #[test]
    fn a_seek_jumps_straight_to_the_new_position() {
        let mut clock = PlayheadClock::default();
        let start = Instant::now();
        clock.position(&state(10.0, true), false, start);
        let shown = clock.position(&state(60.0, true), false, start + Duration::from_millis(16));
        assert_eq!(shown, 60.0);
    }
}
