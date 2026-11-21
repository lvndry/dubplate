use std::fmt;
use std::str::FromStr;

use crate::beat::BeatLength;
use crate::config::EffectConfig;
use crate::context::ProcessContext;
use crate::effects::backspin::Backspin;
use crate::effects::brake::Brake;
use crate::effects::crusher::Crusher;
use crate::effects::echo::Echo;
use crate::effects::filter::{Filter, FilterMode};
use crate::effects::flanger::Flanger;
use crate::effects::gate::Gate;
use crate::effects::phaser::Phaser;
use crate::effects::ping_pong::PingPong;
use crate::effects::reverb::Reverb;
use crate::effects::sweep::SweepFilter;

/// A DJ effect that processes stereo audio in place on the audio thread.
///
/// Every method is real-time safe: none allocates, locks or blocks, so an engine can call them
/// from its audio callback. Controls take effect smoothly over the next few milliseconds.
pub trait Effect: Send {
    /// Which effect this is.
    fn kind(&self) -> EffectKind;

    /// Processes one block of planar stereo audio in place. When the two slices differ in
    /// length, only the frames both hold are processed.
    fn process(&mut self, left: &mut [f32], right: &mut [f32], context: &ProcessContext);

    /// Switches the effect on or off with a short fade. Echo, ping-pong and reverb keep
    /// ringing after they are switched off. Brake and backspin start their stop when switched
    /// on and return to the live music when switched off.
    fn set_enabled(&mut self, enabled: bool);

    /// Whether the effect is switched on.
    fn is_enabled(&self) -> bool;

    /// Whether the effect is still changing the sound: switched on, fading out, or ringing
    /// out a trail. An inactive effect passes audio through untouched and costs almost nothing.
    fn is_active(&self) -> bool;

    /// Sets the balance between the untouched music (0) and the effect (1).
    fn set_mix(&mut self, mix: f32);

    /// The mix the effect is moving to.
    fn mix(&self) -> f32;

    /// Turns the single performance knob found on DJ mixers, from 0 to 1. Each effect maps it
    /// to the controls that make it sound deeper; the documentation of each effect says how.
    fn set_depth(&mut self, depth: f32);

    /// Sets the beat length of a beat-synced effect. Effects without one ignore it.
    fn set_beat(&mut self, beat: BeatLength);

    /// The beat length of a beat-synced effect, or `None` for effects without one.
    fn beat(&self) -> Option<BeatLength>;

    /// Clears every buffer and filter, silencing any trail at once.
    fn reset(&mut self);
}

/// Every effect Dubplate offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EffectKind {
    /// A tempo-synced echo with feedback.
    Echo,
    /// An echo that bounces between left and right.
    PingPong,
    /// A room-to-hall reverb.
    Reverb,
    /// A resonant low-pass filter.
    LowPass,
    /// A resonant high-pass filter.
    HighPass,
    /// A resonant band-pass filter.
    BandPass,
    /// The one-knob filter of a DJ mixer: low-pass to the left, high-pass to the right.
    SweepFilter,
    /// A beat-synced jet flanger.
    Flanger,
    /// A beat-synced phaser.
    Phaser,
    /// A beat-synced trance gate.
    Gate,
    /// A bit crusher with sample-rate reduction.
    Crusher,
    /// A turntable stopping.
    Brake,
    /// A record spun backwards.
    Backspin,
}

impl EffectKind {
    /// Every effect, in the order a DJ would browse them.
    pub const ALL: [Self; 13] = [
        Self::Echo,
        Self::PingPong,
        Self::Reverb,
        Self::SweepFilter,
        Self::LowPass,
        Self::HighPass,
        Self::BandPass,
        Self::Flanger,
        Self::Phaser,
        Self::Gate,
        Self::Crusher,
        Self::Brake,
        Self::Backspin,
    ];

    /// The short name used on screen and on the command line.
    pub fn name(self) -> &'static str {
        match self {
            Self::Echo => "echo",
            Self::PingPong => "ping-pong",
            Self::Reverb => "reverb",
            Self::LowPass => "low-pass",
            Self::HighPass => "high-pass",
            Self::BandPass => "band-pass",
            Self::SweepFilter => "sweep",
            Self::Flanger => "flanger",
            Self::Phaser => "phaser",
            Self::Gate => "gate",
            Self::Crusher => "crusher",
            Self::Brake => "brake",
            Self::Backspin => "backspin",
        }
    }
}

impl fmt::Display for EffectKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

/// The error returned when text names no effect.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseEffectKindError {
    text: String,
}

impl fmt::Display for ParseEffectKindError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "`{}` is not an effect; choose one of: ",
            self.text
        )?;
        for (index, kind) in EffectKind::ALL.iter().enumerate() {
            if index > 0 {
                formatter.write_str(", ")?;
            }
            write!(formatter, "{kind}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ParseEffectKindError {}

impl FromStr for EffectKind {
    type Err = ParseEffectKindError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let wanted = text.trim().to_ascii_lowercase();
        Self::ALL
            .into_iter()
            .find(|kind| kind.name() == wanted)
            .ok_or_else(|| ParseEffectKindError {
                text: text.to_owned(),
            })
    }
}

/// Builds an effect with its default settings, switched off.
///
/// This allocates the effect's buffers, so call it outside the audio thread.
pub fn create(kind: EffectKind, config: &EffectConfig) -> Box<dyn Effect> {
    match kind {
        EffectKind::Echo => Box::new(Echo::new(config)),
        EffectKind::PingPong => Box::new(PingPong::new(config)),
        EffectKind::Reverb => Box::new(Reverb::new(config)),
        EffectKind::LowPass => Box::new(Filter::new(FilterMode::LowPass)),
        EffectKind::HighPass => Box::new(Filter::new(FilterMode::HighPass)),
        EffectKind::BandPass => Box::new(Filter::new(FilterMode::BandPass)),
        EffectKind::SweepFilter => Box::new(SweepFilter::new()),
        EffectKind::Flanger => Box::new(Flanger::new(config)),
        EffectKind::Phaser => Box::new(Phaser::new()),
        EffectKind::Gate => Box::new(Gate::new()),
        EffectKind::Crusher => Box::new(Crusher::new()),
        EffectKind::Brake => Box::new(Brake::new(config)),
        EffectKind::Backspin => Box::new(Backspin::new(config)),
    }
}

#[cfg(test)]
mod tests {
    use super::EffectKind;

    #[test]
    fn every_name_parses_back_to_its_effect() {
        for kind in EffectKind::ALL {
            assert_eq!(kind.name().parse::<EffectKind>(), Ok(kind));
        }
        assert!("wah".parse::<EffectKind>().is_err());
    }
}
