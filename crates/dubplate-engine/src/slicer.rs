use crate::grid::BeatGrid;
use crate::looping::LoopRegion;

/// The length of the slicer's domain, in beats. The domain is cut into 8 equal slices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u64)]
pub enum SlicerDomain {
    /// 4 beats: one bar.
    Four = 4,
    /// 8 beats: the default.
    Eight = 8,
    /// 16 beats.
    Sixteen = 16,
    /// 32 beats.
    ThirtyTwo = 32,
    /// 64 beats.
    SixtyFour = 64,
}

impl SlicerDomain {
    /// Every domain, smallest first.
    pub const ALL: [SlicerDomain; 5] = [
        SlicerDomain::Four,
        SlicerDomain::Eight,
        SlicerDomain::Sixteen,
        SlicerDomain::ThirtyTwo,
        SlicerDomain::SixtyFour,
    ];

    /// The domain in beats.
    pub const fn beats(self) -> f64 {
        (self as u64) as f64
    }
}

/// The length of a slicer pad roll, as a share of the domain: 1/8 to 1/64.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SlicerRoll {
    /// 1/8 of the domain.
    Eighth,
    /// 1/16 of the domain.
    Sixteenth,
    /// 1/32 of the domain.
    ThirtySecond,
    /// 1/64 of the domain.
    SixtyFourth,
}

impl SlicerRoll {
    /// Every roll length, longest first.
    pub const ALL: [SlicerRoll; 4] = [
        SlicerRoll::Eighth,
        SlicerRoll::Sixteenth,
        SlicerRoll::ThirtySecond,
        SlicerRoll::SixtyFourth,
    ];

    /// The share of the domain the roll repeats.
    pub const fn fraction(self) -> f64 {
        match self {
            SlicerRoll::Eighth => 1.0 / 8.0,
            SlicerRoll::Sixteenth => 1.0 / 16.0,
            SlicerRoll::ThirtySecond => 1.0 / 32.0,
            SlicerRoll::SixtyFourth => 1.0 / 64.0,
        }
    }
}

/// A slice of the slicer's domain, counted from the domain's downbeat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SlicerSlice(u8);

impl SlicerSlice {
    /// The number of slices in a domain.
    pub const COUNT: u8 = 8;

    /// The first slice, the one the domain's downbeat starts.
    pub const FIRST: SlicerSlice = SlicerSlice(0);

    /// The slice `index` when it is under 8.
    pub const fn from_index(index: u8) -> Option<Self> {
        if index < Self::COUNT {
            Some(Self(index))
        } else {
            None
        }
    }

    /// The slice's index, counted from 0.
    pub const fn index(self) -> u8 {
        self.0
    }
}

/// The slicer's mode, as on a rekordbox's SLICER button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum SlicerMode {
    /// The domain plays through, then moves forward to the next domain with the music.
    #[default]
    Slicer,
    /// The domain loops: reaching its end wraps to its start, so the slices repeat.
    SlicerLoop,
}

impl SlicerMode {
    pub(crate) const fn index(self) -> u8 {
        match self {
            SlicerMode::Slicer => 0,
            SlicerMode::SlicerLoop => 1,
        }
    }
}

/// What one frame did to the slicer: the deck jumps to `jump` when a held pad's slice plays in,
/// and the loop it goes round changes when the domain moves.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct SlicerFrame {
    pub(crate) jump: Option<f64>,
    pub(crate) domain_moved: bool,
}

/// The domain of `beats` beats on the grid's beats that holds `frame`, from its start beat to
/// its end, in track frames and cut at the track's end.
pub(crate) fn domain_around(
    grid: &BeatGrid,
    source_rate: f64,
    frame: f64,
    beats: f64,
    end_frame: Option<f64>,
) -> (f64, f64) {
    let start_beat = (grid.beat_at(frame / source_rate) / beats).floor() * beats;
    let to_frame = |beat: f64| {
        let frame = grid.seconds_at(beat) * source_rate;
        end_frame.map_or(frame, |end| frame.min(end))
    };
    (to_frame(start_beat).max(0.0), to_frame(start_beat + beats))
}

/// The slicer on a deck, in track frames: the domain, the held pad, and the shadow playhead
/// the track keeps playing underneath while a pad holds.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Slicer {
    enabled: bool,
    mode: SlicerMode,
    domain_beats: f64,
    start: f64,
    end: f64,
    pad: Option<SlicerSlice>,
    roll: Option<SlicerRoll>,
    shadow: Option<f64>,
    /// The held pad's slice has not played in yet: it does at the next slice boundary.
    pending_jump: bool,
}

impl Slicer {
    /// The slicer on, the domain running from `start` to `end`, in track frames.
    pub(crate) fn start(&mut self, mode: SlicerMode, domain: SlicerDomain, start: f64, end: f64) {
        self.enabled = true;
        self.mode = mode;
        self.domain_beats = domain.beats();
        self.start = start;
        self.end = end;
        self.pad = None;
        self.roll = None;
        self.shadow = None;
        self.pending_jump = false;
    }

    /// The slicer off: any held pad lets go. Returns where the deck lands: where the track
    /// would be now, when a pad still holds.
    pub(crate) fn stop(&mut self) -> Option<f64> {
        self.enabled = false;
        if self.pad.is_some() {
            let landing = self.shadow.take();
            self.pad = None;
            self.roll = None;
            self.pending_jump = false;
            landing
        } else {
            None
        }
    }

    pub(crate) fn enabled(&self) -> bool {
        self.enabled
    }

    pub(crate) fn mode(&self) -> SlicerMode {
        self.mode
    }

    /// The domain's start and end, in track frames.
    pub(crate) fn domain_frames(&self) -> (f64, f64) {
        (self.start, self.end)
    }

    /// The length of one slice, in track frames.
    fn slice_frames(&self) -> f64 {
        (self.end - self.start) / f64::from(SlicerSlice::COUNT)
    }

    /// The frame slice `slice` starts on.
    pub(crate) fn slice_start(&self, slice: SlicerSlice) -> f64 {
        self.start + f64::from(slice.0) * self.slice_frames()
    }

    /// Which slice `frame` falls in, when the slicer is on and it is inside the domain.
    pub(crate) fn slice_at(&self, frame: f64) -> Option<SlicerSlice> {
        if !self.enabled {
            return None;
        }
        let offset = frame - self.start;
        let inside = offset >= 0.0 && offset < self.end - self.start;
        inside.then(|| {
            SlicerSlice::from_index((offset / self.slice_frames()) as u8)
                .unwrap_or(SlicerSlice::FIRST)
        })
    }

    /// The slice boundary the shadow crossed going from `previous` to `next`, if any: that
    /// is when a held pad's slice plays in.
    pub(crate) fn crossed_boundary(&self, previous: f64, next: f64) -> Option<f64> {
        let slice = self.slice_frames();
        let index = (next - self.start) / slice;
        let boundary = self.start + index.floor() * slice;
        (boundary > previous && boundary <= next).then_some(boundary)
    }

    /// Presses pad `slice` with an optional roll, the shadow starting at `position`: the
    /// track keeps playing underneath and the slice plays in at the next slice boundary.
    pub(crate) fn press(&mut self, slice: SlicerSlice, roll: Option<SlicerRoll>, position: f64) {
        if !self.enabled {
            return;
        }
        self.shadow = self.shadow.or(Some(position));
        self.pad = Some(slice);
        self.roll = roll;
        self.pending_jump = true;
    }

    /// Lets go of pad `slice`. Returns where the deck lands, where the track would be now,
    /// when it was the pad held.
    pub(crate) fn release(&mut self, slice: SlicerSlice) -> Option<f64> {
        if self.pad != Some(slice) {
            return None;
        }
        self.pad = None;
        self.roll = None;
        self.pending_jump = false;
        self.shadow.take()
    }

    #[cfg(test)]
    fn shadow(&self) -> Option<f64> {
        self.shadow
    }

    /// Moves the slicer on by one frame, the track at `position` before it. The shadow
    /// playhead, while a pad holds, moves by `motor_step`, wrapping on the domain's end in
    /// slicer loop mode. The domain moves forward with the music once the track leaves it,
    /// and a held pad's slice plays in at the next slice boundary: the frame it does is
    /// reported, with where the deck jumps to.
    pub(crate) fn advance(
        &mut self,
        position: f64,
        step: f64,
        motor_step: f64,
        end_frame: Option<f64>,
        grid: Option<&BeatGrid>,
        source_rate: f64,
    ) -> SlicerFrame {
        let mut frame = SlicerFrame::default();
        if !self.enabled {
            return frame;
        }
        let before = self.shadow.unwrap_or(position);
        let mut wrapped = false;
        if let Some(shadow) = &mut self.shadow {
            *shadow += motor_step;
            if self.mode == SlicerMode::SlicerLoop {
                let length = self.end - self.start;
                if length > 0.0 && *shadow >= self.end {
                    *shadow = self.start + (*shadow - self.start).rem_euclid(length);
                    wrapped = true;
                }
            } else if let Some(end) = end_frame {
                *shadow = shadow.min(end);
            }
        }
        let reference = self.shadow.unwrap_or(position + step);
        let left_domain = reference >= self.end || reference < self.start;
        if self.mode == SlicerMode::Slicer
            && left_domain
            && let Some(grid) = grid
        {
            let (start, end) =
                domain_around(grid, source_rate, reference, self.domain_beats, end_frame);
            if end > start {
                self.start = start;
                self.end = end;
                self.pending_jump = self.pad.is_some();
                frame.domain_moved = true;
            }
        }
        if self.pending_jump
            && let Some(pad) = self.pad
            && let Some(shadow) = self.shadow
            && let Some(boundary) = wrapped
                .then_some(self.start)
                .or_else(|| self.crossed_boundary(before, shadow))
        {
            self.pending_jump = false;
            frame.jump = Some(self.slice_start(pad) + (shadow - boundary));
        }
        frame
    }

    /// The loop the playhead goes round while the slicer is on: the held pad's slice, cut to
    /// the roll's length with a roll, or the whole domain in slicer loop mode.
    pub(crate) fn active_region(&self) -> Option<LoopRegion> {
        if !self.enabled {
            return None;
        }
        if let Some(pad) = self.pad {
            let start = self.slice_start(pad);
            let length = self.roll.map_or(self.slice_frames(), |roll| {
                (self.end - self.start) * roll.fraction()
            });
            return Some(LoopRegion {
                start,
                end: start + length,
                active: true,
                length: None,
            });
        }
        (self.mode == SlicerMode::SlicerLoop).then_some(LoopRegion {
            start: self.start,
            end: self.end,
            active: true,
            length: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_domain_sits_on_its_downbeats_and_the_slices_divide_it() {
        let mut slicer = Slicer::default();
        slicer.start(SlicerMode::Slicer, SlicerDomain::Eight, 800.0, 1200.0);
        assert_eq!(slicer.domain_frames(), (800.0, 1200.0));
        assert_eq!(slicer.slice_at(800.0), Some(SlicerSlice::FIRST));
        assert_eq!(
            slicer.slice_at(1049.0),
            Some(SlicerSlice::from_index(4).unwrap())
        );
        assert_eq!(slicer.slice_at(1200.0), None);
        assert_eq!(
            slicer.slice_start(SlicerSlice::from_index(3).unwrap()),
            950.0
        );
    }

    #[test]
    fn the_boundary_a_frame_crosses_is_the_first_one_past_its_start() {
        let mut slicer = Slicer::default();
        slicer.start(SlicerMode::Slicer, SlicerDomain::Eight, 800.0, 1200.0);
        assert_eq!(slicer.crossed_boundary(1030.0, 1049.0), None);
        assert_eq!(slicer.crossed_boundary(1030.0, 1050.0), Some(1050.0));
        assert_eq!(slicer.crossed_boundary(1060.0, 1099.0), None);
        assert_eq!(slicer.crossed_boundary(1060.0, 1100.0), Some(1100.0));
        assert_eq!(slicer.crossed_boundary(1050.0, 1050.0), None);
    }

    #[test]
    fn the_slicer_loop_shadow_wraps_on_the_domains_end() {
        let mut slicer = Slicer::default();
        slicer.start(SlicerMode::SlicerLoop, SlicerDomain::Eight, 800.0, 1200.0);
        slicer.press(SlicerSlice::FIRST, None, 1190.0);
        slicer.advance(30.0, 0.0, 30.0, None, None, 1.0);
        assert_eq!(slicer.shadow(), Some(820.0));
    }

    #[test]
    fn a_held_pad_goes_round_its_slice_and_a_roll_cuts_it_to_its_length() {
        let mut slicer = Slicer::default();
        slicer.start(SlicerMode::Slicer, SlicerDomain::Eight, 800.0, 1200.0);
        slicer.press(SlicerSlice::from_index(2).unwrap(), None, 1030.0);
        assert_eq!(
            slicer
                .active_region()
                .map(|region| (region.start, region.end)),
            Some((900.0, 950.0))
        );
        slicer.press(
            SlicerSlice::from_index(2).unwrap(),
            Some(SlicerRoll::Sixteenth),
            1030.0,
        );
        assert_eq!(
            slicer
                .active_region()
                .map(|region| (region.start, region.end)),
            Some((900.0, 925.0))
        );
    }

    #[test]
    fn releasing_lands_on_the_shadow() {
        let mut slicer = Slicer::default();
        slicer.start(SlicerMode::Slicer, SlicerDomain::Eight, 800.0, 1200.0);
        slicer.press(SlicerSlice::FIRST, None, 900.0);
        slicer.advance(100.0, 0.0, 100.0, None, None, 1.0);
        assert_eq!(slicer.release(SlicerSlice::FIRST), Some(1000.0));
        assert_eq!(slicer.release(SlicerSlice::FIRST), None);
        assert!(slicer.enabled());
        slicer.stop();
        assert!(!slicer.enabled());
    }

    #[test]
    fn stopping_while_holding_lands_on_the_shadow_and_clears_the_pad() {
        let mut slicer = Slicer::default();
        slicer.start(SlicerMode::Slicer, SlicerDomain::Eight, 800.0, 1200.0);
        slicer.press(SlicerSlice::FIRST, None, 900.0);
        slicer.advance(50.0, 0.0, 50.0, None, None, 1.0);
        assert_eq!(slicer.stop(), Some(950.0));
        assert!(!slicer.enabled());
        assert_eq!(slicer.shadow(), None);
    }
}
