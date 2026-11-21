use dubplate_fx::BeatLength;

/// A loop on a deck, in track frames.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LoopRegion {
    pub(crate) start: f64,
    pub(crate) end: f64,
    /// Whether the playhead goes round the loop; an exited loop is kept for reloop.
    pub(crate) active: bool,
    /// The length in beats for a beat loop; `None` for a loop set by hand with loop in and out.
    pub(crate) length: Option<BeatLength>,
}

impl LoopRegion {
    pub(crate) fn length_frames(&self) -> f64 {
        self.end - self.start
    }

    /// Where the playhead belongs when one frame took it from `previous` to `position` across
    /// an end of the loop: as far into the loop from the other end. Only a playhead that was
    /// inside the loop goes round, so one entering it from outside plays straight in.
    pub(crate) fn wrap(&self, previous: f64, position: f64) -> Option<f64> {
        let length = self.length_frames();
        if length <= 0.0 {
            return None;
        }
        let inside_before = previous >= self.start && previous < self.end;
        if !inside_before {
            return None;
        }
        if position >= self.end {
            Some(self.start + (position - self.end).rem_euclid(length))
        } else if position < self.start {
            Some(self.end - (self.start - position).rem_euclid(length))
        } else {
            None
        }
    }

    /// Where a playhead at `position` lands inside the loop, as far from its start as it was,
    /// wrapped by the loop's length.
    pub(crate) fn fold(&self, position: f64) -> f64 {
        let length = self.length_frames();
        if length <= 0.0 || (position >= self.start && position < self.end) {
            position
        } else {
            self.start + (position - self.start).rem_euclid(length)
        }
    }
}

/// A deck's loop and the loop in point waiting for its loop out.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Looping {
    pub(crate) region: Option<LoopRegion>,
    pub(crate) pending_start: Option<f64>,
}

impl Looping {
    /// The loop the playhead goes round, if one is on.
    pub(crate) fn active_loop(&self) -> Option<LoopRegion> {
        self.region.filter(|region| region.active)
    }

    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region() -> LoopRegion {
        LoopRegion {
            start: 100.0,
            end: 200.0,
            active: true,
            length: None,
        }
    }

    #[test]
    fn the_playhead_goes_round_at_either_end() {
        let region = region();
        assert_eq!(region.wrap(199.5, 200.25), Some(100.25));
        assert_eq!(region.wrap(100.5, 99.5), Some(199.5));
        assert_eq!(region.wrap(150.0, 151.0), None);
    }

    #[test]
    fn a_playhead_coming_from_outside_plays_straight_in() {
        let region = region();
        assert_eq!(region.wrap(99.5, 100.5), None);
        assert_eq!(region.wrap(250.0, 251.0), None);
    }

    #[test]
    fn folding_keeps_the_distance_into_the_loop() {
        let region = region();
        assert_eq!(region.fold(230.0), 130.0);
        assert_eq!(region.fold(150.0), 150.0);
        assert_eq!(region.fold(90.0), 190.0);
    }
}
