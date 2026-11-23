//! Peak meters as segments: a peak lights its segment at once and falls back slowly, as on a
//! club mixer. The highest peak stays lit a moment above the falling level, and a peak at full
//! scale lights the top segment as a clip light that holds long enough to be seen.

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
/// How long the highest peak stays lit before it falls, in seconds.
const PEAK_HOLD_SECONDS: f32 = 1.5;
/// A peak this loud, in dBFS, is at full scale: it lights the clip light.
const CLIP_DB: f32 = -0.1;
/// How long the clip light stays on after the last peak at full scale, in seconds.
const CLIP_HOLD_SECONDS: f32 = 2.0;

/// One meter's shown level.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PeakMeter {
    shown_db: f32,
    held_db: f32,
    hold_left_seconds: f32,
    clip_left_seconds: f32,
}

impl Default for PeakMeter {
    fn default() -> Self {
        Self {
            shown_db: SILENCE_DB,
            held_db: SILENCE_DB,
            hold_left_seconds: 0.0,
            clip_left_seconds: 0.0,
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
        let elapsed_seconds = elapsed_seconds.max(0.0);
        let fall = FALL_DB_PER_SECOND * elapsed_seconds;
        self.shown_db = peak_db.max(self.shown_db - fall).max(SILENCE_DB);
        self.hold_left_seconds -= elapsed_seconds;
        if peak_db >= self.held_db {
            self.held_db = peak_db;
            self.hold_left_seconds = PEAK_HOLD_SECONDS;
        } else if self.hold_left_seconds <= 0.0 {
            self.held_db = (self.held_db - fall).max(self.shown_db);
        }
        self.clip_left_seconds = if peak_db >= CLIP_DB {
            CLIP_HOLD_SECONDS
        } else {
            (self.clip_left_seconds - elapsed_seconds).max(0.0)
        };
    }

    /// How many segments are lit, from the bottom.
    pub fn lit_segments(&self) -> usize {
        segments_at(self.shown_db)
    }

    /// The segment the highest recent peak holds lit above the falling level, if any.
    pub fn held_segment(&self) -> Option<usize> {
        let held = segments_at(self.held_db);
        (held > self.lit_segments()).then(|| held - 1)
    }

    /// Whether a peak at full scale came in the last two seconds: the top segment is the clip
    /// light.
    pub fn clipping(&self) -> bool {
        self.clip_left_seconds > 0.0
    }

    /// The level shown, in dBFS.
    pub fn decibels(&self) -> f32 {
        self.shown_db
    }

    /// Whether the meter shows anything, so the window keeps redrawing while it falls.
    pub fn is_moving(&self) -> bool {
        self.held_db > SEGMENT_THRESHOLDS_DB[0] || self.clipping()
    }
}

/// How many segments a level lights, from the bottom.
fn segments_at(decibels: f32) -> usize {
    SEGMENT_THRESHOLDS_DB
        .iter()
        .take_while(|threshold| decibels >= **threshold)
        .count()
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

    #[test]
    fn the_highest_peak_holds_its_segment_then_falls() {
        let mut meter = PeakMeter::default();
        meter.update(0.5, 0.016);
        assert_eq!(meter.held_segment(), None, "the level itself lights it");
        meter.update(0.0, 0.5);
        assert_eq!(meter.lit_segments(), 5);
        assert_eq!(meter.held_segment(), Some(7), "-6 dBFS stays lit");
        meter.update(0.0, 0.9);
        assert_eq!(meter.held_segment(), Some(7), "for a second and a half");
        meter.update(0.0, 0.5);
        assert_eq!(meter.lit_segments(), 0);
        assert_eq!(
            meter.held_segment(),
            Some(4),
            "then it falls as the level does"
        );
        meter.update(0.0, 2.0);
        assert_eq!(meter.held_segment(), None);
        assert!(!meter.is_moving());
    }

    #[test]
    fn a_peak_at_full_scale_lights_the_clip_light_for_two_seconds() {
        let mut meter = PeakMeter::default();
        meter.update(0.9, 0.016);
        assert!(!meter.clipping());
        meter.update(1.0, 0.016);
        assert!(meter.clipping());
        meter.update(0.1, 1.9);
        assert!(meter.clipping());
        assert!(
            meter.is_moving(),
            "the window keeps drawing until it goes out"
        );
        meter.update(0.1, 0.2);
        assert!(!meter.clipping());
    }
}
