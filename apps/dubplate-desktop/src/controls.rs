//! The mixer and tempo sliders: their positions, and how a pointer on a slider becomes a value.

use dubplate_engine::{DeckId, decibels_to_gain};
use gpui::{Bounds, Pixels, Point};

/// How far the tempo slider reaches each way: ±8%, a CDJ's default range.
pub const TEMPO_RANGE: f64 = 0.08;
/// How much one tempo key press changes the tempo: 0.1%.
pub const TEMPO_STEP: f64 = 0.001;
/// How far one crossfader key press moves the crossfader.
pub const CROSSFADER_STEP: f32 = 0.05;
/// The master level slider's range below unity, in decibels; the bottom mutes.
const MASTER_RANGE_DB: f32 = 40.0;

/// A slider the DJ can drag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slider {
    /// A deck's channel fader.
    ChannelFader(DeckId),
    /// The crossfader.
    Crossfader,
    /// A deck's tempo.
    Tempo(DeckId),
    /// The master level.
    MasterLevel,
}

/// Which way a slider runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    /// Left is 0.0, right is 1.0.
    Horizontal,
    /// Bottom is 0.0, top is 1.0.
    Vertical,
}

impl Slider {
    /// Every slider.
    pub const ALL: [Slider; 6] = [
        Slider::ChannelFader(DeckId::A),
        Slider::ChannelFader(DeckId::B),
        Slider::Crossfader,
        Slider::Tempo(DeckId::A),
        Slider::Tempo(DeckId::B),
        Slider::MasterLevel,
    ];

    /// Its position in [`Slider::ALL`].
    pub fn index(self) -> usize {
        match self {
            Slider::ChannelFader(DeckId::A) => 0,
            Slider::ChannelFader(DeckId::B) => 1,
            Slider::Crossfader => 2,
            Slider::Tempo(DeckId::A) => 3,
            Slider::Tempo(DeckId::B) => 4,
            Slider::MasterLevel => 5,
        }
    }

    /// Which way it runs.
    pub fn orientation(self) -> Orientation {
        match self {
            Slider::ChannelFader(_) => Orientation::Vertical,
            Slider::Crossfader | Slider::Tempo(_) | Slider::MasterLevel => Orientation::Horizontal,
        }
    }
}

/// Where along a slider drawn in `bounds` the pointer is, from 0.0 to 1.0.
pub fn fraction_at(
    position: Point<Pixels>,
    bounds: Bounds<Pixels>,
    orientation: Orientation,
) -> f32 {
    let fraction = match orientation {
        Orientation::Horizontal => {
            f32::from(position.x - bounds.origin.x) / f32::from(bounds.size.width).max(1.0)
        }
        Orientation::Vertical => {
            1.0 - f32::from(position.y - bounds.origin.y) / f32::from(bounds.size.height).max(1.0)
        }
    };
    fraction.clamp(0.0, 1.0)
}

/// The tempo at a tempo slider position, where the middle is the track's own tempo.
pub fn tempo_at(fraction: f32) -> f64 {
    1.0 + (f64::from(fraction) * 2.0 - 1.0) * TEMPO_RANGE
}

/// The tempo slider position for a tempo.
pub fn tempo_fraction(tempo: f64) -> f32 {
    (((tempo - 1.0) / TEMPO_RANGE + 1.0) / 2.0).clamp(0.0, 1.0) as f32
}

/// The tempo one key press away, kept inside the slider's range.
pub fn nudged_tempo(tempo: f64, sign: f64) -> f64 {
    let steps = ((tempo - 1.0) / TEMPO_STEP).round() + sign;
    (1.0 + steps * TEMPO_STEP).clamp(1.0 - TEMPO_RANGE, 1.0 + TEMPO_RANGE)
}

/// The master gain at a master level slider position.
pub fn master_gain_at(fraction: f32) -> f32 {
    if fraction <= 0.0 {
        0.0
    } else {
        decibels_to_gain(-MASTER_RANGE_DB * (1.0 - fraction))
    }
}

/// Where the mixer and tempo sliders are. The interface keeps them, rather than reading them
/// back from the engine, so quick key presses each count before the engine has caught up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SliderPositions {
    /// Each deck's tempo, where 1.0 is the track's own tempo.
    pub tempos: [f64; 2],
    /// Each deck's channel fader, from 0.0 to 1.0.
    pub channel_faders: [f32; 2],
    /// The crossfader, from 0.0 (deck A) to 1.0 (deck B).
    pub crossfader: f32,
    /// The master level slider, from 0.0 (muted) to 1.0 (unity).
    pub master_level: f32,
}

impl Default for SliderPositions {
    fn default() -> Self {
        Self {
            tempos: [1.0, 1.0],
            channel_faders: [1.0, 1.0],
            crossfader: 0.5,
            master_level: 1.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use gpui::{point, px, size};

    use super::*;

    #[test]
    fn every_slider_has_its_own_index() {
        for (position, slider) in Slider::ALL.into_iter().enumerate() {
            assert_eq!(slider.index(), position);
        }
    }

    #[test]
    fn the_pointer_maps_to_a_fraction_along_the_slider() {
        let bounds = Bounds::new(point(px(100.0), px(50.0)), size(px(200.0), px(100.0)));
        let middle = point(px(200.0), px(100.0));
        assert_eq!(fraction_at(middle, bounds, Orientation::Horizontal), 0.5);
        assert_eq!(fraction_at(middle, bounds, Orientation::Vertical), 0.5);
        let top_left = point(px(100.0), px(50.0));
        assert_eq!(fraction_at(top_left, bounds, Orientation::Horizontal), 0.0);
        assert_eq!(fraction_at(top_left, bounds, Orientation::Vertical), 1.0);
        let far_outside = point(px(900.0), px(900.0));
        assert_eq!(
            fraction_at(far_outside, bounds, Orientation::Horizontal),
            1.0
        );
        assert_eq!(fraction_at(far_outside, bounds, Orientation::Vertical), 0.0);
    }

    #[test]
    fn the_tempo_slider_spans_eight_percent_each_way() {
        assert!((tempo_at(0.5) - 1.0).abs() < 1e-9);
        assert!((tempo_at(1.0) - 1.08).abs() < 1e-9);
        assert!((tempo_at(0.0) - 0.92).abs() < 1e-9);
        assert!((tempo_fraction(1.04) - 0.75).abs() < 1e-6);
        assert_eq!(tempo_fraction(2.0), 1.0);
    }

    #[test]
    fn tempo_keys_step_by_a_tenth_of_a_percent_within_the_range() {
        assert!((nudged_tempo(1.0, 1.0) - 1.001).abs() < 1e-9);
        assert!((nudged_tempo(1.0123, -1.0) - 1.011).abs() < 1e-9);
        assert!((nudged_tempo(1.08, 1.0) - 1.08).abs() < 1e-9);
        assert!((nudged_tempo(0.92, -1.0) - 0.92).abs() < 1e-9);
    }

    #[test]
    fn the_master_level_runs_from_silence_to_unity() {
        assert_eq!(master_gain_at(0.0), 0.0);
        assert!((master_gain_at(1.0) - 1.0).abs() < 1e-6);
        assert!((master_gain_at(0.5) - 0.1).abs() < 1e-6);
    }
}
