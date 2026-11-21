//! Turns rekordbox's waveforms and beat grid into what the screen draws: band levels per screen
//! column, worked out once per track and zoom, and where each column and beat lands for a
//! playhead position.

use std::ops::Range;

use dubplate_rekordbox::analysis::{
    BeatGrid, ColorDetailColumn, DETAIL_COLUMNS_PER_SECOND, MonochromeColumn, ThreeBandColumn,
    Waveforms,
};

/// The level of each band in one column, from 0.0 to 1.0.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BandLevels {
    /// The lows: kick and bass.
    pub low: f32,
    /// The mids.
    pub mid: f32,
    /// The highs: hats and air.
    pub high: f32,
}

impl BandLevels {
    /// The highest of the three levels.
    pub fn loudest(self) -> f32 {
        self.low.max(self.mid).max(self.high)
    }

    fn max(self, other: Self) -> Self {
        Self {
            low: self.low.max(other.low),
            mid: self.mid.max(other.mid),
            high: self.high.max(other.high),
        }
    }

    fn scaled(self, factor: f32) -> Self {
        Self {
            low: (self.low * factor).min(1.0),
            mid: (self.mid * factor).min(1.0),
            high: (self.high * factor).min(1.0),
        }
    }
}

/// The horizontal scale of the zoomed waveforms.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Zoom {
    pixels_per_second: f64,
}

impl Zoom {
    /// One screen column for each column rekordbox analysed: 150 a second, about nine seconds
    /// across a laptop screen.
    pub const DEFAULT: Self = Self {
        pixels_per_second: DETAIL_COLUMNS_PER_SECOND as f64,
    };

    /// A zoom of `pixels_per_second`, at least one pixel a second.
    #[cfg(test)]
    pub fn new(pixels_per_second: f64) -> Self {
        Self {
            pixels_per_second: pixels_per_second.max(1.0),
        }
    }

    /// Screen pixels per second of the track.
    pub fn pixels_per_second(self) -> f64 {
        self.pixels_per_second
    }
}

/// A track's scrolling waveform at one zoom: one [`BandLevels`] per screen column.
#[derive(Debug, Clone, PartialEq)]
pub struct ZoomedWaveform {
    columns: Vec<BandLevels>,
    zoom: Zoom,
}

impl ZoomedWaveform {
    /// The best scrolling waveform rekordbox saved, 3-band first, then colour, then blue, at
    /// `zoom`. `None` when the track has none of them.
    pub fn from_waveforms(waveforms: &Waveforms, zoom: Zoom) -> Option<Self> {
        let levels = detail_levels(waveforms)?;
        Some(Self {
            columns: resample(
                &normalized(levels),
                f64::from(DETAIL_COLUMNS_PER_SECOND),
                zoom.pixels_per_second(),
            ),
            zoom,
        })
    }

    /// One entry per screen column, from the start of the track.
    pub fn columns(&self) -> &[BandLevels] {
        &self.columns
    }

    /// The zoom the columns were made for.
    pub fn zoom(&self) -> Zoom {
        self.zoom
    }
}

/// A track's whole-track waveform as one level per column, from 0.0 to 1.0.
#[derive(Debug, Clone, PartialEq)]
pub struct OverviewWaveform {
    levels: Vec<f32>,
}

impl OverviewWaveform {
    /// The best overview rekordbox saved, 3-band first, then colour, then blue, else the
    /// scrolling waveform squeezed. `None` when the track has no waveform.
    pub fn from_waveforms(waveforms: &Waveforms) -> Option<Self> {
        let levels: Vec<BandLevels> = if let Some(columns) = &waveforms.three_band_preview {
            columns.iter().map(three_band_levels).collect()
        } else if let Some(columns) = &waveforms.color_preview {
            columns
                .iter()
                .map(|column| BandLevels {
                    low: f32::from(column.0[3]),
                    mid: f32::from(column.0[4]),
                    high: f32::from(column.0[5]),
                })
                .collect()
        } else if let Some(columns) = &waveforms.preview {
            columns.iter().map(monochrome_levels).collect()
        } else {
            detail_levels(waveforms)?
        };
        Some(Self {
            levels: normalized(levels)
                .into_iter()
                .map(BandLevels::loudest)
                .collect(),
        })
    }

    /// The level of each of `bar_count` bars across the whole track: the loudest column under
    /// each bar.
    pub fn bars(&self, bar_count: usize) -> Vec<f32> {
        (0..bar_count)
            .map(|bar| {
                let columns = covered_columns(bar, bar_count, self.levels.len());
                self.levels[columns].iter().copied().fold(0.0, f32::max)
            })
            .collect()
    }
}

/// The columns of `column_count` that bar `bar` of `bar_count` covers, never empty.
fn covered_columns(bar: usize, bar_count: usize, column_count: usize) -> Range<usize> {
    let start = bar * column_count / bar_count.max(1);
    let end = ((bar + 1) * column_count / bar_count.max(1)).max(start + 1);
    start.min(column_count.saturating_sub(1))..end.min(column_count)
}

/// The scrolling waveform's levels: 3-band (`PWV7`) first, then colour (`PWV5`), then blue
/// (`PWV3`).
fn detail_levels(waveforms: &Waveforms) -> Option<Vec<BandLevels>> {
    if let Some(columns) = &waveforms.three_band_detail {
        return Some(columns.iter().map(three_band_levels).collect());
    }
    if let Some(columns) = &waveforms.color_detail {
        return Some(columns.iter().copied().map(colour_levels).collect());
    }
    let columns = waveforms.detail.as_ref()?;
    Some(columns.iter().map(monochrome_levels).collect())
}

fn three_band_levels(column: &ThreeBandColumn) -> BandLevels {
    BandLevels {
        low: f32::from(column.low),
        mid: f32::from(column.mid),
        high: f32::from(column.high),
    }
}

/// The colour waveform gives a height and a colour whose red, green and blue are the share of
/// the lows, mids and highs; the loudest band gets the full height.
fn colour_levels(column: ColorDetailColumn) -> BandLevels {
    let height = f32::from(column.height());
    let strongest = column.red().max(column.green()).max(column.blue()).max(1);
    let share = |channel: u8| height * f32::from(channel) / f32::from(strongest);
    BandLevels {
        low: share(column.red()),
        mid: share(column.green()),
        high: share(column.blue()),
    }
}

/// The blue waveform has one height, drawn in the low band's colour.
fn monochrome_levels(column: &MonochromeColumn) -> BandLevels {
    BandLevels {
        low: f32::from(column.height()),
        mid: 0.0,
        high: 0.0,
    }
}

/// Scales levels so the loudest band of the loudest column reaches 1.0.
fn normalized(levels: Vec<BandLevels>) -> Vec<BandLevels> {
    let peak = levels
        .iter()
        .map(|column| column.loudest())
        .fold(0.0, f32::max);
    if peak <= 0.0 {
        return levels;
    }
    levels
        .into_iter()
        .map(|column| column.scaled(1.0 / peak))
        .collect()
}

/// Resamples columns taken at `source_rate` a second to `target_rate` a second: the loudest
/// source column under each target column when shrinking, the nearest one when stretching.
fn resample(columns: &[BandLevels], source_rate: f64, target_rate: f64) -> Vec<BandLevels> {
    let duration_seconds = columns.len() as f64 / source_rate;
    let target_count = (duration_seconds * target_rate).round() as usize;
    let sources_per_target = source_rate / target_rate;
    (0..target_count)
        .map(|target| {
            let start = ((target as f64 * sources_per_target) as usize).min(columns.len() - 1);
            let end = (((target + 1) as f64 * sources_per_target) as usize)
                .clamp(start + 1, columns.len());
            columns[start..end]
                .iter()
                .fold(BandLevels::default(), |loudest, column| {
                    loudest.max(*column)
                })
        })
        .collect()
}

/// Where the zoomed waveform of one lane sits on screen for one frame: the playhead position
/// is drawn at the centre of the lane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LaneView {
    /// The lane's width in pixels.
    pub width: f32,
    /// The playhead position in seconds.
    pub playhead_seconds: f64,
    /// The horizontal scale.
    pub zoom: Zoom,
}

/// The columns a lane shows in one frame.
#[derive(Debug, Clone, PartialEq)]
pub struct VisibleColumns {
    /// The columns on screen.
    pub range: Range<usize>,
    /// The x position of the first column's left edge, relative to the lane.
    pub first_x: f32,
}

impl LaneView {
    fn centre(&self) -> f64 {
        f64::from(self.width) / 2.0
    }

    /// The x position, relative to the lane, of a moment in the track.
    pub fn x_of(&self, seconds: f64) -> f32 {
        (self.centre() + (seconds - self.playhead_seconds) * self.zoom.pixels_per_second()) as f32
    }

    /// The moments of the track at the left and right edges of the lane.
    pub fn seconds_on_screen(&self) -> Range<f64> {
        let half_span = self.centre() / self.zoom.pixels_per_second();
        self.playhead_seconds - half_span..self.playhead_seconds + half_span
    }

    /// The columns of a waveform with `column_count` one-pixel columns that fall on screen.
    pub fn visible_columns(&self, column_count: usize) -> VisibleColumns {
        let left_column = self.playhead_seconds * self.zoom.pixels_per_second() - self.centre();
        let first = left_column.floor().max(0.0);
        let last = (left_column + f64::from(self.width)).ceil().max(0.0);
        let start = (first as usize).min(column_count);
        let end = (last as usize).clamp(start, column_count);
        VisibleColumns {
            range: start..end,
            first_x: (first - left_column) as f32,
        }
    }
}

/// A beat of the grid, as the lanes draw it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BeatMark {
    /// When the beat plays, in seconds.
    pub seconds: f64,
    /// Whether it starts a bar.
    pub downbeat: bool,
}

/// A track's beat grid in seconds, for drawing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BeatMarks {
    marks: Vec<BeatMark>,
}

impl BeatMarks {
    /// The beats of rekordbox's grid.
    pub fn from_grid(grid: &BeatGrid) -> Self {
        Self {
            marks: grid
                .beats
                .iter()
                .map(|beat| BeatMark {
                    seconds: f64::from(beat.time_ms) / 1000.0,
                    downbeat: beat.is_downbeat(),
                })
                .collect(),
        }
    }

    /// The beats between `range.start` and `range.end` seconds.
    pub fn within(&self, range: Range<f64>) -> &[BeatMark] {
        let start = self
            .marks
            .partition_point(|mark| mark.seconds < range.start);
        let end = self.marks.partition_point(|mark| mark.seconds <= range.end);
        &self.marks[start..end.max(start)]
    }

    /// The beat nearest to `seconds`, or `None` when the grid is empty. Quantize snaps cues to it.
    pub fn nearest(&self, seconds: f64) -> Option<f64> {
        let after = self.marks.partition_point(|mark| mark.seconds < seconds);
        let candidates = [after.checked_sub(1), Some(after)];
        candidates
            .into_iter()
            .flatten()
            .filter_map(|index| self.marks.get(index))
            .map(|mark| mark.seconds)
            .min_by(|first, second| (first - seconds).abs().total_cmp(&(second - seconds).abs()))
    }
}

#[cfg(test)]
mod tests {
    use dubplate_rekordbox::analysis::{Beat, ColorPreviewColumn};

    use super::*;

    fn three_band(low: u8, mid: u8, high: u8) -> ThreeBandColumn {
        ThreeBandColumn { low, mid, high }
    }

    #[test]
    fn quantize_snaps_to_the_nearest_beat() {
        let marks = BeatMarks {
            marks: [0.5, 1.0, 1.5]
                .map(|seconds| BeatMark {
                    seconds,
                    downbeat: false,
                })
                .to_vec(),
        };
        assert_eq!(marks.nearest(0.0), Some(0.5));
        assert_eq!(marks.nearest(1.2), Some(1.0));
        assert_eq!(marks.nearest(1.3), Some(1.5));
        assert_eq!(marks.nearest(99.0), Some(1.5));
        assert_eq!(BeatMarks::default().nearest(1.0), None);
    }

    #[test]
    fn the_three_band_waveform_wins_and_is_scaled_to_its_peak() {
        let waveforms = Waveforms {
            three_band_detail: Some(vec![three_band(100, 50, 0), three_band(25, 0, 10)]),
            detail: Some(vec![MonochromeColumn(31), MonochromeColumn(31)]),
            ..Waveforms::default()
        };
        let zoomed = ZoomedWaveform::from_waveforms(&waveforms, Zoom::DEFAULT).unwrap();
        let levels: Vec<[f32; 3]> = zoomed
            .columns()
            .iter()
            .map(|column| [column.low, column.mid, column.high])
            .collect();
        let expected = [[1.0, 0.5, 0.0], [0.25, 0.0, 0.1]];
        for (column, wanted) in levels.iter().zip(expected) {
            for (level, wanted_level) in column.iter().zip(wanted) {
                assert!((level - wanted_level).abs() < 1e-6, "{levels:?}");
            }
        }
    }

    #[test]
    fn without_three_bands_the_colour_then_the_blue_waveform_is_used() {
        let colour = (7 << 13) | (7 << 7) | (31 << 2);
        let waveforms = Waveforms {
            color_detail: Some(vec![ColorDetailColumn(colour)]),
            detail: Some(vec![MonochromeColumn(31)]),
            ..Waveforms::default()
        };
        let zoomed = ZoomedWaveform::from_waveforms(&waveforms, Zoom::DEFAULT).unwrap();
        assert_eq!(
            zoomed.columns()[0],
            BandLevels {
                low: 1.0,
                mid: 0.0,
                high: 1.0
            }
        );

        let blue_only = Waveforms {
            detail: Some(vec![MonochromeColumn(31), MonochromeColumn(0xe0 | 15)]),
            ..Waveforms::default()
        };
        let zoomed = ZoomedWaveform::from_waveforms(&blue_only, Zoom::DEFAULT).unwrap();
        assert!((zoomed.columns()[1].low - 15.0 / 31.0).abs() < 1e-6);
        assert!(ZoomedWaveform::from_waveforms(&Waveforms::default(), Zoom::DEFAULT).is_none());
    }

    #[test]
    fn zooming_out_keeps_the_loudest_column_under_each_pixel() {
        let waveforms = Waveforms {
            three_band_detail: Some(vec![
                three_band(10, 0, 0),
                three_band(40, 0, 0),
                three_band(20, 0, 0),
                three_band(0, 40, 0),
            ]),
            ..Waveforms::default()
        };
        let half = Zoom::new(f64::from(DETAIL_COLUMNS_PER_SECOND) / 2.0);
        let zoomed = ZoomedWaveform::from_waveforms(&waveforms, half).unwrap();
        let lows: Vec<f32> = zoomed.columns().iter().map(|column| column.low).collect();
        let mids: Vec<f32> = zoomed.columns().iter().map(|column| column.mid).collect();
        assert_eq!(lows, [1.0, 0.5]);
        assert_eq!(mids, [0.0, 1.0]);
    }

    #[test]
    fn zooming_in_repeats_columns() {
        let waveforms = Waveforms {
            three_band_detail: Some(vec![three_band(10, 0, 0), three_band(20, 0, 0)]),
            ..Waveforms::default()
        };
        let double = Zoom::new(f64::from(DETAIL_COLUMNS_PER_SECOND) * 2.0);
        let zoomed = ZoomedWaveform::from_waveforms(&waveforms, double).unwrap();
        let lows: Vec<f32> = zoomed.columns().iter().map(|column| column.low).collect();
        assert_eq!(lows, [0.5, 0.5, 1.0, 1.0]);
    }

    #[test]
    fn the_playhead_column_sits_at_the_centre_of_the_lane() {
        let view = LaneView {
            width: 100.0,
            playhead_seconds: 1.0,
            zoom: Zoom::new(100.0),
        };
        let visible = view.visible_columns(1000);
        assert_eq!(visible.range, 50..150);
        assert_eq!(visible.first_x, 0.0);
        assert_eq!(view.x_of(1.0), 50.0);
        assert_eq!(view.x_of(1.5), 100.0);
        assert_eq!(view.seconds_on_screen(), 0.5..1.5);
    }

    #[test]
    fn columns_scroll_by_fractions_of_a_pixel() {
        let view = LaneView {
            width: 100.0,
            playhead_seconds: 1.0025,
            zoom: Zoom::new(100.0),
        };
        let visible = view.visible_columns(1000);
        assert_eq!(visible.range, 50..151);
        assert!((visible.first_x + 0.25).abs() < 1e-4);
    }

    #[test]
    fn near_the_start_and_end_only_real_columns_are_shown() {
        let start = LaneView {
            width: 100.0,
            playhead_seconds: 0.0,
            zoom: Zoom::new(100.0),
        };
        let visible = start.visible_columns(1000);
        assert_eq!(visible.range, 0..50);
        assert_eq!(visible.first_x, 50.0);

        let end = LaneView {
            playhead_seconds: 10.0,
            ..start
        };
        assert_eq!(end.visible_columns(1000).range, 950..1000);
        let past_the_end = LaneView {
            playhead_seconds: 20.0,
            ..start
        };
        assert!(past_the_end.visible_columns(1000).range.is_empty());
    }

    #[test]
    fn only_beats_on_screen_are_drawn_with_their_downbeats() {
        let grid = BeatGrid {
            beats: (0..16)
                .map(|index| Beat {
                    beat_in_bar: (index % 4 + 1) as u16,
                    tempo_hundredths: 12000,
                    time_ms: index * 500,
                })
                .collect(),
        };
        let marks = BeatMarks::from_grid(&grid);
        let view = LaneView {
            width: 300.0,
            playhead_seconds: 2.0,
            zoom: Zoom::new(100.0),
        };
        let shown = marks.within(view.seconds_on_screen());
        let seconds: Vec<f64> = shown.iter().map(|mark| mark.seconds).collect();
        assert_eq!(seconds, [0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5]);
        let downbeats: Vec<f64> = shown
            .iter()
            .filter(|mark| mark.downbeat)
            .map(|mark| mark.seconds)
            .collect();
        assert_eq!(downbeats, [2.0]);
        assert_eq!(view.x_of(2.0), 150.0);
    }

    #[test]
    fn the_overview_squeezes_the_whole_track_into_bars() {
        let waveforms = Waveforms {
            color_preview: Some(vec![
                ColorPreviewColumn([0, 0, 0, 10, 0, 0]),
                ColorPreviewColumn([0, 0, 0, 0, 40, 0]),
                ColorPreviewColumn([0, 0, 0, 0, 0, 20]),
                ColorPreviewColumn([0, 0, 0, 0, 0, 0]),
            ]),
            ..Waveforms::default()
        };
        let overview = OverviewWaveform::from_waveforms(&waveforms).unwrap();
        assert_eq!(overview.bars(2), [1.0, 0.5]);
        assert_eq!(overview.bars(4), [0.25, 1.0, 0.5, 0.0]);
        assert_eq!(overview.bars(8).len(), 8);
    }
}
