//! Auto Gain, as rekordbox's Preferences > Controller > Mixer > Auto Gain: a track loads with
//! a gain that brings it to a common loudness, on top of the channel's TRIM knob. A rekordbox
//! track takes the gain rekordbox measured; a folder track takes the gain from the loudness
//! Dubplate's analysis measured. The TRIM knob stays where the DJ, or the controller's knob,
//! left it.

use dubplate_engine::decibels_to_gain;
use dubplate_prep::Lufs;

use crate::controls::{trim_fraction_for_gain, trim_gain_at};

/// The loudness Auto Gain brings a folder track to, as Dubplate's analysis measures it: where
/// rekordbox's own Auto Gain brought the median track of a DJ's rekordbox library, measured
/// with `dubplate analyse --compare-rekordbox`, so folder tracks and rekordbox tracks meet at
/// the same level.
pub const TARGET_LOUDNESS: Lufs = Lufs(-8.0);

/// The most Auto Gain cuts or boosts, in decibels, so a near-silent or broken file is never
/// pushed into the limiter.
const LIMIT_DB: f32 = 12.0;

/// Where a track's Auto Gain comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoGainSource {
    /// rekordbox's analysis.
    Rekordbox,
    /// Dubplate's analysis of the track's loudness.
    Analysis,
}

/// The gain Auto Gain gives a track.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutoGain {
    /// The gain, in decibels.
    pub decibels: f32,
    /// Where it comes from.
    pub source: AutoGainSource,
}

impl AutoGain {
    /// The gain of a track: rekordbox's when rekordbox analysed it, else the step from
    /// `loudness` to [`TARGET_LOUDNESS`], each kept within ±12 dB. `None` when neither is
    /// known yet.
    pub fn of(rekordbox_db: Option<f32>, loudness: Option<Lufs>) -> Option<Self> {
        let (decibels, source) = match (rekordbox_db, loudness) {
            (Some(decibels), _) => (decibels, AutoGainSource::Rekordbox),
            (None, Some(loudness)) => (TARGET_LOUDNESS.0 - loudness.0, AutoGainSource::Analysis),
            (None, None) => return None,
        };
        decibels.is_finite().then(|| Self {
            decibels: decibels.clamp(-LIMIT_DB, LIMIT_DB),
            source,
        })
    }

    /// The gain as a factor.
    pub fn factor(self) -> f32 {
        decibels_to_gain(self.decibels)
    }

    /// How the gain reads beside the TRIM knob, such as `AUTO +2.5 dB`.
    pub fn label(self) -> String {
        let tenths = (self.decibels * 10.0).round() / 10.0 + 0.0;
        format!("AUTO {tenths:+.1} dB")
    }
}

/// The gain a channel plays at: the TRIM knob's, times the track's Auto Gain.
pub fn channel_gain(trim_fraction: f32, auto_gain: Option<AutoGain>) -> f32 {
    trim_gain_at(trim_fraction) * auto_gain.map_or(1.0, AutoGain::factor)
}

/// Where the TRIM knob would sit for the channel's whole gain without Auto Gain, the mark the
/// knob shows beside its own position, when Auto Gain moves the level at all.
pub fn applied_trim_fraction(trim_fraction: f32, auto_gain: Option<AutoGain>) -> Option<f32> {
    let auto_gain = auto_gain.filter(|gain| gain.decibels.abs() >= SHOWN_DB)?;
    let gain = channel_gain(trim_fraction, Some(auto_gain));
    (gain > 0.0).then(|| trim_fraction_for_gain(gain))
}

/// An Auto Gain under this many decibels is not marked on the knob.
const SHOWN_DB: f32 = 0.05;

#[cfg(test)]
mod tests {
    use super::*;

    fn close(found: f32, expected: f32) -> bool {
        (found - expected).abs() < 1e-3
    }

    fn decibels_of(gain: f32) -> f32 {
        dubplate_engine::gain_to_decibels(gain)
    }

    #[test]
    fn rekordbox_gain_wins_and_analysis_reaches_the_target() {
        let rekordbox = AutoGain::of(Some(-3.2), Some(Lufs(-14.0))).unwrap();
        assert_eq!(rekordbox.source, AutoGainSource::Rekordbox);
        assert!(close(rekordbox.decibels, -3.2));

        let quiet = AutoGain::of(None, Some(Lufs(TARGET_LOUDNESS.0 - 4.5))).unwrap();
        assert_eq!(quiet.source, AutoGainSource::Analysis);
        assert!(close(quiet.decibels, 4.5));
        let loud = AutoGain::of(None, Some(Lufs(TARGET_LOUDNESS.0 + 2.0))).unwrap();
        assert!(close(loud.decibels, -2.0));

        assert_eq!(AutoGain::of(None, None), None);
    }

    #[test]
    fn auto_gain_stays_within_twelve_decibels() {
        let silence_ish = AutoGain::of(None, Some(Lufs(-60.0))).unwrap();
        assert!(close(silence_ish.decibels, 12.0));
        let broken = AutoGain::of(Some(40.0), None).unwrap();
        assert!(close(broken.decibels, 12.0));
        assert_eq!(AutoGain::of(Some(f32::NAN), None), None);
    }

    #[test]
    fn the_channel_plays_trim_and_auto_gain_together() {
        let gain = AutoGain::of(Some(-6.0), None);
        assert!(close(decibels_of(channel_gain(0.5, gain)), -6.0));
        assert!(close(decibels_of(channel_gain(1.0, gain)), 3.0), "TRIM +9 dB, AUTO -6 dB");
        assert!(close(channel_gain(0.5, None), 1.0));
        assert_eq!(channel_gain(0.0, gain), 0.0, "TRIM fully left mutes the channel");
    }

    #[test]
    fn the_knob_marks_where_the_level_really_is() {
        let cut = AutoGain::of(Some(-6.0), None);
        let marked = applied_trim_fraction(0.5, cut).unwrap();
        assert!(close(decibels_of(trim_gain_at(marked)), -6.0));
        assert!(marked < 0.5);

        let boost = AutoGain::of(Some(4.5), None);
        let marked = applied_trim_fraction(0.5, boost).unwrap();
        assert!(close(marked, 0.75), "+4.5 dB is half the knob's +9 dB");

        let beyond = applied_trim_fraction(1.0, boost).unwrap();
        assert!(close(beyond, 1.0), "a level past the knob's end marks the end");

        assert_eq!(applied_trim_fraction(0.5, None), None);
        assert_eq!(applied_trim_fraction(0.5, AutoGain::of(Some(0.01), None)), None);
    }

    #[test]
    fn the_label_reads_like_a_mixer_readout() {
        assert_eq!(AutoGain::of(Some(2.46), None).unwrap().label(), "AUTO +2.5 dB");
        assert_eq!(AutoGain::of(Some(-0.04), None).unwrap().label(), "AUTO +0.0 dB");
    }
}
