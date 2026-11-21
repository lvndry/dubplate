//! What a DJ does on a controller, independent of the controller that sent it.

use std::fmt;

use serde::Deserialize;

use crate::value::{Position, Ticks};

/// A deck on the controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Deck {
    /// The left deck.
    A,
    /// The right deck.
    B,
}

impl fmt::Display for Deck {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Deck::A => formatter.write_str("deck A"),
            Deck::B => formatter.write_str("deck B"),
        }
    }
}

/// Whether SHIFT was held. Controllers send different messages for a control with SHIFT held.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Layer {
    /// SHIFT was up.
    #[default]
    Normal,
    /// SHIFT was held.
    Shift,
}

/// What the performance pads do, chosen with the pad mode buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PadMode {
    /// Set and jump to hot cues.
    HotCue,
    /// Pad FX bank 1.
    #[serde(rename = "pad-fx-1")]
    PadFx1,
    /// Jump by a number of beats.
    BeatJump,
    /// Trigger samples.
    Sampler,
    /// Play a hot cue at different keys.
    Keyboard,
    /// Pad FX bank 2.
    #[serde(rename = "pad-fx-2")]
    PadFx2,
    /// Start loops of a set number of beats.
    BeatLoop,
    /// Shift the key of the track.
    KeyShift,
}

impl fmt::Display for PadMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            PadMode::HotCue => "hot cue",
            PadMode::PadFx1 => "pad FX 1",
            PadMode::BeatJump => "beat jump",
            PadMode::Sampler => "sampler",
            PadMode::Keyboard => "keyboard",
            PadMode::PadFx2 => "pad FX 2",
            PadMode::BeatLoop => "beat loop",
            PadMode::KeyShift => "key shift",
        })
    }
}

/// A performance pad, numbered from 1 like the hardware.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(try_from = "u8")]
pub struct PadIndex(u8);

impl PadIndex {
    /// The most pads a deck can have.
    pub const MAX_PADS: u8 = 16;

    /// Pad `number`, counted from 1, or `None` outside 1 to [`PadIndex::MAX_PADS`].
    pub const fn new(number: u8) -> Option<PadIndex> {
        if number >= 1 && number <= Self::MAX_PADS {
            Some(PadIndex(number))
        } else {
            None
        }
    }

    /// The pad's number, counted from 1.
    pub const fn number(self) -> u8 {
        self.0
    }
}

impl TryFrom<u8> for PadIndex {
    type Error = String;

    fn try_from(number: u8) -> Result<Self, Self::Error> {
        PadIndex::new(number)
            .ok_or_else(|| format!("pad {number} is outside 1 to {}", PadIndex::MAX_PADS))
    }
}

/// Left or right, for the paired arrow buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    /// The ◁ button.
    Left,
    /// The ▷ button.
    Right,
}

impl fmt::Display for Direction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Direction::Left => "left",
            Direction::Right => "right",
        })
    }
}

/// What a fader start asks the deck to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FaderStart {
    /// The fader left its closed end: start playing.
    Play,
    /// The fader reached its closed end: go back to the cue point.
    Cue,
}

/// An EQ band.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EqBand {
    /// HI.
    High,
    /// MID.
    Mid,
    /// LOW.
    Low,
}

/// What a jog wheel turn should do to the track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JogMode {
    /// The platter top is touched in vinyl mode: move the track with the hand.
    Scratch,
    /// The wheel's side, or the platter with vinyl mode off: bend the tempo to line up beats.
    Nudge,
    /// SHIFT is held: move quickly through the track.
    Search,
}

impl fmt::Display for JogMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            JogMode::Scratch => "scratch",
            JogMode::Nudge => "nudge",
            JogMode::Search => "search",
        })
    }
}

/// A button, pad or touch sensor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Button {
    /// PLAY/PAUSE.
    Play(Deck),
    /// CUE.
    Cue(Deck),
    /// BEAT SYNC. Some controllers, the DDJ-FLX4 among them, send it when the button is released.
    Sync(Deck),
    /// BEAT SYNC held down.
    SyncHold(Deck),
    /// SHIFT.
    Shift(Deck),
    /// A hand on top of the jog wheel's platter.
    JogTouch(Deck),
    /// Loop IN.
    LoopIn(Deck),
    /// Loop OUT.
    LoopOut(Deck),
    /// 4 BEAT/EXIT: a four-beat loop, or leave the current loop.
    FourBeatExit(Deck),
    /// CUE/LOOP CALL ◁ or ▷.
    CueLoopCall {
        /// The deck.
        deck: Deck,
        /// Which arrow.
        direction: Direction,
    },
    /// The channel's headphone CUE.
    HeadphoneCue(Deck),
    /// The channel fader or crossfader started or cued the deck.
    FaderStart {
        /// The deck.
        deck: Deck,
        /// What the deck should do.
        start: FaderStart,
    },
    /// A pad mode button.
    PadMode {
        /// The deck.
        deck: Deck,
        /// The mode it selects.
        mode: PadMode,
    },
    /// A performance pad.
    Pad {
        /// The deck.
        deck: Deck,
        /// The pad mode the controller is in.
        mode: PadMode,
        /// Which pad.
        pad: PadIndex,
    },
    /// LOAD to a deck.
    Load(Deck),
    /// Pushing the browse knob.
    BrowsePush,
    /// BEAT FX SELECT.
    BeatFxSelect,
    /// BEAT ◁ or ▷.
    BeatFxBeat(Direction),
    /// BEAT FX ON/OFF.
    BeatFxOn,
    /// MASTER CUE.
    MasterCue,
    /// SMART CFX.
    SmartCfx,
    /// SMART FADER.
    SmartFader,
}

impl fmt::Display for Button {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Button::Play(deck) => write!(formatter, "{deck} play"),
            Button::Cue(deck) => write!(formatter, "{deck} cue"),
            Button::Sync(deck) => write!(formatter, "{deck} beat sync"),
            Button::SyncHold(deck) => write!(formatter, "{deck} beat sync hold"),
            Button::Shift(deck) => write!(formatter, "{deck} shift"),
            Button::JogTouch(deck) => write!(formatter, "{deck} jog touch"),
            Button::LoopIn(deck) => write!(formatter, "{deck} loop in"),
            Button::LoopOut(deck) => write!(formatter, "{deck} loop out"),
            Button::FourBeatExit(deck) => write!(formatter, "{deck} 4 beat/exit"),
            Button::CueLoopCall { deck, direction } => {
                write!(formatter, "{deck} cue/loop call {direction}")
            }
            Button::HeadphoneCue(deck) => write!(formatter, "{deck} headphone cue"),
            Button::FaderStart { deck, start } => match start {
                FaderStart::Play => write!(formatter, "{deck} fader start play"),
                FaderStart::Cue => write!(formatter, "{deck} fader start cue"),
            },
            Button::PadMode { deck, mode } => write!(formatter, "{deck} pad mode {mode}"),
            Button::Pad { deck, mode, pad } => {
                write!(formatter, "{deck} pad {} ({mode})", pad.number())
            }
            Button::Load(deck) => write!(formatter, "load to {deck}"),
            Button::BrowsePush => formatter.write_str("browse push"),
            Button::BeatFxSelect => formatter.write_str("beat FX select"),
            Button::BeatFxBeat(direction) => write!(formatter, "beat FX beat {direction}"),
            Button::BeatFxOn => formatter.write_str("beat FX on/off"),
            Button::MasterCue => formatter.write_str("master cue"),
            Button::SmartCfx => formatter.write_str("smart CFX"),
            Button::SmartFader => formatter.write_str("smart fader"),
        }
    }
}

/// A switch or lever that stays where it is put.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Switch {
    /// The beat FX channel lever sends the effect to this deck.
    BeatFxChannel(Deck),
    /// The output is summed to mono.
    MonoOutput,
}

impl fmt::Display for Switch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Switch::BeatFxChannel(deck) => write!(formatter, "beat FX channel {deck}"),
            Switch::MonoOutput => formatter.write_str("mono output"),
        }
    }
}

/// A fader or knob with an absolute position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Continuous {
    /// TEMPO fader. The bottom of the range is the "-" end.
    Tempo(Deck),
    /// Channel fader.
    ChannelFader(Deck),
    /// Crossfader. The bottom of the range is the left side.
    Crossfader,
    /// TRIM.
    Trim(Deck),
    /// An EQ knob.
    Eq {
        /// The deck.
        deck: Deck,
        /// The band.
        band: EqBand,
    },
    /// The channel's filter knob (CFX on Pioneer controllers).
    Filter(Deck),
    /// BEAT FX LEVEL/DEPTH.
    BeatFxLevelDepth,
    /// MASTER LEVEL.
    MasterLevel,
    /// MIC LEVEL.
    MicLevel,
    /// HEADPHONES MIX between the cued channels and the master.
    HeadphoneMix,
    /// HEADPHONES LEVEL.
    HeadphoneLevel,
}

impl Continuous {
    /// Whether the control rests in the middle, so its position reads best as -100% to +100%.
    fn is_centered(self) -> bool {
        match self {
            Continuous::Tempo(_)
            | Continuous::Trim(_)
            | Continuous::Eq { .. }
            | Continuous::Filter(_)
            | Continuous::HeadphoneMix => true,
            Continuous::ChannelFader(_)
            | Continuous::Crossfader
            | Continuous::BeatFxLevelDepth
            | Continuous::MasterLevel
            | Continuous::MicLevel
            | Continuous::HeadphoneLevel => false,
        }
    }
}

impl fmt::Display for Continuous {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Continuous::Tempo(deck) => write!(formatter, "{deck} tempo"),
            Continuous::ChannelFader(deck) => write!(formatter, "{deck} channel fader"),
            Continuous::Crossfader => formatter.write_str("crossfader"),
            Continuous::Trim(deck) => write!(formatter, "{deck} trim"),
            Continuous::Eq { deck, band } => match band {
                EqBand::High => write!(formatter, "{deck} EQ high"),
                EqBand::Mid => write!(formatter, "{deck} EQ mid"),
                EqBand::Low => write!(formatter, "{deck} EQ low"),
            },
            Continuous::Filter(deck) => write!(formatter, "{deck} filter"),
            Continuous::BeatFxLevelDepth => formatter.write_str("beat FX level/depth"),
            Continuous::MasterLevel => formatter.write_str("master level"),
            Continuous::MicLevel => formatter.write_str("mic level"),
            Continuous::HeadphoneMix => formatter.write_str("headphone mix"),
            Continuous::HeadphoneLevel => formatter.write_str("headphone level"),
        }
    }
}

/// A control that reports movement rather than a position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Encoder {
    /// A jog wheel.
    Jog {
        /// The deck.
        deck: Deck,
        /// What the turn should do.
        mode: JogMode,
    },
    /// The browse knob.
    Browse,
}

/// Whether a button went down or came up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ButtonState {
    /// It went down.
    Pressed,
    /// It came up.
    Released,
}

/// One thing the DJ did on the controller, with its value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    /// A button went down or up.
    Button {
        /// Which button.
        button: Button,
        /// Down or up.
        state: ButtonState,
    },
    /// A switch moved.
    Switch {
        /// Which switch.
        switch: Switch,
        /// Whether it is now on.
        on: bool,
    },
    /// A fader or knob moved.
    Continuous {
        /// Which control.
        control: Continuous,
        /// Where it is now.
        position: Position,
    },
    /// A jog wheel or encoder turned.
    Encoder {
        /// Which encoder.
        encoder: Encoder,
        /// How far, and which way.
        ticks: Ticks,
    },
}

impl fmt::Display for Action {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Action::Button { button, state } => match state {
                ButtonState::Pressed => write!(formatter, "{button} pressed"),
                ButtonState::Released => write!(formatter, "{button} released"),
            },
            Action::Switch { switch, on } => {
                write!(formatter, "{switch} {}", if *on { "on" } else { "off" })
            }
            Action::Continuous { control, position } => {
                if control.is_centered() {
                    write!(formatter, "{control} {:+.1}%", position.bipolar() * 100.0)?;
                } else {
                    write!(formatter, "{control} {:.1}%", position.fraction() * 100.0)?;
                }
                write!(formatter, " ({})", position.raw().get())
            }
            Action::Encoder { encoder, ticks } => match encoder {
                Encoder::Jog { deck, mode } => {
                    write!(formatter, "{deck} jog turn {ticks} ({mode})")
                }
                Encoder::Browse => write!(formatter, "browse turn {ticks}"),
            },
        }
    }
}

/// An action and the SHIFT layer it came from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ControlEvent {
    /// What the DJ did.
    pub action: Action,
    /// Whether SHIFT was held.
    pub layer: Layer,
}

impl fmt::Display for ControlEvent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.layer {
            Layer::Normal => write!(formatter, "{}", self.action),
            Layer::Shift => write!(formatter, "shift + {}", self.action),
        }
    }
}
