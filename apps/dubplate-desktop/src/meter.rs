//! Peak meters as segments: a peak lights its segment at once and falls back slowly, as on a
//! club mixer.

use dubplate_engine::gain_to_decibels;

/// Segments in each meter.
pub const SEGMENTS: usize = 12;
/// Segments from this one up are drawn in the warning colour.
pub const HOT_FROM: usize = 10;
/// The level, in dBFS, at which each segment lights.
const SEGMENT_THRESHOLDS_DB: [f32; SEGMENTS] = [
    -48.0, -40.0, -33.0, -27.0, -22.0, -18.0, -14.0, -10.0, -6.0, -3.0, -1.0, -0.1,
];
/// How fast the meter falls after a peak, in decibels a second.
const FALL_DB_PER_SECOND: f32 = 24.0;
const SILENCE_DB: f32 = -120.0;

/// One meter's shown level.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PeakMeter {
    shown_db: f32,
}

impl Default for PeakMeter {
    fn default() -> Self {
        Self {
            shown_db: SILENCE_DB,
        }
    }
}

impl PeakMeter {
    /// Takes the linear peak measured since the last frame, `elapsed_seconds` ago.
    pub fn update(&mut self, peak: f32, elapsed_seconds: f32) {
        let peak_db = if peak > 0.0 {
            gain_to_decibels(peak)
        } else {
            SILENCE_DB
        };
        let fallen = self.shown_db - FALL_DB_PER_SECOND * elapsed_seconds.max(0.0);
        self.shown_db = peak_db.max(fallen).max(SILENCE_DB);
    }

    /// How many segments are lit, from the bottom.
    pub fn lit_segments(&self) -> usize {
        SEGMENT_THRESHOLDS_DB
            .iter()
            .take_while(|threshold| self.shown_db >= **threshold)
            .count()
    }

    /// The level shown, in dBFS.
    pub fn decibels(&self) -> f32 {
        self.shown_db
    }

    /// Whether the meter shows anything, so the window keeps redrawing while it falls.
    pub fn is_moving(&self) -> bool {
        self.shown_db > SEGMENT_THRESHOLDS_DB[0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_lights_nothing() {
        let mut meter = PeakMeter::default();
        meter.update(0.0, 0.016);
        assert_eq!(meter.lit_segments(), 0);
        assert!(!meter.is_moving());
    }

    #[test]
    fn a_peak_lights_its_segment_at_once() {
        let mut meter = PeakMeter::default();
        meter.update(0.5, 0.016);
        assert_eq!(meter.lit_segments(), 8);
        meter.update(1.0, 0.016);
        assert_eq!(meter.lit_segments(), SEGMENTS);
    }

    #[test]
    fn the_meter_falls_back_slowly_after_a_peak() {
        let mut meter = PeakMeter::default();
        meter.update(1.0, 0.016);
        meter.update(0.0, 0.1);
        assert_eq!(meter.lit_segments(), 10);
        meter.update(0.0, 0.5);
        assert_eq!(meter.lit_segments(), 6);
        meter.update(0.0, 5.0);
        assert_eq!(meter.lit_segments(), 0);
    }
}
