//! How loud each stem of a deck plays, and the smoothing that keeps every change click-free.

use crate::curves::Ramp;
use crate::stems::Stem;

/// How long a stem takes to fade out, fade in or reach a new level.
pub(crate) const STEM_FADE_SECONDS: f64 = 0.02;

/// A stem's level: a gain from silent at 0 to the stem's own level in the track at 1.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct StemLevel(f32);

impl StemLevel {
    /// The stem as loud as it is in the track.
    pub const FULL: StemLevel = StemLevel(1.0);
    /// The stem silent.
    pub const SILENT: StemLevel = StemLevel(0.0);

    /// A level of `gain`, kept between silent and full; not a number counts as full.
    pub fn new(gain: f32) -> Self {
        if gain.is_nan() {
            Self::FULL
        } else {
            Self(gain.clamp(0.0, 1.0))
        }
    }

    /// The gain, from 0 to 1.
    pub fn gain(self) -> f32 {
        self.0
    }
}

impl Default for StemLevel {
    fn default() -> Self {
        Self::FULL
    }
}

/// The level of each stem of a deck and whether it is muted. A muted stem keeps its level for
/// when it comes back.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct StemSettings {
    levels: [StemLevel; Stem::COUNT],
    muted: [bool; Stem::COUNT],
}

impl StemSettings {
    /// Every stem at full level, none muted: the track as it is.
    pub const UNTOUCHED: StemSettings = StemSettings {
        levels: [StemLevel::FULL; Stem::COUNT],
        muted: [false; Stem::COUNT],
    };

    /// The level of `stem`.
    pub fn level(&self, stem: Stem) -> StemLevel {
        self.levels[stem.index()]
    }

    /// Whether `stem` is muted.
    pub fn is_muted(&self, stem: Stem) -> bool {
        self.muted[stem.index()]
    }

    /// Whether `stem` is heard: not muted and above silent.
    pub fn is_heard(&self, stem: Stem) -> bool {
        !self.is_muted(stem) && self.level(stem) > StemLevel::SILENT
    }

    /// These settings with `stem` at `level`.
    #[must_use]
    pub fn with_level(mut self, stem: Stem, level: StemLevel) -> Self {
        self.levels[stem.index()] = level;
        self
    }

    /// These settings with `stem` muted or not.
    #[must_use]
    pub fn with_muted(mut self, stem: Stem, muted: bool) -> Self {
        self.muted[stem.index()] = muted;
        self
    }

    /// These settings with `stem` muted if it played and back if it was muted.
    #[must_use]
    pub fn toggled(self, stem: Stem) -> Self {
        self.with_muted(stem, !self.is_muted(stem))
    }

    /// These settings with only `stem` heard, the others muted: [`Stem::Vocals`] gives the
    /// acapella.
    #[must_use]
    pub fn solo(mut self, stem: Stem) -> Self {
        self.muted = Stem::ALL.map(|other| other != stem);
        self
    }

    /// These settings with `stem` muted and every other stem back: [`Stem::Vocals`] gives the
    /// instrumental.
    #[must_use]
    pub fn without(mut self, stem: Stem) -> Self {
        self.muted = Stem::ALL.map(|other| other == stem);
        self
    }

    /// Whether only `stem` is muted, as [`StemSettings::without`] leaves it.
    pub fn is_without(&self, stem: Stem) -> bool {
        self.muted == Stem::ALL.map(|other| other == stem)
    }

    /// Whether only `stem` is unmuted, as [`StemSettings::solo`] leaves it.
    pub fn is_solo(&self, stem: Stem) -> bool {
        self.muted == Stem::ALL.map(|other| other != stem)
    }

    /// Whether every stem plays at full level, so the deck sounds like the track.
    pub fn is_untouched(&self) -> bool {
        *self == Self::UNTOUCHED
    }

    /// The gain each stem should reach, in the order of [`Stem::ALL`].
    pub(crate) fn gains(&self) -> [f32; Stem::COUNT] {
        Stem::ALL.map(|stem| {
            if self.is_muted(stem) {
                0.0
            } else {
                self.level(stem).gain()
            }
        })
    }
}

/// The gains a deck's stems play at, each gliding to its new value over [`STEM_FADE_SECONDS`]
/// so muting, unmuting and level moves never click.
pub(crate) struct StemMix {
    settings: StemSettings,
    gains: [Ramp; Stem::COUNT],
}

impl StemMix {
    pub(crate) fn new(output_rate: u32) -> Self {
        let fade_frames = (f64::from(output_rate) * STEM_FADE_SECONDS).round().max(1.0) as u32;
        Self {
            settings: StemSettings::UNTOUCHED,
            gains: std::array::from_fn(|_| Ramp::new(1.0, fade_frames)),
        }
    }

    pub(crate) fn settings(&self) -> StemSettings {
        self.settings
    }

    /// Starts every stem gliding to the gain of `settings`.
    pub(crate) fn set(&mut self, settings: StemSettings) {
        self.settings = settings;
        for (ramp, gain) in self.gains.iter_mut().zip(settings.gains()) {
            if ramp.target() != gain {
                ramp.set_target(gain);
            }
        }
    }

    /// The gains of the next output frame.
    #[inline]
    pub(crate) fn next_gains(&mut self) -> [f32; Stem::COUNT] {
        [
            self.gains[0].next_value(),
            self.gains[1].next_value(),
            self.gains[2].next_value(),
            self.gains[3].next_value(),
        ]
    }

    /// Puts every stem at the gain of the settings at once, for a new track.
    pub(crate) fn settle(&mut self) {
        for (ramp, gain) in self.gains.iter_mut().zip(self.settings.gains()) {
            ramp.jump_to(gain);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instrumental_and_acapella_are_one_touch_each() {
        let instrumental = StemSettings::UNTOUCHED.without(Stem::Vocals);
        assert!(instrumental.is_without(Stem::Vocals));
        assert_eq!(instrumental.gains(), [1.0, 1.0, 1.0, 0.0]);
        let acapella = instrumental.solo(Stem::Vocals);
        assert!(acapella.is_solo(Stem::Vocals));
        assert_eq!(acapella.gains(), [0.0, 0.0, 0.0, 1.0]);
        assert!(!acapella.is_without(Stem::Vocals));
    }

    #[test]
    fn a_muted_stem_keeps_its_level_for_when_it_comes_back() {
        let settings = StemSettings::UNTOUCHED
            .with_level(Stem::Bass, StemLevel::new(0.5))
            .toggled(Stem::Bass);
        assert_eq!(settings.gains()[Stem::Bass.index()], 0.0);
        assert!(!settings.is_heard(Stem::Bass));
        let back = settings.toggled(Stem::Bass);
        assert_eq!(back.gains()[Stem::Bass.index()], 0.5);
        assert!(!back.is_untouched());
    }

    #[test]
    fn levels_stay_between_silent_and_full() {
        assert_eq!(StemLevel::new(2.0), StemLevel::FULL);
        assert_eq!(StemLevel::new(-1.0), StemLevel::SILENT);
        assert_eq!(StemLevel::new(f32::NAN), StemLevel::FULL);
    }

    #[test]
    fn a_stem_fades_out_over_twenty_milliseconds() {
        let mut mix = StemMix::new(48_000);
        mix.set(StemSettings::UNTOUCHED.with_muted(Stem::Vocals, true));
        let fade_frames = 960;
        let mut previous = 1.0;
        for _ in 0..fade_frames - 1 {
            let gain = mix.next_gains()[Stem::Vocals.index()];
            assert!(gain < previous && gain > 0.0);
            previous = gain;
        }
        assert_eq!(mix.next_gains(), [1.0, 1.0, 1.0, 0.0]);
    }
}
