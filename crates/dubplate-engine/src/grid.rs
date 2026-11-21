use std::sync::Arc;

use dubplate_fx::{MAX_TEMPO_BPM, MIN_TEMPO_BPM};

use crate::error::{EngineError, check_range};
use crate::track::MAX_TRACK_SECONDS;

/// Neighbouring beats whose tempos differ by less than this many BPM share one tempo segment.
/// rekordbox stores tempos in hundredths of a BPM.
const SAME_TEMPO_BPM: f64 = 0.005;

/// One beat of a beat grid as rekordbox analyses it: where it falls and the tempo from it on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridBeat {
    /// Where the beat falls, in seconds from the start of the track.
    pub seconds: f64,
    /// The tempo from this beat on, in beats per minute.
    pub bpm: f64,
}

/// A stretch of a grid at one tempo.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Segment {
    first_beat: f64,
    start_seconds: f64,
    beats_per_second: f64,
}

impl Segment {
    fn beat_at(&self, seconds: f64) -> f64 {
        self.first_beat + (seconds - self.start_seconds) * self.beats_per_second
    }

    fn seconds_at(&self, beat: f64) -> f64 {
        self.start_seconds + (beat - self.first_beat) / self.beats_per_second
    }
}

/// A track's beat grid at its own tempo: where every beat falls, with tempo changes.
///
/// Beats are counted from the grid's first beat, which is beat 0. Between beats the grid
/// moves evenly, so a grid with a tempo change per beat follows a live drummer exactly and a
/// grid at one tempo is a straight line. Cloning shares the beats instead of copying them.
#[derive(Debug, Clone, PartialEq)]
pub struct BeatGrid {
    segments: Arc<[Segment]>,
}

impl BeatGrid {
    /// A grid at one tempo of `bpm` beats per minute whose first beat falls
    /// `first_beat_seconds` into the track. The BPM must be one effects can follow, from
    /// [`MIN_TEMPO_BPM`] to [`MAX_TEMPO_BPM`].
    pub fn new(bpm: f64, first_beat_seconds: f64) -> Result<Self, EngineError> {
        Self::from_beats(&[GridBeat {
            seconds: first_beat_seconds,
            bpm,
        }])
    }

    /// A grid through every beat of `beats`, in order, as rekordbox stores it. Each beat's BPM
    /// holds until the next beat; the last one carries on to the end of the track.
    pub fn from_beats(beats: &[GridBeat]) -> Result<Self, EngineError> {
        let longest = MAX_TRACK_SECONDS as f64;
        let mut starts: Vec<(usize, GridBeat)> = Vec::new();
        let mut previous_seconds = f64::NEG_INFINITY;
        for (index, beat) in beats.iter().enumerate() {
            check_range("BPM", beat.bpm, MIN_TEMPO_BPM, MAX_TEMPO_BPM)?;
            check_range("beat time", beat.seconds, -longest, longest)?;
            if beat.seconds <= previous_seconds {
                return Err(EngineError::InvalidParameter {
                    name: "beat time after the previous beat",
                    value: beat.seconds,
                });
            }
            previous_seconds = beat.seconds;
            let same_tempo = starts
                .last()
                .is_some_and(|(_, start)| (start.bpm - beat.bpm).abs() < SAME_TEMPO_BPM);
            if !same_tempo {
                starts.push((index, *beat));
            }
        }
        if starts.is_empty() {
            return Err(EngineError::InvalidParameter {
                name: "beat count",
                value: 0.0,
            });
        }
        let segments = starts
            .iter()
            .enumerate()
            .map(|(position, (index, beat))| {
                let beats_per_second = match starts.get(position + 1) {
                    Some((next_index, next)) => {
                        (next_index - index) as f64 / (next.seconds - beat.seconds)
                    }
                    None => beat.bpm / 60.0,
                };
                Segment {
                    first_beat: *index as f64,
                    start_seconds: beat.seconds,
                    beats_per_second,
                }
            })
            .collect();
        Ok(Self { segments })
    }

    fn segment_at_seconds(&self, seconds: f64) -> &Segment {
        let following = self
            .segments
            .partition_point(|segment| segment.start_seconds <= seconds);
        &self.segments[following.saturating_sub(1)]
    }

    fn segment_at_beat(&self, beat: f64) -> &Segment {
        let following = self
            .segments
            .partition_point(|segment| segment.first_beat <= beat);
        &self.segments[following.saturating_sub(1)]
    }

    /// The tempo at the first beat, in beats per minute.
    pub fn bpm(&self) -> f64 {
        self.segments[0].beats_per_second * 60.0
    }

    /// The tempo at `seconds` into the track, in beats per minute.
    pub fn bpm_at(&self, seconds: f64) -> f64 {
        self.segment_at_seconds(seconds).beats_per_second * 60.0
    }

    /// Whether the tempo changes anywhere in the track.
    pub fn has_tempo_changes(&self) -> bool {
        self.segments.len() > 1
    }

    /// Where the first beat falls, in seconds from the start of the track.
    pub fn first_beat_seconds(&self) -> f64 {
        self.segments[0].start_seconds
    }

    /// Beats from the first beat to `seconds` into the track. The whole part counts beats and
    /// the fraction is the phase within the beat.
    pub fn beat_at(&self, seconds: f64) -> f64 {
        self.segment_at_seconds(seconds).beat_at(seconds)
    }

    /// Where beat `beat`, counted from the first beat and possibly fractional, falls in
    /// seconds from the start of the track.
    pub fn seconds_at(&self, beat: f64) -> f64 {
        self.segment_at_beat(beat).seconds_at(beat)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn beats(times_and_tempos: &[(f64, f64)]) -> Vec<GridBeat> {
        times_and_tempos
            .iter()
            .map(|(seconds, bpm)| GridBeat {
                seconds: *seconds,
                bpm: *bpm,
            })
            .collect()
    }

    #[test]
    fn a_grid_at_one_tempo_counts_beats_from_the_first_one() {
        let grid = BeatGrid::new(120.0, 0.5).unwrap();
        assert_eq!(grid.beat_at(0.5), 0.0);
        assert_eq!(grid.beat_at(2.5), 4.0);
        assert_eq!(grid.seconds_at(4.0), 2.5);
        assert_eq!(grid.beat_at(0.0), -1.0);
        assert_eq!(grid.bpm(), 120.0);
        assert!(!grid.has_tempo_changes());
    }

    #[test]
    fn a_grid_follows_its_tempo_changes_through_every_beat() {
        let grid = BeatGrid::from_beats(&beats(&[
            (1.0, 120.0),
            (1.5, 120.0),
            (2.0, 120.0),
            (2.5, 100.0),
            (3.1, 100.0),
        ]))
        .unwrap();
        assert!(grid.has_tempo_changes());
        assert_eq!(grid.beat_at(2.0), 2.0);
        assert_eq!(grid.beat_at(2.5), 3.0);
        assert!((grid.beat_at(3.1) - 4.0).abs() < 1e-12);
        assert!((grid.beat_at(3.7) - 5.0).abs() < 1e-12);
        assert_eq!(grid.bpm_at(1.2), 120.0);
        assert_eq!(grid.bpm_at(2.8), 100.0);
        for beat in [0.0, 1.5, 3.0, 3.25, 7.5] {
            assert!((grid.beat_at(grid.seconds_at(beat)) - beat).abs() < 1e-12);
        }
    }

    #[test]
    fn rounded_beat_times_keep_the_grid_continuous() {
        let grid = BeatGrid::from_beats(&beats(&[
            (0.0, 126.0),
            (0.476, 126.0),
            (0.952, 126.5),
            (1.426, 126.5),
        ]))
        .unwrap();
        let before = grid.beat_at(0.952 - 1e-9);
        let after = grid.beat_at(0.952);
        assert!((after - before).abs() < 1e-6);
        assert_eq!(after, 2.0);
    }

    #[test]
    fn bad_grids_are_rejected() {
        assert!(BeatGrid::new(0.0, 0.0).is_err());
        assert!(BeatGrid::new(f64::NAN, 0.0).is_err());
        assert!(BeatGrid::new(128.0, f64::INFINITY).is_err());
        assert!(BeatGrid::from_beats(&[]).is_err());
        assert!(BeatGrid::from_beats(&beats(&[(1.0, 120.0), (1.0, 120.0)])).is_err());
    }
}
