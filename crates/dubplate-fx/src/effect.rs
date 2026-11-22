use std::fmt;
use std::str::FromStr;

use crate::beat::BeatLength;
use crate::config::EffectConfig;
use crate::context::ProcessContext;
use crate::effects::backspin::Backspin;
use crate::effects::beat_filter::BeatFilter;
use crate::effects::brake::Brake;
use crate::effects::crusher::Crusher;
use crate::effects::echo::Echo;
use crate::effects::filter::{Filter, FilterMode};
use crate::effects::flanger::Flanger;
use crate::effects::gate::Gate;
use crate::effects::phaser::Phaser;
use crate::effects::ping_pong::PingPong;
use crate::effects::reverb::Reverb;
use crate::parameter::{ParameterSpec, ParameterValue, parameter_spec};

/// The name the beat filter had when it was the mixer's one-knob filter, still read from saved
/// settings.
const FORMER_FILTER_NAME: &str = "sweep";

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

    /// Whether the effect needs to hear the music while it is switched off, so it can act on the
    /// live music the moment it is switched on. Brake and backspin do; the other effects do not,
    /// and an engine may skip them entirely while they are off and not ringing.
    fn needs_input_when_off(&self) -> bool {
        false
    }

    /// Sets the balance between the untouched music (0) and the effect (1).
    fn set_mix(&mut self, mix: f32);

    /// The mix the effect is moving to.
    fn mix(&self) -> f32;

    /// Turns the single performance knob found on DJ mixers, from 0 to 1. Each effect maps it
    /// to the controls that make it sound deeper; the documentation of each effect says how.
    fn set_depth(&mut self, depth: f32);

    /// Sets the effect's parameter, the value the BEAT buttons of a DJ mixer step through: a
    /// beat length for time-based effects, a percentage or a pitch for the others. See
    /// [`EffectKind::parameter`]. A value of another unit than the effect's is ignored.
    fn set_parameter(&mut self, value: ParameterValue);

    /// The effect's parameter, or `None` for an effect without one.
    fn parameter(&self) -> Option<ParameterValue>;

    /// Sets the beat length of a beat-synced effect. Effects without one ignore it.
    fn set_beat(&mut self, beat: BeatLength) {
        self.set_parameter(ParameterValue::Beats(beat));
    }

    /// The beat length of a beat-synced effect, or `None` for effects without one.
    fn beat(&self) -> Option<BeatLength> {
        self.parameter().and_then(ParameterValue::beats)
    }

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
    /// A low-pass filter that sweeps down and back up in time with the beat.
    Filter,
    /// A resonant low-pass filter.
    LowPass,
    /// A resonant high-pass filter.
    HighPass,
    /// A resonant band-pass filter.
    BandPass,
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
        Self::Filter,
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

    /// Where the effect sits in [`EffectKind::ALL`], which lists the effects in the order they
    /// are declared.
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The effect's parameter: what it sets, the values the BEAT buttons step through and its
    /// default, or `None` for an effect without one.
    pub fn parameter(self) -> Option<&'static ParameterSpec> {
        parameter_spec(self)
    }

    /// The short name used on screen and on the command line.
    pub fn name(self) -> &'static str {
        match self {
            Self::Echo => "echo",
            Self::PingPong => "ping-pong",
            Self::Reverb => "reverb",
            Self::LowPass => "low-pass",
            Self::HighPass => "high-pass",
            Self::BandPass => "band-pass",
            Self::Filter => "filter",
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
        let lowercase = text.trim().to_ascii_lowercase();
        let wanted = match lowercase.as_str() {
            FORMER_FILTER_NAME => Self::Filter.name(),
            other => other,
        };
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
        EffectKind::Filter => Box::new(BeatFilter::new()),
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
        for (index, kind) in EffectKind::ALL.into_iter().enumerate() {
            assert_eq!(kind.index(), index, "{kind} is declared out of order");
        }
        assert_eq!("sweep".parse::<EffectKind>(), Ok(EffectKind::Filter));
    }
}
