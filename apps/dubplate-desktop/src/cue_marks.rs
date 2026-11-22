//! Noticing a cue the moment it is set, from the CUE button or a hot cue pad, so the lanes and
//! the overview can pulse it where it landed.

use std::time::{Duration, Instant};

use crate::deck::HotCues;

/// How long a newly set cue pulses.
pub const PULSE_DURATION: Duration = Duration::from_millis(700);

/// The cues of one deck as last seen, to notice the one just set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SeenMarks {
    /// The load the cues belong to, so loading a track never counts as setting a cue.
    pub load_generation: u64,
    /// The CUE button's point, in seconds.
    pub cue_point: f64,
    /// The hot cues.
    pub hot_cues: HotCues,
}

impl SeenMarks {
    /// Where a cue was set between `self` and `now`, in seconds, if one was: a hot cue that
    /// appeared or moved first, else a new cue point.
    pub fn newly_set(&self, now: &Self) -> Option<f64> {
        if self.load_generation != now.load_generation {
            return None;
        }
        let new_hot_cue = now
            .hot_cues
            .iter()
            .find(|(pad, seconds)| self.hot_cues.get(*pad) != Some(*seconds))
            .map(|(_, seconds)| seconds);
        new_hot_cue.or_else(|| {
            ((now.cue_point - self.cue_point).abs() > f64::EPSILON).then_some(now.cue_point)
        })
    }
}

/// A cue pulsing where it was just set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MarkerPulse {
    /// Where the cue is, in seconds.
    pub seconds: f64,
    /// When it was set.
    pub started: Instant,
}

impl MarkerPulse {
    /// How strong the pulse is at `now`, from 1.0 when set to 0.0 once it is over, or `None`
    /// when it is over.
    pub fn strength(&self, now: Instant) -> Option<f32> {
        let elapsed = now.saturating_duration_since(self.started);
        (elapsed < PULSE_DURATION)
            .then(|| 1.0 - elapsed.as_secs_f32() / PULSE_DURATION.as_secs_f32())
    }
}

#[cfg(test)]
mod tests {
    use dubplate_rekordbox::HotCuePad;

    use super::*;

    fn seen(load_generation: u64, cue_point: f64, hot_cues: HotCues) -> SeenMarks {
        SeenMarks {
            load_generation,
            cue_point,
            hot_cues,
        }
    }

    #[test]
    fn a_new_hot_cue_or_cue_point_is_noticed_but_a_new_track_is_not() {
        let before = seen(1, 2.0, HotCues::default());
        assert_eq!(before.newly_set(&before), None);
        let mut with_pad = HotCues::default();
        with_pad.set(HotCuePad::C, 31.5);
        assert_eq!(before.newly_set(&seen(1, 2.0, with_pad)), Some(31.5));
        assert_eq!(
            before.newly_set(&seen(1, 16.0, HotCues::default())),
            Some(16.0)
        );
        assert_eq!(
            before.newly_set(&seen(2, 16.0, with_pad)),
            None,
            "a new load is not a cue being set"
        );
    }

    #[test]
    fn a_pulse_fades_out() {
        let started = Instant::now();
        let pulse = MarkerPulse {
            seconds: 4.0,
            started,
        };
        assert_eq!(pulse.strength(started), Some(1.0));
        let halfway = pulse.strength(started + PULSE_DURATION / 2).unwrap();
        assert!((halfway - 0.5).abs() < 1e-3, "{halfway}");
        assert_eq!(pulse.strength(started + PULSE_DURATION), None);
    }
}
