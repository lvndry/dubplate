//! A track's 3-band waveform worked out from its decoded audio, for the tracks rekordbox has no
//! waveform for. It has the shape of rekordbox's own 3-band waveform: 150 columns a second of
//! the lows, the mids and the highs, split where the mixer's EQ splits them, so the decks draw
//! it exactly as they draw rekordbox's.
//!
//! The band filters are recursive, so one run through the track is a chain where every sample
//! waits for the one before. To use the vector units, the track is cut into [`SEGMENTS`]
//! stretches of whole columns that are filtered side by side, one stretch per vector lane, in a
//! single pass. Each stretch starts [`SETTLE_FRAMES`] early so its filters have forgotten their
//! silent start by the time its first column is measured.

use std::f32::consts::{FRAC_1_SQRT_2, TAU};

use dubplate_engine::{HIGH_CROSSOVER_HZ, LOW_CROSSOVER_HZ, Track};
use dubplate_rekordbox::analysis::{DETAIL_COLUMNS_PER_SECOND, ThreeBandColumn, Waveforms};

/// Stretches of the track filtered side by side: two 128-bit vectors of `f32`.
const SEGMENTS: usize = 8;
/// Frames each stretch moves on per block.
const BLOCK_FRAMES: usize = 1_024;
/// Frames run through the filters before a stretch's first column, about 40 ms: their response
/// to the silence before the stretch has died away well before then.
const SETTLE_FRAMES: usize = 2_048;
const BUTTERWORTH_Q: f32 = FRAC_1_SQRT_2;
const LEVEL_STEPS: f32 = u8::MAX as f32;
/// How loud each band is drawn against its level. rekordbox draws the mids and the highs
/// larger next to the lows than their levels alone would: these weights, fitted against
/// rekordbox's own 3-band waveforms of the same tracks, give the same balance.
const BAND_WEIGHTS: [f32; 3] = [1.0, 1.6, 1.2];

/// The waveforms of `track`'s decoded audio: the scrolling 3-band waveform, from which the
/// overview is drawn too. Reads the audio once; call it once the track is decoded, off the
/// interface and audio threads.
pub fn three_band_waveforms(track: &Track) -> Waveforms {
    Waveforms {
        three_band_detail: Some(quantized(&band_levels(track))),
        ..Waveforms::default()
    }
}

/// The root-mean-square level of the lows, the mids and the highs in each column.
fn band_levels(track: &Track) -> Vec<[f32; 3]> {
    let columns = ColumnClock::new(track.sample_rate(), track.decoded_frames());
    let mut segments: [Segment; SEGMENTS] =
        std::array::from_fn(|index| Segment::new(&columns, index));
    let mut filters = BandFilters::new(track.sample_rate() as f32);
    let mut frames = vec![[0.0_f32; 2]; BLOCK_FRAMES];
    let mut mono = vec![[0.0_f32; SEGMENTS]; BLOCK_FRAMES];
    let mut energies = RunningEnergies::new();
    while segments.iter().any(Segment::is_running) {
        for (lane, segment) in segments.iter().enumerate() {
            segment.read_mono(track, &mut frames, &mut mono, lane);
        }
        filters.process(&mono, &mut energies);
        for (lane, segment) in segments.iter_mut().enumerate() {
            segment.measure(&columns, &energies, lane);
        }
    }
    segments
        .into_iter()
        .flat_map(|segment| segment.levels)
        .collect()
}

/// Weighs the bands as rekordbox does and scales the levels so the loudest band of the
/// loudest column is the highest step.
fn quantized(levels: &[[f32; 3]]) -> Vec<ThreeBandColumn> {
    let weighed = |column: &[f32; 3]| {
        std::array::from_fn::<f32, 3, _>(|band| column[band] * BAND_WEIGHTS[band])
    };
    let peak = levels.iter().flat_map(weighed).fold(0.0_f32, f32::max);
    let scale = if peak > 0.0 { LEVEL_STEPS / peak } else { 0.0 };
    let step = |level: f32| (level * scale).round().clamp(0.0, LEVEL_STEPS) as u8;
    levels
        .iter()
        .map(|column| {
            let [low, mid, high] = weighed(column);
            ThreeBandColumn {
                low: step(low),
                mid: step(mid),
                high: step(high),
            }
        })
        .collect()
}

/// Where the columns of a track start, on the track's own clock, so they stay exact at any
/// sample rate.
struct ColumnClock {
    sample_rate: u64,
    frame_count: usize,
    column_count: usize,
}

impl ColumnClock {
    fn new(sample_rate: u32, frame_count: usize) -> Self {
        let sample_rate = u64::from(sample_rate.max(1));
        let column_count =
            (frame_count as u64 * u64::from(DETAIL_COLUMNS_PER_SECOND)).div_ceil(sample_rate);
        Self {
            sample_rate,
            frame_count,
            column_count: usize::try_from(column_count).unwrap_or(usize::MAX),
        }
    }

    /// The first frame of `column`; the end of the track for the column after the last.
    fn start(&self, column: usize) -> usize {
        let frame = column as u64 * self.sample_rate / u64::from(DETAIL_COLUMNS_PER_SECOND);
        usize::try_from(frame).map_or(self.frame_count, |frame| frame.min(self.frame_count))
    }
}

/// One stretch of whole columns, filtered in its own vector lane.
struct Segment {
    /// The track frame the next block starts at.
    position: usize,
    /// Frames before this one only settle the filters.
    first_measured: usize,
    end: usize,
    /// The column being measured.
    column: usize,
    energy: [f32; 3],
    levels: Vec<[f32; 3]>,
}

impl Segment {
    fn new(columns: &ColumnClock, index: usize) -> Self {
        let first_column = index * columns.column_count / SEGMENTS;
        let end_column = (index + 1) * columns.column_count / SEGMENTS;
        let first_measured = columns.start(first_column);
        let end = columns.start(end_column);
        let position = if first_column == end_column {
            end
        } else {
            first_measured.saturating_sub(SETTLE_FRAMES)
        };
        Self {
            position,
            first_measured,
            end,
            column: first_column,
            energy: [0.0; 3],
            levels: Vec::with_capacity(end_column - first_column),
        }
    }

    fn is_running(&self) -> bool {
        self.position < self.end
    }

    /// Puts the next block of the stretch, as mono, in lane `lane` of `mono`, with silence
    /// past its end.
    fn read_mono(
        &self,
        track: &Track,
        frames: &mut [[f32; 2]],
        mono: &mut [[f32; SEGMENTS]],
        lane: usize,
    ) {
        let wanted = BLOCK_FRAMES.min(self.end.saturating_sub(self.position));
        let read = track.read_frames(self.position, &mut frames[..wanted]);
        let (heard, silent) = mono.split_at_mut(read);
        for (frame, [left, right]) in heard.iter_mut().zip(&frames[..read]) {
            frame[lane] = (left + right) * 0.5;
        }
        for frame in silent {
            frame[lane] = 0.0;
        }
    }

    /// Adds the block's energies in lane `lane` to the stretch's columns and moves on a block.
    fn measure(&mut self, columns: &ColumnClock, energies: &RunningEnergies, lane: usize) {
        let block_start = self.position;
        let block_end = (block_start + BLOCK_FRAMES).min(self.end);
        let mut frame = self.first_measured.max(block_start);
        while frame < block_end {
            let column_end = columns.start(self.column + 1);
            let run_end = column_end.min(block_end);
            let before = &energies.totals[frame - block_start];
            let after = &energies.totals[run_end - block_start];
            for ((energy, before), after) in self.energy.iter_mut().zip(before).zip(after) {
                *energy += after[lane] - before[lane];
            }
            frame = run_end;
            if frame == column_end {
                let frame_count = (column_end - columns.start(self.column)).max(1) as f32;
                self.levels
                    .push(self.energy.map(|energy| (energy / frame_count).sqrt()));
                self.energy = [0.0; 3];
                self.column += 1;
            }
        }
        self.position = block_start + BLOCK_FRAMES;
    }
}

/// The energy of each band in each lane from the start of the block to each frame of it, so a
/// column's energy is the difference of two entries, whatever lane and length it has.
struct RunningEnergies {
    totals: Vec<[[f32; SEGMENTS]; 3]>,
}

impl RunningEnergies {
    fn new() -> Self {
        Self {
            totals: vec![[[0.0; SEGMENTS]; 3]; BLOCK_FRAMES + 1],
        }
    }
}

/// The filters of every lane: a low-pass for the lows, a band-pass for the mids and a
/// high-pass for the highs.
struct BandFilters {
    low: Biquad,
    mid_above_lows: Biquad,
    mid_below_highs: Biquad,
    high: Biquad,
}

impl BandFilters {
    fn new(sample_rate: f32) -> Self {
        Self {
            low: Biquad::new(Coefficients::low_pass(LOW_CROSSOVER_HZ, sample_rate)),
            mid_above_lows: Biquad::new(Coefficients::high_pass(LOW_CROSSOVER_HZ, sample_rate)),
            mid_below_highs: Biquad::new(Coefficients::low_pass(HIGH_CROSSOVER_HZ, sample_rate)),
            high: Biquad::new(Coefficients::high_pass(HIGH_CROSSOVER_HZ, sample_rate)),
        }
    }

    fn process(&mut self, mono: &[[f32; SEGMENTS]], energies: &mut RunningEnergies) {
        let (start, totals) = energies.totals.split_at_mut(1);
        let mut running = [[0.0; SEGMENTS]; 3];
        start[0] = running;
        for (input, total) in mono.iter().zip(totals) {
            let bands = [
                self.low.process(*input),
                self.mid_below_highs
                    .process(self.mid_above_lows.process(*input)),
                self.high.process(*input),
            ];
            for (sums, band) in running.iter_mut().zip(bands) {
                for (sum, level) in sums.iter_mut().zip(band) {
                    *sum = level.mul_add(level, *sum);
                }
            }
            *total = running;
        }
    }
}

/// A second-order filter in transposed direct form II, run in every lane at once.
struct Biquad {
    coefficients: Coefficients,
    first: [f32; SEGMENTS],
    second: [f32; SEGMENTS],
}

impl Biquad {
    fn new(coefficients: Coefficients) -> Self {
        Self {
            coefficients,
            first: [0.0; SEGMENTS],
            second: [0.0; SEGMENTS],
        }
    }

    #[inline]
    fn process(&mut self, input: [f32; SEGMENTS]) -> [f32; SEGMENTS] {
        let Coefficients { b0, b1, b2, a1, a2 } = self.coefficients;
        let mut output = [0.0; SEGMENTS];
        for lane in 0..SEGMENTS {
            let sample = input[lane];
            let filtered = b0.mul_add(sample, self.first[lane]);
            self.first[lane] = b1.mul_add(sample, (-a1).mul_add(filtered, self.second[lane]));
            self.second[lane] = b2.mul_add(sample, -a2 * filtered);
            output[lane] = filtered;
        }
        output
    }
}

/// A Butterworth second-order section, normalised so the output's own coefficient is one.
#[derive(Debug, Clone, Copy)]
struct Coefficients {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

impl Coefficients {
    fn low_pass(cutoff_hz: f32, sample_rate: f32) -> Self {
        let (cosine, alpha) = angle(cutoff_hz, sample_rate);
        let side = (1.0 - cosine) / 2.0;
        Self::normalized([side, 1.0 - cosine, side], cosine, alpha)
    }

    fn high_pass(cutoff_hz: f32, sample_rate: f32) -> Self {
        let (cosine, alpha) = angle(cutoff_hz, sample_rate);
        let side = (1.0 + cosine) / 2.0;
        Self::normalized([side, -(1.0 + cosine), side], cosine, alpha)
    }

    fn normalized([b0, b1, b2]: [f32; 3], cosine: f32, alpha: f32) -> Self {
        let scale = 1.0 / (1.0 + alpha);
        Self {
            b0: b0 * scale,
            b1: b1 * scale,
            b2: b2 * scale,
            a1: -2.0 * cosine * scale,
            a2: (1.0 - alpha) * scale,
        }
    }
}

/// The cosine of the cutoff's angle and the bandwidth term, with the cutoff kept below the
/// Nyquist frequency for low sample rates.
fn angle(cutoff_hz: f32, sample_rate: f32) -> (f32, f32) {
    let angle = TAU * cutoff_hz.min(sample_rate * 0.45) / sample_rate;
    (angle.cos(), angle.sin() / (2.0 * BUTTERWORTH_Q))
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use super::*;
    use crate::waveform::{OverviewWaveform, Zoom, ZoomedWaveform};

    const SAMPLE_RATE: u32 = 44_100;

    fn track(seconds: f32, sample: impl Fn(usize) -> f32) -> Track {
        let frame_count = (seconds * SAMPLE_RATE as f32) as usize;
        let samples: Vec<f32> = (0..frame_count)
            .flat_map(|frame| {
                let value = sample(frame);
                [value, value]
            })
            .collect();
        Track::from_interleaved_stereo(&samples, SAMPLE_RATE).unwrap()
    }

    fn sine(frequency: f32, frame: usize) -> f32 {
        0.5 * (TAU * frequency * frame as f32 / SAMPLE_RATE as f32).sin()
    }

    fn columns(track: &Track) -> Vec<ThreeBandColumn> {
        three_band_waveforms(track).three_band_detail.unwrap()
    }

    #[test]
    fn there_are_150_columns_a_second() {
        let tone = |frame| sine(1_000.0, frame);
        assert_eq!(columns(&track(2.0, tone)).len(), 300);
        assert_eq!(columns(&track(0.5, tone)).len(), 75);
        assert_eq!(columns(&track(0.01, tone)).len(), 2);
        assert!(columns(&track(0.0, tone)).is_empty());
    }

    #[test]
    fn a_44_1_khz_and_a_48_khz_file_line_up_column_for_column() {
        let burst_seconds = 2.0;
        let length_and_onset = |sample_rate: u32| {
            let frame_count = (4.0 * sample_rate as f32) as usize;
            let burst = (burst_seconds * sample_rate as f32) as usize..;
            let samples: Vec<f32> = (0..frame_count)
                .flat_map(|frame| {
                    let tone = 0.5 * (TAU * 900.0 * frame as f32 / sample_rate as f32).sin();
                    let value = if burst.contains(&frame) { tone } else { 0.0 };
                    [value, value]
                })
                .collect();
            let track = Track::from_interleaved_stereo(&samples, sample_rate).unwrap();
            let columns = columns(&track);
            let first_loud = columns.iter().position(|column| column.mid > 0);
            (columns.len(), first_loud)
        };
        let (cd_columns, cd_onset) = length_and_onset(44_100);
        let (studio_columns, studio_onset) = length_and_onset(48_000);
        assert_eq!(cd_columns, 600);
        assert_eq!(studio_columns, 600);
        let onset = |column: Option<usize>| column.unwrap() as i64;
        assert!(
            (onset(cd_onset) - onset(studio_onset)).abs() <= 1,
            "{cd_onset:?} {studio_onset:?}"
        );
        assert!((onset(cd_onset) - 300).abs() <= 1, "{cd_onset:?}");
    }

    #[test]
    fn a_track_rekordbox_never_analysed_gets_both_waveforms_from_its_audio() {
        let waveforms = three_band_waveforms(&track(2.0, |frame| sine(120.0, frame)));
        let zoomed = ZoomedWaveform::from_waveforms(&waveforms, Zoom::DEFAULT).unwrap();
        assert_eq!(zoomed.columns().len(), 300);
        let overview = OverviewWaveform::from_waveforms(&waveforms).unwrap();
        assert!(overview.bars(100).iter().all(|level| level.loudest() > 0.9));
    }

    #[test]
    fn each_tone_lands_in_its_band_all_through_the_track() {
        for (frequency, band) in [(60.0, 0), (900.0, 1), (9_000.0, 2)] {
            for column in &columns(&track(3.0, |frame| sine(frequency, frame)))[2..448] {
                let levels = [column.low, column.mid, column.high];
                assert!(levels[band] > 200, "{frequency} Hz: {column:?}");
                for (other, level) in levels.into_iter().enumerate() {
                    if other != band {
                        assert!(level < 90, "{frequency} Hz: {column:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_stretches_join_without_a_seam() {
        let one_period_a_column = DETAIL_COLUMNS_PER_SECOND as f32;
        let columns = columns(&track(4.0, |frame| sine(one_period_a_column, frame)));
        let lows: Vec<u8> = columns[2..columns.len() - 1]
            .iter()
            .map(|column| column.low)
            .collect();
        let quietest = lows.iter().min().unwrap();
        let loudest = lows.iter().max().unwrap();
        assert!(loudest - quietest <= 2, "{quietest} to {loudest}");
    }

    #[test]
    fn silence_draws_nothing_and_the_music_after_it_draws_full_height() {
        let half = SAMPLE_RATE as usize / 2;
        let columns = columns(&track(1.0, |frame| {
            if frame < half {
                0.0
            } else {
                sine(100.0, frame)
            }
        }));
        assert_eq!(
            columns[10],
            ThreeBandColumn {
                low: 0,
                mid: 0,
                high: 0
            }
        );
        assert!(columns[120].low > 200, "{:?}", columns[120]);
    }
}
