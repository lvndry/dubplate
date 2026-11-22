//! Telling the DJ when the audio glitches often enough to matter: three dropouts within a
//! minute, at most once in a while, and never as a running count in front of the DJ.

use std::time::{Duration, Instant};

/// How many dropouts within [`WINDOW`] count as glitching.
const DROPOUTS: usize = 3;
/// How close together they must come.
const WINDOW: Duration = Duration::from_secs(60);
/// How long after saying so Dubplate stays quiet, so a struggling Mac does not nag a DJ
/// mid-set.
const QUIET: Duration = Duration::from_secs(600);

/// Watches the engine's dropout count.
#[derive(Debug, Default)]
pub struct GlitchWatch {
    seen: u64,
    recent: [Option<Instant>; DROPOUTS],
    next: usize,
    last_told: Option<Instant>,
}

impl GlitchWatch {
    /// Notes the engine's dropout count `total` at `now`, and returns whether to tell the DJ
    /// the audio is glitching.
    pub fn observe(&mut self, total: u64, now: Instant) -> bool {
        if total < self.seen {
            self.seen = total;
        }
        for _ in self.seen..total.min(self.seen + DROPOUTS as u64) {
            self.recent[self.next] = Some(now);
            self.next = (self.next + 1) % DROPOUTS;
        }
        self.seen = total;
        let glitching = self
            .recent
            .iter()
            .all(|dropout| dropout.is_some_and(|at| now.saturating_duration_since(at) <= WINDOW));
        let quiet = self
            .last_told
            .is_some_and(|told| now.saturating_duration_since(told) < QUIET);
        if glitching && !quiet {
            self.last_told = Some(now);
            self.recent = [None; DROPOUTS];
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_dropout_stays_quiet() {
        let mut watch = GlitchWatch::default();
        let start = Instant::now();
        assert!(!watch.observe(1, start));
        assert!(!watch.observe(1, start + Duration::from_secs(5)));
    }

    #[test]
    fn three_dropouts_within_a_minute_say_so_once() {
        let mut watch = GlitchWatch::default();
        let start = Instant::now();
        assert!(!watch.observe(1, start));
        assert!(!watch.observe(2, start + Duration::from_secs(20)));
        assert!(watch.observe(3, start + Duration::from_secs(40)));
        assert!(!watch.observe(6, start + Duration::from_secs(45)));
        assert!(watch.observe(9, start + QUIET + Duration::from_secs(60)));
    }

    #[test]
    fn dropouts_spread_over_minutes_stay_quiet() {
        let mut watch = GlitchWatch::default();
        let start = Instant::now();
        for minute in 0..5 {
            assert!(!watch.observe(minute + 1, start + Duration::from_secs(70 * minute)));
        }
    }
}
