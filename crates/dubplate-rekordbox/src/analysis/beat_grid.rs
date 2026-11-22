use serde::Serialize;

use super::error::AnalysisError;
use super::reader::SectionReader;

/// One beat of a beat grid, as rekordbox stores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Beat {
    /// Where the beat falls in its bar, from 1 (the downbeat) to 4.
    pub beat_in_bar: u16,
    /// The tempo at this beat in hundredths of a BPM, so `12200` is 122.00 BPM.
    pub tempo_hundredths: u16,
    /// When the beat plays, in milliseconds from the start of the track at normal speed.
    pub time_ms: u32,
}

impl Beat {
    /// The tempo at this beat in beats per minute.
    pub fn bpm(&self) -> f64 {
        f64::from(self.tempo_hundredths) / 100.0
    }

    /// Whether this beat starts a bar.
    pub fn is_downbeat(&self) -> bool {
        self.beat_in_bar == 1
    }
}

/// Where a moment in the track sits on the beat grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct GridPosition {
    /// The nearest beat, counted from 1 like rekordbox's phrase and beat numbers.
    pub beat_number: u32,
    /// The bar of that beat, counted from 1 at the first downbeat. Beats before the first
    /// downbeat are in bar 0.
    pub bar: u32,
    /// Where that beat falls in its bar, from 1 to 4.
    pub beat_in_bar: u16,
    /// How far the moment is from that beat, in milliseconds. Negative means before it.
    pub offset_ms: i64,
}

/// Every beat rekordbox found in a track, from the `PQTZ` section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BeatGrid {
    /// The beats in playing order.
    pub beats: Vec<Beat>,
}

impl BeatGrid {
    /// The first beat that starts a bar.
    pub fn first_downbeat(&self) -> Option<&Beat> {
        self.downbeats().next()
    }

    /// Where the first beat that starts a bar is in `beats`.
    pub fn first_downbeat_index(&self) -> Option<usize> {
        self.beats.iter().position(Beat::is_downbeat)
    }

    /// Every beat that starts a bar, in playing order.
    pub fn downbeats(&self) -> impl Iterator<Item = &Beat> {
        self.beats.iter().filter(|beat| beat.is_downbeat())
    }

    /// The lowest and highest tempo on the grid, in hundredths of a BPM.
    pub fn tempo_range_hundredths(&self) -> Option<(u16, u16)> {
        let lowest = self.beats.iter().map(|beat| beat.tempo_hundredths).min()?;
        let highest = self.beats.iter().map(|beat| beat.tempo_hundredths).max()?;
        Some((lowest, highest))
    }

    /// The beat with rekordbox's 1-based beat number, as used by phrases.
    pub fn beat(&self, beat_number: u32) -> Option<&Beat> {
        let index = usize::try_from(beat_number.checked_sub(1)?).ok()?;
        self.beats.get(index)
    }

    /// The bar that the beat with this 1-based number falls in. Beats before the first
    /// downbeat are in bar 0.
    pub fn bar_of_beat(&self, beat_number: u32) -> Option<u32> {
        let index = usize::try_from(beat_number.checked_sub(1)?).ok()?;
        let beats_so_far = self.beats.get(..=index)?;
        let downbeats = beats_so_far
            .iter()
            .filter(|beat| beat.is_downbeat())
            .count();
        u32::try_from(downbeats).ok()
    }

    /// The beat nearest to `time_ms`, with the bar it is in.
    pub fn position_of(&self, time_ms: u32) -> Option<GridPosition> {
        let following_index = self.beats.partition_point(|beat| beat.time_ms <= time_ms);
        let nearest_index = match following_index {
            0 => 0,
            index if index == self.beats.len() => index - 1,
            index => {
                let before = time_ms.abs_diff(self.beats[index - 1].time_ms);
                let after = time_ms.abs_diff(self.beats[index].time_ms);
                if after < before { index } else { index - 1 }
            }
        };
        let beat = self.beats.get(nearest_index)?;
        let beat_number = u32::try_from(nearest_index + 1).ok()?;
        Some(GridPosition {
            beat_number,
            bar: self.bar_of_beat(beat_number)?,
            beat_in_bar: beat.beat_in_bar,
            offset_ms: i64::from(time_ms) - i64::from(beat.time_ms),
        })
    }
}

const BEAT_COUNT_POSITION: usize = 0x14;
const FIRST_BEAT_POSITION: usize = 0x18;
const BEAT_SIZE: usize = 8;

pub(crate) fn parse(reader: &SectionReader<'_>) -> Result<BeatGrid, AnalysisError> {
    let count = reader.u32_at(BEAT_COUNT_POSITION, "the beat count")?;
    let entries = reader.expect_entries(FIRST_BEAT_POSITION, count, BEAT_SIZE, "beats")?;
    let beats = entries
        .as_chunks::<BEAT_SIZE>()
        .0
        .iter()
        .map(|entry| Beat {
            beat_in_bar: u16::from_be_bytes([entry[0], entry[1]]),
            tempo_hundredths: u16::from_be_bytes([entry[2], entry[3]]),
            time_ms: u32::from_be_bytes([entry[4], entry[5], entry[6], entry[7]]),
        })
        .collect();
    Ok(BeatGrid { beats })
}
