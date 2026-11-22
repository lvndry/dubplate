//! The mixer and tempo sliders: their positions, and how a pointer on a slider becomes a value.

use dubplate_engine::{DeckId, EqBand, Stem, TempoRange, decibels_to_gain};
use gpui::{Bounds, Pixels, Point};

/// How far one crossfader key press moves the crossfader.
pub const CROSSFADER_STEP: f32 = 0.05;
/// The master level slider's range below unity, in decibels; the bottom mutes.
const MASTER_RANGE_DB: f32 = 40.0;
/// How far the trim knob boosts at its right end, in decibels.
const TRIM_BOOST_DB: f32 = 9.0;
/// How far the trim knob cuts just before its left end, in decibels; the end mutes.
const TRIM_CUT_DB: f32 = 30.0;

/// A fader or knob of the mixer or a deck, with a position from 0.0 to 1.0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Control {
    /// A deck's channel fader.
    ChannelFader(DeckId),
    /// The crossfader.
    Crossfader,
    /// A deck's tempo.
    Tempo(DeckId),
    /// The master level.
    MasterLevel,
    /// A deck's trim, unity in the middle.
    Trim(DeckId),
    /// One of a deck's EQ knobs: killed at the left, unity in the middle, +6 dB at the right.
    Eq(DeckId, EqBand),
    /// A deck's filter knob (CFX): low-pass to the left, flat in the middle, high-pass to the
    /// right.
    Filter(DeckId),
    /// The headphone MIXING knob: the cued channels to the left, the master to the right.
    HeadphoneMix,
    /// The headphone LEVEL knob.
    HeadphoneLevel,
    /// The sampler's volume.
    SamplerVolume,
    /// The level of one of a deck's stems, on the channel fader's curve.
    StemLevel(DeckId, Stem),
}

impl Control {
    /// Where the control rests: a double-click puts it back there.
    pub fn resting(self) -> f32 {
        match self {
            Control::ChannelFader(_) | Control::MasterLevel | Control::StemLevel(..) => 1.0,
            Control::Crossfader
            | Control::Tempo(_)
            | Control::Trim(_)
            | Control::Eq(..)
            | Control::Filter(_)
            | Control::HeadphoneMix
            | Control::HeadphoneLevel => 0.5,
            Control::SamplerVolume => DEFAULT_SAMPLER_VOLUME,
        }
    }
}

impl From<Slider> for Control {
    fn from(slider: Slider) -> Self {
        match slider {
            Slider::ChannelFader(deck) => Control::ChannelFader(deck),
            Slider::Crossfader => Control::Crossfader,
            Slider::Tempo(deck) => Control::Tempo(deck),
            Slider::MasterLevel => Control::MasterLevel,
        }
    }
}

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

/// The engine's tempo fader position, -1.0 to 1.0, for a tempo slider at `fraction`.
pub fn tempo_fader(fraction: f32) -> f64 {
    f64::from(fraction.clamp(0.0, 1.0)) * 2.0 - 1.0
}

/// Where a tempo slider sits for `tempo`, the speed where 1.0 is the track's own tempo, in
/// `range`: the inverse of [`tempo_fader`], kept to the slider's ends.
pub fn tempo_slider_at(tempo: f64, range: TempoRange) -> f32 {
    let position = (tempo - 1.0) / range.span();
    ((position + 1.0) / 2.0).clamp(0.0, 1.0) as f32
}

/// The tempo slider one key press away: the range's next step up or down, 0.02% at ±6% as on a
/// CDJ. A slider dragged between two steps goes to the nearer one in the pressed direction.
pub fn nudged_tempo_slider(fraction: f32, sign: f64, range: TempoRange) -> f32 {
    // The slider is an f32, so a tempo on a step can read a few hundred-thousandths off it.
    const ON_A_STEP: f64 = 1e-3;
    let steps_per_side = range.span() / range.step();
    let steps_now = tempo_fader(fraction) * steps_per_side;
    let steps = if sign > 0.0 {
        (steps_now + ON_A_STEP).floor() + 1.0
    } else {
        (steps_now - ON_A_STEP).ceil() - 1.0
    };
    let position = (steps / steps_per_side).clamp(-1.0, 1.0);
    ((position + 1.0) / 2.0) as f32
}

/// How the tempo range reads beside the slider, such as `±10%`.
pub fn tempo_range_label(range: TempoRange) -> String {
    format!("\u{b1}{:.0}%", range.span() * 100.0)
}

/// The master gain at a master level slider position.
pub fn master_gain_at(fraction: f32) -> f32 {
    if fraction <= 0.0 {
        0.0
    } else {
        decibels_to_gain(-MASTER_RANGE_DB * (1.0 - fraction))
    }
}

/// The trim gain at a trim knob position: unity in the middle, muted at the left end.
pub fn trim_gain_at(fraction: f32) -> f32 {
    if fraction <= 0.0 {
        return 0.0;
    }
    let decibels = if fraction >= 0.5 {
        TRIM_BOOST_DB * (fraction * 2.0 - 1.0)
    } else {
        -TRIM_CUT_DB * (1.0 - fraction * 2.0)
    };
    decibels_to_gain(decibels)
}

/// Where the sampler's volume starts, a little under its loudest so a sample sits in the mix.
pub const DEFAULT_SAMPLER_VOLUME: f32 = 0.8;
/// The master level the first time Dubplate opens, 10 dB under full, before the DJ or the
/// controller's MASTER LEVEL knob sets it.
pub const DEFAULT_MASTER_LEVEL: f32 = 0.75;

/// Where the mixer and tempo controls are. The interface keeps them, rather than reading them
/// back from the engine, so quick key presses each count before the engine has caught up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MixerPositions {
    /// Each deck's tempo slider, from 0.0 (slowest) through 0.5 (the track's own tempo) to 1.0
    /// (fastest) across the deck's tempo range.
    pub tempo_sliders: [f32; 2],
    /// Each deck's channel fader, from 0.0 to 1.0.
    pub channel_faders: [f32; 2],
    /// The crossfader, from 0.0 (deck A) to 1.0 (deck B).
    pub crossfader: f32,
    /// The master level slider, from 0.0 (muted) to 1.0 (unity).
    pub master_level: f32,
    /// Each deck's trim knob, from 0.0 to 1.0 with unity in the middle.
    pub trims: [f32; 2],
    /// Each deck's EQ knobs, in the order of [`EqBand::ALL`], unity in the middle.
    pub eqs: [[f32; 3]; 2],
    /// Each deck's filter knob, flat in the middle.
    pub filters: [f32; 2],
    /// The headphone MIXING knob, cue only at 0.0 and master only at 1.0.
    pub headphone_mix: f32,
    /// The headphone LEVEL knob.
    pub headphone_level: f32,
    /// The sampler's volume, on the channel fader's curve.
    pub sampler_volume: f32,
    /// Each deck's stem levels, in the order of [`Stem::ALL`], on the channel fader's curve.
    pub stem_levels: [[f32; Stem::COUNT]; 2],
}

impl MixerPositions {
    /// Where `control` is, from 0.0 to 1.0.
    pub fn fraction(&self, control: Control) -> f32 {
        match control {
            Control::ChannelFader(deck) => self.channel_faders[deck.index()],
            Control::Crossfader => self.crossfader,
            Control::Tempo(deck) => self.tempo_sliders[deck.index()],
            Control::MasterLevel => self.master_level,
            Control::Trim(deck) => self.trims[deck.index()],
            Control::Eq(deck, band) => self.eqs[deck.index()][band.index()],
            Control::Filter(deck) => self.filters[deck.index()],
            Control::HeadphoneMix => self.headphone_mix,
            Control::HeadphoneLevel => self.headphone_level,
            Control::SamplerVolume => self.sampler_volume,
            Control::StemLevel(deck, stem) => self.stem_levels[deck.index()][stem.index()],
        }
    }
}

impl Default for MixerPositions {
    fn default() -> Self {
        Self {
            tempo_sliders: [0.5, 0.5],
            channel_faders: [1.0, 1.0],
            crossfader: 0.5,
            master_level: 1.0,
            trims: [0.5, 0.5],
            eqs: [[0.5; 3]; 2],
            filters: [0.5, 0.5],
            headphone_mix: 0.5,
            headphone_level: 0.5,
            sampler_volume: DEFAULT_SAMPLER_VOLUME,
            stem_levels: [[1.0; Stem::COUNT]; 2],
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
    fn the_tempo_slider_drives_the_engines_fader_and_keys_step_the_ranges_finest_step() {
        assert_eq!(tempo_fader(0.5), 0.0);
        assert_eq!(tempo_fader(0.0), -1.0);
        assert_eq!(tempo_fader(1.0), 1.0);
        let tempo_after = |fraction: f32, sign: f64, range: TempoRange| {
            1.0 + tempo_fader(nudged_tempo_slider(fraction, sign, range)) * range.span()
        };
        assert!((tempo_after(0.5, 1.0, TempoRange::SixPercent) - 1.0002).abs() < 1e-6);
        assert!((tempo_after(0.5, -1.0, TempoRange::TenPercent) - 0.9996).abs() < 1e-6);
        assert!((tempo_after(0.5, 1.0, TempoRange::Wide) - 1.005).abs() < 1e-6);
        let between_steps = tempo_slider_at(1.000_13, TempoRange::SixPercent);
        assert!((tempo_after(between_steps, 1.0, TempoRange::SixPercent) - 1.0002).abs() < 1e-6);
        assert!((tempo_after(between_steps, -1.0, TempoRange::SixPercent) - 1.0).abs() < 1e-6);
        let mut fraction = 0.5;
        for _ in 0..7 {
            fraction = nudged_tempo_slider(fraction, 1.0, TempoRange::SixPercent);
        }
        let seven_steps_up = 1.0 + tempo_fader(fraction) * TempoRange::SixPercent.span();
        assert!((seven_steps_up - 1.0014).abs() < 1e-5, "{seven_steps_up}");
        assert_eq!(nudged_tempo_slider(1.0, 1.0, TempoRange::SixPercent), 1.0);
        assert_eq!(nudged_tempo_slider(0.0, -1.0, TempoRange::SixPercent), 0.0);
        assert_eq!(tempo_range_label(TempoRange::SixteenPercent), "\u{b1}16%");
        let in_between = tempo_slider_at(1.03, TempoRange::TenPercent);
        assert!((in_between - 0.65).abs() < 1e-6, "{in_between}");
        assert!((tempo_fader(in_between) - 0.3).abs() < 1e-6);
        assert_eq!(tempo_slider_at(1.2, TempoRange::SixPercent), 1.0);
        assert_eq!(tempo_slider_at(1.0, TempoRange::Wide), 0.5);
        assert_eq!(tempo_range_label(TempoRange::Wide), "\u{b1}100%");
    }

    #[test]
    fn the_trim_is_unity_in_the_middle_and_mutes_at_the_left() {
        assert!((trim_gain_at(0.5) - 1.0).abs() < 1e-6);
        assert_eq!(trim_gain_at(0.0), 0.0);
        assert!((trim_gain_at(1.0) - decibels_to_gain(9.0)).abs() < 1e-5);
        assert!(trim_gain_at(0.25) < 1.0);
    }

    #[test]
    fn every_slider_is_a_control_resting_where_a_double_click_puts_it() {
        assert_eq!(Control::from(Slider::MasterLevel).resting(), 1.0);
        assert_eq!(Control::from(Slider::Tempo(DeckId::B)).resting(), 0.5);
        let positions = MixerPositions::default();
        for slider in Slider::ALL {
            let control = Control::from(slider);
            assert_eq!(positions.fraction(control), control.resting(), "{slider:?}");
        }
    }

    #[test]
    fn the_master_level_runs_from_silence_to_unity() {
        assert_eq!(master_gain_at(0.0), 0.0);
        assert!((master_gain_at(1.0) - 1.0).abs() < 1e-6);
        assert!((master_gain_at(0.5) - 0.1).abs() < 1e-6);
    }
}
