//! Tap tempo: the DJ taps along with the beat and Dubplate works out the tempo and where the
//! beats fall, for a track rekordbox never analysed.

/// Taps further apart than this, in seconds of the track, start a new count.
const RESTART_AFTER_SECONDS: f64 = 2.0;
/// Taps needed before a tempo is given: three beats measured.
const TAPS_NEEDED: usize = 4;
/// The most recent taps kept.
const TAPS_KEPT: usize = 16;
/// The tempos a tap count may give; outside them the taps were not on the beat.
const TEMPO_RANGE_BPM: std::ops::RangeInclusive<f64> = 40.0..=240.0;

/// A tempo from taps: beats per minute, and one beat's place in the track.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TappedTempo {
    /// Beats per minute of the track at its own speed.
    pub bpm: f64,
    /// The first beat of the track, in seconds from its start.
    pub first_beat_seconds: f64,
}

/// The taps on one deck, as places in the track, so the deck's tempo does not matter.
#[derive(Debug, Clone, Default)]
pub struct TapTempo {
    taps: Vec<f64>,
}

impl TapTempo {
    /// A tap heard at `seconds` into the track. Returns the tempo once enough taps agree.
    pub fn tap(&mut self, seconds: f64) -> Option<TappedTempo> {
        let restart = self
            .taps
            .last()
            .is_none_or(|last| seconds <= *last || seconds - last > RESTART_AFTER_SECONDS);
        if restart {
            self.taps.clear();
        }
        if self.taps.len() == TAPS_KEPT {
            self.taps.remove(0);
        }
        self.taps.push(seconds);
        if self.taps.len() < TAPS_NEEDED {
            return None;
        }
        let mut intervals: Vec<f64> = self.taps.windows(2).map(|pair| pair[1] - pair[0]).collect();
        intervals.sort_by(f64::total_cmp);
        let beat_seconds = intervals[intervals.len() / 2];
        let bpm = 60.0 / beat_seconds;
        if !TEMPO_RANGE_BPM.contains(&bpm) {
            return None;
        }
        Some(TappedTempo {
            bpm,
            first_beat_seconds: seconds.rem_euclid(beat_seconds),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_taps_give_the_tempo_and_where_the_beats_fall() {
        let mut taps = TapTempo::default();
        let beat = 60.0 / 122.0;
        let start = 10.2;
        assert_eq!(taps.tap(start), None);
        assert_eq!(taps.tap(start + beat), None);
        assert_eq!(taps.tap(start + 2.0 * beat), None);
        let tapped = taps.tap(start + 3.0 * beat).unwrap();
        assert!((tapped.bpm - 122.0).abs() < 1e-6, "{tapped:?}");
        let offset = (start - tapped.first_beat_seconds) / beat;
        assert!(
            (offset - offset.round()).abs() < 1e-6,
            "a tap falls on a beat"
        );
        assert!(tapped.first_beat_seconds < beat);
    }

    #[test]
    fn one_stray_tap_does_not_move_the_tempo_and_a_pause_starts_again() {
        let mut taps = TapTempo::default();
        let beat = 0.5;
        for index in 0..5 {
            taps.tap(f64::from(index) * beat);
        }
        let with_stray = taps.tap(2.5 + 0.12).unwrap();
        assert!((with_stray.bpm - 120.0).abs() < 1e-6, "{with_stray:?}");
        assert_eq!(taps.tap(10.0), None, "a long pause starts a new count");
        assert_eq!(
            taps.tap(9.0),
            None,
            "a tap behind the last one starts again"
        );
    }
}
