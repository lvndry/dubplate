use std::fmt;
use std::str::FromStr;

use crate::beat::BeatLength;
use crate::config::EffectConfig;
use crate::context::ProcessContext;
use crate::effects::delays::echo::Echo;
use crate::effects::delays::multi_tap::MultiTapDelay;
use crate::effects::delays::ping_pong::PingPong;
use crate::effects::delays::reverse::ReverseDelay;
use crate::effects::delays::voicings::{
    Climbing, Crushed, Diffused, EchoFilter, Filtered, Layered, Noise, Outgoing, Single,
};
use crate::effects::delays::{EchoEffect, Voicing};
use crate::effects::filters::beat_filter::BeatFilter;
use crate::effects::filters::filter::{Filter, FilterMode};
use crate::effects::lofi::crusher::Crusher;
use crate::effects::modulation::enigma_jet::EnigmaJet;
use crate::effects::modulation::flanger::Flanger;
use crate::effects::modulation::gate::Gate;
use crate::effects::modulation::mobius::Mobius;
use crate::effects::modulation::pan::Pan;
use crate::effects::modulation::phaser::Phaser;
use crate::effects::pitch::pitch::Pitch;
use crate::effects::pitch::robot::Robot;
use crate::effects::release::backspin::Backspin;
use crate::effects::release::brake::Brake;
use crate::effects::release::vinyl_brake::VinylBrake;
use crate::effects::reverb::Reverb;
use crate::effects::reverb::shimmer::ShimmerReverb;
use crate::effects::rolls::roll::{RollEffect, RollStyle};
use crate::effects::rolls::stretch::Stretch;
use crate::parameter::{ParameterSpec, ParameterValue, parameter_spec};

/// Names effects had before, still read from saved settings: the beat filter was the mixer's
/// one-knob filter, and Trans was the gate.
const FORMER_NAMES: [(&str, EffectKind); 2] =
    [("sweep", EffectKind::Filter), ("gate", EffectKind::Trans)];

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

/// Every effect Dubplate offers: rekordbox's Beat FX in the order rekordbox lists them, then
/// Dubplate's own filters, crusher, brake and backspin.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EffectKind {
    /// A single repeat on the beat.
    Delay,
    /// Repeats on the beat that fade away, aged like tape.
    Echo,
    /// An echo that blooms into a reverb and bends in pitch when its time changes.
    Spiral,
    /// A room-to-hall reverb.
    Reverb,
    /// Each beat length played back backwards.
    RevDelay,
    /// Up to seven repeats an eighth of a beat apart.
    MtDelay,
    /// An echo whose repeats climb in pitch.
    UpEcho,
    /// An echo whose repeats fall in pitch.
    DownEcho,
    /// A gate that chops the music on the beat.
    Trans,
    /// An auto pan throwing the music left and right on the beat.
    Pan,
    /// A low-pass filter that sweeps down and back up in time with the beat.
    Filter,
    /// A beat-synced jet flanger.
    Flanger,
    /// A beat-synced phaser.
    Phaser,
    /// A roll that records again whenever its length changes.
    SlipRoll,
    /// The music from the moment it is switched on, repeated every beat length.
    Roll,
    /// A roll played backwards.
    RevRoll,
    /// A robot's voice tuned to one pitch.
    Robot,
    /// The music shifted in pitch without changing its tempo.
    Pitch,
    /// A jet flanger that rises or falls forever.
    EnigmaJet,
    /// A sawtooth tone that rises or falls forever over the music.
    MobiusSaw,
    /// A triangle tone that rises or falls forever over the music.
    MobiusTriangle,
    /// An echo with its lows cut.
    LowCutEcho,
    /// An echo that bounces between left and right.
    PingPong,
    /// Repeats that layer on each other, freezing fully right.
    Helix,
    /// A record slowed by hand, as far as the knob turns.
    VinylBrake,
    /// The music slowed without changing its pitch, restarting on every cycle.
    Stretch,
    /// An echo through a band-pass the knob sweeps up.
    BpfEcho,
    /// Filtered white noise echoed on the beat, a build-up's riser.
    Noise,
    /// A spiral whose repeats climb in pitch.
    SpiralUp,
    /// A reverb whose tail climbs an octave at a time.
    ReverbUp,
    /// An echo through a high-pass the knob sweeps up.
    HpfEcho,
    /// An echo through a low-pass the knob sweeps down.
    LpfEcho,
    /// An echo that crushes its repeats.
    CrushEcho,
    /// A spiral whose repeats fall in pitch.
    SpiralDown,
    /// A reverb whose tail falls an octave at a time.
    ReverbDown,
    /// A resonant low-pass filter.
    LowPass,
    /// A resonant high-pass filter.
    HighPass,
    /// A resonant band-pass filter.
    BandPass,
    /// A bit crusher with sample-rate reduction.
    Crusher,
    /// A turntable stopping.
    Brake,
    /// A record spun backwards.
    Backspin,
    /// The last beat length echoed while the music cuts out: the afro house exit.
    EchoOut,
}

impl EffectKind {
    /// Every effect, in the order FX SELECT steps through them.
    pub const ALL: [Self; 42] = [
        Self::Delay,
        Self::Echo,
        Self::Spiral,
        Self::Reverb,
        Self::RevDelay,
        Self::MtDelay,
        Self::UpEcho,
        Self::DownEcho,
        Self::Trans,
        Self::Pan,
        Self::Filter,
        Self::Flanger,
        Self::Phaser,
        Self::SlipRoll,
        Self::Roll,
        Self::RevRoll,
        Self::Robot,
        Self::Pitch,
        Self::EnigmaJet,
        Self::MobiusSaw,
        Self::MobiusTriangle,
        Self::LowCutEcho,
        Self::PingPong,
        Self::Helix,
        Self::VinylBrake,
        Self::Stretch,
        Self::BpfEcho,
        Self::Noise,
        Self::SpiralUp,
        Self::ReverbUp,
        Self::HpfEcho,
        Self::LpfEcho,
        Self::CrushEcho,
        Self::SpiralDown,
        Self::ReverbDown,
        Self::LowPass,
        Self::HighPass,
        Self::BandPass,
        Self::Crusher,
        Self::Brake,
        Self::Backspin,
        Self::EchoOut,
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
            Self::Delay => "delay",
            Self::Echo => "echo",
            Self::Spiral => "spiral",
            Self::Reverb => "reverb",
            Self::RevDelay => "rev-delay",
            Self::MtDelay => "mt-delay",
            Self::UpEcho => "up-echo",
            Self::DownEcho => "down-echo",
            Self::Trans => "trans",
            Self::Pan => "pan",
            Self::Filter => "filter",
            Self::Flanger => "flanger",
            Self::Phaser => "phaser",
            Self::SlipRoll => "slip-roll",
            Self::Roll => "roll",
            Self::RevRoll => "rev-roll",
            Self::Robot => "robot",
            Self::Pitch => "pitch",
            Self::EnigmaJet => "enigma-jet",
            Self::MobiusSaw => "mobius-saw",
            Self::MobiusTriangle => "mobius-tri",
            Self::LowCutEcho => "low-cut-echo",
            Self::PingPong => "ping-pong",
            Self::Helix => "helix",
            Self::VinylBrake => "vinyl-brake",
            Self::Stretch => "stretch",
            Self::BpfEcho => "bpf-echo",
            Self::Noise => "noise",
            Self::SpiralUp => "spiral-up",
            Self::ReverbUp => "reverb-up",
            Self::HpfEcho => "hpf-echo",
            Self::LpfEcho => "lpf-echo",
            Self::CrushEcho => "crush-echo",
            Self::SpiralDown => "spiral-down",
            Self::ReverbDown => "reverb-down",
            Self::LowPass => "low-pass",
            Self::HighPass => "high-pass",
            Self::BandPass => "band-pass",
            Self::Crusher => "crusher",
            Self::Brake => "brake",
            Self::Backspin => "backspin",
            Self::EchoOut => "echo-out",
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
        let wanted = FORMER_NAMES
            .iter()
            .find(|(former, _)| *former == lowercase)
            .map_or(lowercase.as_str(), |(_, kind)| kind.name());
        Self::ALL
            .into_iter()
            .find(|kind| kind.name() == wanted)
            .ok_or_else(|| ParseEffectKindError {
                text: text.to_owned(),
            })
    }
}

/// An effect of the echo family at 1/2 beat with `voicing`.
fn echo<V: Voicing>(config: &EffectConfig, voicing: V) -> Box<dyn Effect> {
    Box::new(EchoEffect::new(config, voicing, BeatLength::HALF))
}

/// Builds an effect with its default settings, switched off.
///
/// This allocates the effect's buffers, so call it outside the audio thread.
pub fn create(kind: EffectKind, config: &EffectConfig) -> Box<dyn Effect> {
    match kind {
        EffectKind::Delay => echo(config, Single),
        EffectKind::Echo => Box::new(Echo::new(config)),
        EffectKind::Spiral => echo(config, Diffused::spiral(config)),
        EffectKind::Reverb => Box::new(Reverb::new(config)),
        EffectKind::RevDelay => Box::new(ReverseDelay::new(config)),
        EffectKind::MtDelay => Box::new(MultiTapDelay::new(config)),
        EffectKind::UpEcho => echo(config, Climbing::new(config, true)),
        EffectKind::DownEcho => echo(config, Climbing::new(config, false)),
        EffectKind::Trans => Box::new(Gate::new()),
        EffectKind::Pan => Box::new(Pan::new()),
        EffectKind::Filter => Box::new(BeatFilter::new()),
        EffectKind::Flanger => Box::new(Flanger::new(config)),
        EffectKind::Phaser => Box::new(Phaser::new()),
        EffectKind::SlipRoll => Box::new(RollEffect::new(config, RollStyle::Slip)),
        EffectKind::Roll => Box::new(RollEffect::new(config, RollStyle::Hold)),
        EffectKind::RevRoll => Box::new(RollEffect::new(config, RollStyle::Reverse)),
        EffectKind::Robot => Box::new(Robot::new(config)),
        EffectKind::Pitch => Box::new(Pitch::new(config)),
        EffectKind::EnigmaJet => Box::new(EnigmaJet::new(config)),
        EffectKind::MobiusSaw => Box::new(Mobius::saw()),
        EffectKind::MobiusTriangle => Box::new(Mobius::triangle()),
        EffectKind::LowCutEcho => echo(config, Filtered::new(EchoFilter::LowCut)),
        EffectKind::PingPong => Box::new(PingPong::new(config)),
        EffectKind::Helix => echo(config, Layered::default()),
        EffectKind::VinylBrake => Box::new(VinylBrake::new(config)),
        EffectKind::Stretch => Box::new(Stretch::new(config)),
        EffectKind::BpfEcho => echo(config, Filtered::new(EchoFilter::BandPass)),
        EffectKind::Noise => echo(config, Noise::new()),
        EffectKind::SpiralUp => echo(config, Diffused::climbing(config, true)),
        EffectKind::ReverbUp => Box::new(ShimmerReverb::new(config, true)),
        EffectKind::HpfEcho => echo(config, Filtered::new(EchoFilter::HighPass)),
        EffectKind::LpfEcho => echo(config, Filtered::new(EchoFilter::LowPass)),
        EffectKind::CrushEcho => echo(config, Crushed::new()),
        EffectKind::SpiralDown => echo(config, Diffused::climbing(config, false)),
        EffectKind::ReverbDown => Box::new(ShimmerReverb::new(config, false)),
        EffectKind::LowPass => Box::new(Filter::new(FilterMode::LowPass)),
        EffectKind::HighPass => Box::new(Filter::new(FilterMode::HighPass)),
        EffectKind::BandPass => Box::new(Filter::new(FilterMode::BandPass)),
        EffectKind::Crusher => Box::new(Crusher::new()),
        EffectKind::Brake => Box::new(Brake::new(config)),
        EffectKind::Backspin => Box::new(Backspin::new(config)),
        EffectKind::EchoOut => Box::new(EchoEffect::new(config, Outgoing::default(), BeatLength::ONE)),
    }
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
        assert_eq!("gate".parse::<EffectKind>(), Ok(EffectKind::Trans));
    }
}
