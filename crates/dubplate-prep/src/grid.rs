//! The DJ's fix of rekordbox's beat grid, kept per set track.

use dubplate_rekordbox::analysis::{Beat, BeatGrid};

use crate::store::SetEntry;

/// Beats in a bar: the range a downbeat shift goes round.
pub const BEATS_PER_BAR: i32 = 4;

/// The DJ's fix of rekordbox's beat grid for one track, kept in the set store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GridCorrection {
    /// How far every beat moves, in milliseconds, later when positive.
    pub offset_ms: i32,
    /// How many beats the bars move, later when positive.
    pub downbeat_shift: i32,
}

impl GridCorrection {
    /// The correction a set entry keeps.
    pub fn of(entry: &SetEntry) -> Self {
        Self {
            offset_ms: entry.grid_offset_ms,
            downbeat_shift: entry.downbeat_shift,
        }
    }

    /// rekordbox's grid with this correction: every beat moved by the offset, beats that would
    /// fall before the start left out, and the bars counted from the shifted downbeat.
    pub fn apply(self, grid: &BeatGrid) -> BeatGrid {
        let beats = grid
            .beats
            .iter()
            .filter_map(|beat| {
                let time_ms = i64::from(beat.time_ms) + i64::from(self.offset_ms);
                let in_bar = (i32::from(beat.beat_in_bar) - 1 - self.downbeat_shift)
                    .rem_euclid(BEATS_PER_BAR)
                    + 1;
                Some(Beat {
                    time_ms: u32::try_from(time_ms).ok()?,
                    beat_in_bar: u16::try_from(in_bar).ok()?,
                    ..*beat
                })
            })
            .collect();
        BeatGrid { beats }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn beat(time_ms: u32, beat_in_bar: u16) -> Beat {
        Beat {
            beat_in_bar,
            tempo_hundredths: 12_000,
            time_ms,
        }
    }

    #[test]
    fn a_correction_moves_the_beats_and_the_bars() {
        let grid = BeatGrid {
            beats: vec![
                beat(0, 1),
                beat(500, 2),
                beat(1000, 3),
                beat(1500, 4),
                beat(2000, 1),
            ],
        };
        let later = GridCorrection {
            offset_ms: 20,
            downbeat_shift: 1,
        }
        .apply(&grid);
        let shown: Vec<(u32, u16)> = later
            .beats
            .iter()
            .map(|beat| (beat.time_ms, beat.beat_in_bar))
            .collect();
        assert_eq!(shown, [(20, 4), (520, 1), (1020, 2), (1520, 3), (2020, 4)]);
        let earlier = GridCorrection {
            offset_ms: -10,
            downbeat_shift: -1,
        }
        .apply(&grid);
        assert_eq!(
            earlier.beats.len(),
            4,
            "a beat moved before the start is left out"
        );
        assert_eq!(
            (earlier.beats[0].time_ms, earlier.beats[0].beat_in_bar),
            (490, 3)
        );
    }
}
