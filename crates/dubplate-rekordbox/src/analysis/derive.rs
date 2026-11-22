//! Every waveform rekordbox writes, worked out from a 3-band detail waveform alone, for tracks
//! rekordbox never analysed.
//!
//! rekordbox draws its waveforms from its own spectral analysis, which is not documented. Each
//! kind here is a linear fit of rekordbox's own columns against the 3-band detail of the same
//! track, measured over 60 tracks rekordbox 7.2 analysed; a silent stretch stays silent. The fits
//! explain most of what rekordbox draws (the heights best), so a CDJ shows the track's shape,
//! loudness and colour balance as rekordbox would, without matching it pixel for pixel.

use super::waveform::{
    ColorDetailColumn, ColorPreviewColumn, MonochromeColumn, ThreeBandColumn, TinyColumn, Waveforms,
};

/// The loudest a 3-band detail column gets in rekordbox's analysis.
const DETAIL_PEAK: f32 = 127.0;
const PREVIEW_COLUMNS: usize = 1200;
const BLUE_PREVIEW_COLUMNS: usize = 400;
const TINY_PREVIEW_COLUMNS: usize = 100;
const FIVE_BITS: f32 = 31.0;
const FOUR_BITS: f32 = 15.0;
const THREE_BITS: f32 = 7.0;
const SEVEN_BITS: f32 = 127.0;
const EIGHT_BITS: f32 = 255.0;

/// A linear fit of one value from the low, mid and high levels.
#[derive(Clone, Copy)]
struct Fit {
    low: f32,
    mid: f32,
    high: f32,
    constant: f32,
    maximum: f32,
}

impl Fit {
    const fn new(low: f32, mid: f32, high: f32, constant: f32, maximum: f32) -> Self {
        Self {
            low,
            mid,
            high,
            constant,
            maximum,
        }
    }

    fn apply(self, [low, mid, high]: [f32; 3]) -> u8 {
        if low == 0.0 && mid == 0.0 && high == 0.0 {
            return 0;
        }
        let value = self.low * low + self.mid * mid + self.high * high + self.constant;
        value.round().clamp(0.0, self.maximum) as u8
    }
}

const PREVIEW_LOW: Fit = Fit::new(0.475, 0.0, 0.0, 2.34, SEVEN_BITS);
const PREVIEW_MID: Fit = Fit::new(0.0, 0.623, 0.0, 1.17, SEVEN_BITS);
const PREVIEW_HIGH: Fit = Fit::new(0.0, 0.0, 1.328, 9.56, SEVEN_BITS);
const DETAIL_HEIGHT: Fit = Fit::new(0.178, 0.107, 0.178, -4.73, FIVE_BITS);
const DETAIL_WHITENESS: Fit = Fit::new(-0.068, 0.037, 0.019, 5.10, THREE_BITS);
const COLOR_RED: Fit = Fit::new(0.074, -0.041, -0.013, 3.08, THREE_BITS);
const COLOR_GREEN: Fit = Fit::new(-0.058, 0.060, -0.011, 4.10, THREE_BITS);
const COLOR_BLUE: Fit = Fit::new(-0.055, 0.011, 0.078, 5.38, THREE_BITS);
const COLOR_HEIGHT: Fit = Fit::new(0.180, 0.109, 0.177, -4.90, FIVE_BITS);
/// The six bytes of a colour preview column, from the 3-band preview of the same column.
const COLOR_PREVIEW: [Fit; 6] = [
    Fit::new(2.358, 0.204, 0.767, 18.59, EIGHT_BITS),
    Fit::new(-0.814, 1.194, -0.244, 149.43, EIGHT_BITS),
    Fit::new(2.865, -0.146, 0.214, 10.93, SEVEN_BITS),
    Fit::new(3.170, -0.800, 0.186, 6.27, SEVEN_BITS),
    Fit::new(0.739, 0.612, 0.178, 6.24, SEVEN_BITS),
    Fit::new(0.456, -0.150, 0.622, 2.01, SEVEN_BITS),
];
const BLUE_PREVIEW_HEIGHT: Fit = Fit::new(0.342, -0.028, 0.099, 1.12, FIVE_BITS);
const BLUE_PREVIEW_WHITENESS: Fit = Fit::new(-0.068, 0.001, 0.052, 4.85, THREE_BITS);
const TINY_PREVIEW_HEIGHT: Fit = Fit::new(0.196, 0.003, 0.001, 3.00, FOUR_BITS);

/// Every waveform of a track from its 3-band detail waveform, 150 columns a second at any
/// scale: it is first brought to rekordbox's scale, where the loudest band of the loudest
/// column is 127.
pub fn derive_waveforms(three_band_detail: &[ThreeBandColumn]) -> Waveforms {
    let peak = three_band_detail
        .iter()
        .flat_map(|column| [column.low, column.mid, column.high])
        .max()
        .unwrap_or(0);
    let scale = if peak == 0 {
        0.0
    } else {
        DETAIL_PEAK / f32::from(peak)
    };
    let detail: Vec<[f32; 3]> = three_band_detail
        .iter()
        .map(|column| {
            [column.low, column.mid, column.high].map(|level| (f32::from(level) * scale).round())
        })
        .collect();
    let preview = column_means(&detail, PREVIEW_COLUMNS);
    let three_band_preview: Vec<[f32; 3]> = preview
        .iter()
        .map(|levels| {
            [
                f32::from(PREVIEW_LOW.apply(*levels)),
                f32::from(PREVIEW_MID.apply(*levels)),
                f32::from(PREVIEW_HIGH.apply(*levels)),
            ]
        })
        .collect();

    Waveforms {
        three_band_detail: Some(detail.iter().map(|levels| band_column(*levels)).collect()),
        three_band_preview: Some(
            three_band_preview
                .iter()
                .map(|levels| band_column(*levels))
                .collect(),
        ),
        detail: Some(
            detail
                .iter()
                .map(|levels| monochrome(DETAIL_HEIGHT, DETAIL_WHITENESS, *levels))
                .collect(),
        ),
        color_detail: Some(detail.iter().map(|levels| color_detail(*levels)).collect()),
        color_preview: Some(
            three_band_preview
                .iter()
                .map(|levels| ColorPreviewColumn(COLOR_PREVIEW.map(|fit| fit.apply(*levels))))
                .collect(),
        ),
        preview: Some(
            column_means(&detail, BLUE_PREVIEW_COLUMNS)
                .iter()
                .map(|levels| monochrome(BLUE_PREVIEW_HEIGHT, BLUE_PREVIEW_WHITENESS, *levels))
                .collect(),
        ),
        tiny_preview: Some(
            column_means(&detail, TINY_PREVIEW_COLUMNS)
                .iter()
                .map(|levels| TinyColumn(TINY_PREVIEW_HEIGHT.apply(*levels)))
                .collect(),
        ),
    }
}

fn band_column([low, mid, high]: [f32; 3]) -> ThreeBandColumn {
    ThreeBandColumn {
        low: low as u8,
        mid: mid as u8,
        high: high as u8,
    }
}

fn monochrome(height: Fit, whiteness: Fit, levels: [f32; 3]) -> MonochromeColumn {
    MonochromeColumn(whiteness.apply(levels) << 5 | height.apply(levels))
}

fn color_detail(levels: [f32; 3]) -> ColorDetailColumn {
    let red = u16::from(COLOR_RED.apply(levels));
    let green = u16::from(COLOR_GREEN.apply(levels));
    let blue = u16::from(COLOR_BLUE.apply(levels));
    let height = u16::from(COLOR_HEIGHT.apply(levels));
    ColorDetailColumn(red << 13 | green << 10 | blue << 7 | height << 2)
}

/// The mean levels of `count` equal stretches of the track, as a fixed-width overview draws
/// them. A track shorter than `count` columns repeats its columns.
fn column_means(detail: &[[f32; 3]], count: usize) -> Vec<[f32; 3]> {
    if detail.is_empty() {
        return vec![[0.0; 3]; count];
    }
    (0..count)
        .map(|column| {
            let start = column * detail.len() / count;
            let end = ((column + 1) * detail.len() / count).max(start + 1);
            let stretch = &detail[start..end.min(detail.len())];
            let mut sums = [0.0_f32; 3];
            for levels in stretch {
                for band in 0..3 {
                    sums[band] += levels[band];
                }
            }
            sums.map(|sum| sum / stretch.len() as f32)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(low: u8, mid: u8, high: u8) -> ThreeBandColumn {
        ThreeBandColumn { low, mid, high }
    }

    #[test]
    fn every_kind_has_rekordbox_s_size_and_range() {
        let detail: Vec<ThreeBandColumn> = (0..45_000_u32)
            .map(|index| {
                let kick = if index % 75 < 8 { 250 } else { 40 };
                column(kick, (index % 200) as u8, 90)
            })
            .collect();
        let waveforms = derive_waveforms(&detail);
        assert_eq!(waveforms.three_band_detail.as_ref().unwrap().len(), 45_000);
        assert_eq!(waveforms.detail.as_ref().unwrap().len(), 45_000);
        assert_eq!(waveforms.color_detail.as_ref().unwrap().len(), 45_000);
        assert_eq!(waveforms.three_band_preview.as_ref().unwrap().len(), 1200);
        assert_eq!(waveforms.color_preview.as_ref().unwrap().len(), 1200);
        assert_eq!(waveforms.preview.as_ref().unwrap().len(), 400);
        assert_eq!(waveforms.tiny_preview.as_ref().unwrap().len(), 100);
        let peak = waveforms
            .three_band_detail
            .as_ref()
            .unwrap()
            .iter()
            .map(|column| column.low.max(column.mid).max(column.high))
            .max();
        assert_eq!(peak, Some(127));
        assert!(
            waveforms
                .detail
                .unwrap()
                .iter()
                .all(|column| column.height() <= 31)
        );
        assert!(
            waveforms
                .tiny_preview
                .unwrap()
                .iter()
                .all(|column| column.0 <= 15)
        );
    }

    #[test]
    fn kicks_draw_redder_and_taller_than_hats() {
        let kick = color_detail([127.0, 20.0, 10.0]);
        let hat = color_detail([5.0, 20.0, 110.0]);
        assert!(kick.red() > hat.red());
        assert!(hat.blue() > kick.blue());
        assert!(kick.height() > 0);
    }

    #[test]
    fn silence_stays_silent() {
        let waveforms = derive_waveforms(&vec![column(0, 0, 0); 300]);
        assert!(
            waveforms
                .preview
                .unwrap()
                .iter()
                .all(|column| column.0 == 0)
        );
        assert!(
            waveforms
                .color_preview
                .unwrap()
                .iter()
                .all(|column| column.0 == [0; 6])
        );
    }
}
