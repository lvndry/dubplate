//! The DJ's fix of rekordbox's beat grid, kept per track file.

use dubplate_rekordbox::analysis::{Beat, BeatGrid};

use crate::store::SetEntry;

/// Beats in a bar: the range a downbeat shift goes round.
pub const BEATS_PER_BAR: i32 = 4;

/// The DJ's fix of rekordbox's beat grid for one track.
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

    /// This correction with the grid moved `milliseconds` more, later when positive.
    #[must_use]
    pub fn nudged(self, milliseconds: i32) -> Self {
        Self {
            offset_ms: self.offset_ms.saturating_add(milliseconds),
            ..self
        }
    }

    /// This correction with the bars moved `beats` more, later when positive.
    #[must_use]
    pub fn downbeat_shifted(self, beats: i32) -> Self {
        Self {
            downbeat_shift: (self.downbeat_shift + beats).rem_euclid(BEATS_PER_BAR),
            ..self
        }
    }
}

/// One change the DJ makes to a track's grid, from the track check or a deck.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridEdit {
    /// Confirm the grid by ear, or take the confirmation back.
    ToggleConfirmed,
    /// Move the grid by milliseconds, later when positive.
    Nudge(i32),
    /// Move the bars by beats, later when positive.
    ShiftDownbeat(i32),
}

/// A track's grid as the DJ left it: the correction and whether it was checked by ear. Kept
/// once per track file, so a set, a deck and an export all read the same one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TrackGrid {
    /// How the grid is moved from rekordbox's.
    pub correction: GridCorrection,
    /// Whether the DJ checked the grid by ear since it last moved.
    pub confirmed: bool,
}

impl TrackGrid {
    /// The grid a set entry carries.
    pub fn of(entry: &SetEntry) -> Self {
        Self {
            correction: GridCorrection::of(entry),
            confirmed: entry.grid_confirmed,
        }
    }

    /// This grid moved `milliseconds`, which needs confirming again.
    #[must_use]
    pub fn nudged(self, milliseconds: i32) -> Self {
        Self {
            correction: self.correction.nudged(milliseconds),
            confirmed: false,
        }
    }

    /// This grid with its bars moved `beats`, which needs confirming again.
    #[must_use]
    pub fn downbeat_shifted(self, beats: i32) -> Self {
        Self {
            correction: self.correction.downbeat_shifted(beats),
            confirmed: false,
        }
    }

    /// This grid checked by ear, or no longer.
    #[must_use]
    pub fn confirmation_toggled(self) -> Self {
        Self {
            confirmed: !self.confirmed,
            ..self
        }
    }

    /// This grid with `edit` made to it.
    #[must_use]
    pub fn edited(self, edit: GridEdit) -> Self {
        match edit {
            GridEdit::ToggleConfirmed => self.confirmation_toggled(),
            GridEdit::Nudge(milliseconds) => self.nudged(milliseconds),
            GridEdit::ShiftDownbeat(beats) => self.downbeat_shifted(beats),
        }
    }

    /// Writes this grid into a set entry.
    pub fn write_to(self, entry: &mut SetEntry) {
        entry.grid_offset_ms = self.correction.offset_ms;
        entry.downbeat_shift = self.correction.downbeat_shift;
        entry.grid_confirmed = self.confirmed;
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
