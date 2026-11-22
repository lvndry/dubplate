//! Controller mappings: which MIDI message is which control and which light, loaded from TOML.

use std::collections::HashMap;
use std::fmt;
use std::time::Duration;

use serde::Deserialize;

use crate::action::{Button, Continuous, Deck, Encoder, Layer, Switch};
use crate::message::{Address, ChannelMessage};
use crate::value::{RelativeEncoding, U7};

mod build;

const BUILT_IN: [&str; 1] = [include_str!("../../mappings/pioneer-ddj-flx4.toml")];

/// A mapping file that cannot be used.
#[derive(Debug, thiserror::Error)]
pub enum MappingError {
    /// The file is not TOML, or does not have the shape of a mapping.
    #[error("the mapping file is not valid: {0}")]
    Parse(#[from] toml::de::Error),
    /// Two controls claim the same MIDI message.
    #[error("{mapping}: message {address} is mapped to more than one control")]
    DuplicateMessage {
        /// The mapping's name.
        mapping: String,
        /// The message's status and number in hex.
        address: String,
    },
    /// Two lights or outputs claim the same target.
    #[error("{mapping}: {output} has more than one output message")]
    DuplicateOutput {
        /// The mapping's name.
        mapping: String,
        /// The light or output.
        output: String,
    },
    /// A value in the mapping is out of range or inconsistent.
    #[error("{mapping}: {reason}")]
    Invalid {
        /// The mapping's name.
        mapping: String,
        /// What is wrong.
        reason: String,
    },
}

/// Something the app shows on the controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feedback {
    /// Turns the light of a button or pad on or off. The DDJ-FLX4's BEAT FX ON/OFF light blinks
    /// while on.
    Light {
        /// The button or pad.
        button: Button,
        /// The light shown while SHIFT is up, or the one shown while it is held.
        layer: Layer,
        /// Whether it is lit.
        on: bool,
    },
    /// A channel level meter.
    LevelMeter {
        /// The channel.
        deck: Deck,
        /// The level, 0 for silence.
        level: U7,
    },
    /// Plays the controller's "track loaded" animation on a deck.
    TrackLoaded(Deck),
    /// Turns the jog wheel's vinyl mode on or off, which decides whether the platter scratches.
    VinylMode {
        /// The deck.
        deck: Deck,
        /// Whether vinyl mode is on.
        on: bool,
    },
}

/// Outputs that are not the light of a button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum OutputTarget {
    LevelMeter(Deck),
    TrackLoaded(Deck),
    VinylMode(Deck),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Output {
    Light(Button, Layer),
    Target(OutputTarget),
}

impl fmt::Display for Output {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Output::Light(button, Layer::Normal) => write!(formatter, "the light of {button}"),
            Output::Light(button, Layer::Shift) => {
                write!(formatter, "the light of shift + {button}")
            }
            Output::Target(OutputTarget::LevelMeter(deck)) => {
                write!(formatter, "the {deck} level meter")
            }
            Output::Target(OutputTarget::TrackLoaded(deck)) => {
                write!(formatter, "the {deck} track loaded light")
            }
            Output::Target(OutputTarget::VinylMode(deck)) => write!(formatter, "{deck} vinyl mode"),
        }
    }
}

/// What one incoming message means.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Input {
    Button(Button),
    Switch(Switch),
    Continuous(Continuous),
    /// The first half of a 14-bit control; the value waits for the second half.
    MostSignificant {
        pair: usize,
    },
    /// The second half of a 14-bit control, which completes the value.
    LeastSignificant {
        control: Continuous,
        pair: usize,
    },
    Encoder(Encoder, RelativeEncoding),
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Slot {
    pub(crate) input: Input,
    pub(crate) layer: Layer,
}

/// A message the controller needs again and again while it is open.
#[derive(Debug, Clone)]
pub(crate) struct Keepalive {
    pub(crate) interval: Duration,
    pub(crate) message: Vec<u8>,
}

/// How one controller model speaks MIDI: what each incoming message means and which message
/// lights each LED. Built-in mappings and user mappings share one TOML shape.
pub struct Mapping {
    name: String,
    ports: Vec<String>,
    jog_ticks_per_revolution: Option<u16>,
    startup: Vec<Vec<u8>>,
    keepalive: Option<Keepalive>,
    inputs: Box<[Option<Slot>]>,
    fourteen_bit_controls: usize,
    outputs: HashMap<Output, Address>,
}

impl fmt::Debug for Mapping {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Mapping")
            .field("name", &self.name)
            .field("ports", &self.ports)
            .finish_non_exhaustive()
    }
}

impl Mapping {
    /// Reads a mapping from TOML.
    pub fn from_toml(text: &str) -> Result<Mapping, MappingError> {
        build::parse(text)
    }

    /// The built-in mapping for a MIDI port, if Dubplate knows the controller behind it.
    pub fn built_in_for_port(port_name: &str) -> Result<Option<Mapping>, MappingError> {
        for text in BUILT_IN {
            let mapping = Mapping::from_toml(text)?;
            if mapping.matches_port(port_name) {
                return Ok(Some(mapping));
            }
        }
        Ok(None)
    }

    /// The controller's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether a MIDI port with this name is this controller.
    pub fn matches_port(&self, port_name: &str) -> bool {
        let port_name = port_name.to_lowercase();
        self.ports
            .iter()
            .any(|pattern| port_name.contains(&pattern.to_lowercase()))
    }

    /// Jog wheel ticks in one full turn, so a scratch can move the track as far as the hand.
    pub fn jog_ticks_per_revolution(&self) -> Option<u16> {
        self.jog_ticks_per_revolution
    }

    /// Whether the message is part of this mapping, including the first half of a 14-bit
    /// control, which decodes to no action on its own.
    pub fn recognizes(&self, bytes: &[u8]) -> bool {
        ChannelMessage::parse(bytes).is_some_and(|message| self.slot(message.address).is_some())
    }

    /// The three bytes that show `feedback` on the controller, or `None` if it has no such light.
    pub fn encode(&self, feedback: Feedback) -> Option<[u8; 3]> {
        let (output, value) = match feedback {
            Feedback::Light { button, layer, on } => (Output::Light(button, layer), on_off(on)),
            Feedback::LevelMeter { deck, level } => {
                (Output::Target(OutputTarget::LevelMeter(deck)), level)
            }
            Feedback::TrackLoaded(deck) => {
                (Output::Target(OutputTarget::TrackLoaded(deck)), U7::MAX)
            }
            Feedback::VinylMode { deck, on } => {
                (Output::Target(OutputTarget::VinylMode(deck)), on_off(on))
            }
        };
        Some(self.outputs.get(&output)?.message(value))
    }

    /// The messages sent to the controller when it is opened.
    pub fn startup(&self) -> &[Vec<u8>] {
        &self.startup
    }

    pub(crate) fn keepalive(&self) -> Option<&Keepalive> {
        self.keepalive.as_ref()
    }

    pub(crate) fn fourteen_bit_controls(&self) -> usize {
        self.fourteen_bit_controls
    }

    pub(crate) fn slot(&self, address: Address) -> Option<Slot> {
        self.inputs.get(address.index()).copied().flatten()
    }
}

const fn on_off(on: bool) -> U7 {
    if on { U7::MAX } else { U7::MIN }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::action::{Action, ButtonState, Direction, JogMode, PadIndex, PadMode};
    use crate::decoder::Decoder;
    use crate::value::{Position, Ticks, U14};

    fn mapping(body: &str) -> Result<Mapping, MappingError> {
        Mapping::from_toml(&format!("name = \"Test\"\nports = [\"Test\"]\n{body}"))
    }

    fn decoder(body: &str) -> Decoder {
        Decoder::new(Arc::new(mapping(body).unwrap()))
    }

    #[test]
    fn seven_bit_controls_spread_over_the_full_range() {
        let mut decoder =
            decoder(r#"continuous = [{ control = "crossfader", message = "B0 01" }]"#);
        let event = decoder.decode(&[0xB0, 0x01, 0x7F]).unwrap();
        assert_eq!(
            event.action,
            Action::Continuous {
                control: Continuous::Crossfader,
                position: Position::new(U14::MAX),
            }
        );
    }

    #[test]
    fn fourteen_bit_controls_wait_for_the_second_half() {
        let mut decoder = decoder(
            r#"continuous = [{ control = "crossfader", message = "B0 01", lsb = "B0 21" }]"#,
        );
        assert_eq!(decoder.decode(&[0xB0, 0x01, 0x12]), None);
        let event = decoder.decode(&[0xB0, 0x21, 0x34]).unwrap();
        let expected = U14::from_halves(U7::new(0x12).unwrap(), U7::new(0x34).unwrap());
        assert_eq!(
            event.action,
            Action::Continuous {
                control: Continuous::Crossfader,
                position: Position::new(expected),
            }
        );
    }

    #[test]
    fn buttons_can_be_control_changes_and_switches_read_on_off() {
        let mut decoder = decoder(
            r#"
            buttons = [{ button = { beat-fx-beat = "left" }, message = "B2 10" }]
            switches = [{ switch = "mono-output", message = "96 6D" }]
            "#,
        );
        assert_eq!(
            decoder.decode(&[0xB2, 0x10, 0x01]).unwrap().action,
            Action::Button {
                button: Button::BeatFxBeat(Direction::Left),
                state: ButtonState::Pressed,
            }
        );
        assert_eq!(
            decoder.decode(&[0x96, 0x6D, 0x00]).unwrap().action,
            Action::Switch {
                switch: Switch::MonoOutput,
                on: false,
            }
        );
    }

    #[test]
    fn encoders_use_their_encoding_and_layer() {
        let mut decoder = decoder(
            r#"encoders = [{ encoder = { jog = { deck = "b", mode = "search" } }, message = "B1 29", encoding = "offset-64", layer = "shift" }]"#,
        );
        let event = decoder.decode(&[0xB1, 0x29, 0x3E]).unwrap();
        assert_eq!(event.layer, Layer::Shift);
        assert_eq!(
            event.action,
            Action::Encoder {
                encoder: Encoder::Jog {
                    deck: Deck::B,
                    mode: JogMode::Search,
                },
                ticks: Ticks::new(-2),
            }
        );
    }

    #[test]
    fn unmapped_messages_decode_to_nothing() {
        let mapping = Arc::new(
            mapping(r#"buttons = [{ button = { play = "a" }, message = "90 0B" }]"#).unwrap(),
        );
        let mut decoder = Decoder::new(Arc::clone(&mapping));
        assert_eq!(decoder.decode(&[0x91, 0x0B, 0x7F]), None);
        assert!(!mapping.recognizes(&[0x91, 0x0B, 0x7F]));
        assert!(mapping.recognizes(&[0x90, 0x0B, 0x7F]));
    }

    #[test]
    fn a_message_can_only_be_mapped_once() {
        let error = mapping(
            r#"buttons = [
                { button = { play = "a" }, message = "90 0B" },
                { button = { cue = "a" }, message = "90 0B" },
            ]"#,
        )
        .unwrap_err();
        assert!(
            matches!(error, MappingError::DuplicateMessage { .. }),
            "{error}"
        );
    }

    #[test]
    fn a_light_can_only_have_one_message() {
        let error = mapping(
            r#"buttons = [
                { button = "beat-fx-on", message = "94 47", light = true },
                { button = "beat-fx-on", message = "95 47", light = true },
            ]"#,
        )
        .unwrap_err();
        assert!(
            matches!(error, MappingError::DuplicateOutput { .. }),
            "{error}"
        );
    }

    #[test]
    fn invalid_mappings_are_rejected() {
        let cases = [
            r#"buttons = [{ button = { play = "a" }, message = "E0 0B" }]"#,
            r#"buttons = [{ button = { play = "c" }, message = "90 0B" }]"#,
            r#"buttons = [{ button = { play = "a" }, message = "90 0B", colour = 1 }]"#,
            r#"continuous = [{ control = "crossfader", message = "B0 01", lsb = "B1 21" }]"#,
            r#"pad_banks = [{ deck = "a", status = 0x97, pads = 17 }]"#,
            r#"pad_modes = [{ mode = "hot-cue", first_pad = 0 }]
               pad_banks = [{ deck = "a", status = 0xE7, pads = 8 }]"#,
            r#"pad_banks = [{ deck = "a", status = 0x97, pads = 8 }]"#,
            r#"pad_modes = [{ mode = "hot-cue", first_pad = 0x7C }]
               pad_banks = [{ deck = "a", status = 0x97, pads = 8 }]"#,
            "keepalive = { interval_ms = 0, message = [0xF0, 0xF7] }",
        ];
        for body in cases {
            assert!(mapping(body).is_err(), "{body} should be rejected");
        }
        assert!(Mapping::from_toml("name = \"Test\"\nports = []").is_err());
    }

    #[test]
    fn pad_banks_expand_to_one_button_per_mode_and_pad() {
        let mapping = Arc::new(
            mapping(
                r#"
                pad_modes = [{ mode = "hot-cue", first_pad = 0x00 }, { mode = "sampler", first_pad = 0x30 }]
                pad_banks = [{ deck = "a", status = 0x97, pads = 8, light = true }]
                "#,
            )
            .unwrap(),
        );
        let mut decoder = Decoder::new(Arc::clone(&mapping));
        let event = decoder.decode(&[0x97, 0x37, 0x7F]).unwrap();
        assert_eq!(
            event.action,
            Action::Button {
                button: Button::Pad {
                    deck: Deck::A,
                    mode: PadMode::Sampler,
                    pad: PadIndex::new(8).unwrap(),
                },
                state: ButtonState::Pressed,
            }
        );
        assert_eq!(decoder.decode(&[0x97, 0x08, 0x7F]), None);
        let light = Feedback::Light {
            button: Button::Pad {
                deck: Deck::A,
                mode: PadMode::HotCue,
                pad: PadIndex::new(3).unwrap(),
            },
            layer: Layer::Normal,
            on: true,
        };
        assert_eq!(mapping.encode(light), Some([0x97, 0x02, 0x7F]));
    }

    #[test]
    fn the_flx4_asks_for_its_positions_and_is_kept_alive() {
        let mapping = Mapping::built_in_for_port("DDJ-FLX4").unwrap().unwrap();
        let query = [
            0xF0, 0x00, 0x40, 0x05, 0x00, 0x00, 0x04, 0x05, 0x00, 0x03, 0x01, 0xF7,
        ];
        let awake = [
            0xF0, 0x00, 0x40, 0x05, 0x00, 0x00, 0x04, 0x05, 0x00, 0x50, 0x02, 0xF7,
        ];
        assert_eq!(mapping.startup(), [query.to_vec()]);
        let keepalive = mapping.keepalive().unwrap();
        assert_eq!(keepalive.interval, Duration::from_millis(200));
        assert_eq!(keepalive.message, awake);
    }

    #[test]
    fn buttons_without_a_light_encode_to_nothing() {
        let mapping =
            mapping(r#"buttons = [{ button = { play = "a" }, message = "90 0B" }]"#).unwrap();
        let light = Feedback::Light {
            button: Button::Play(Deck::A),
            layer: Layer::Normal,
            on: true,
        };
        assert_eq!(mapping.encode(light), None);
    }

    #[test]
    fn a_bad_address_names_the_entry_that_wrote_it() {
        for (body, expected) in [
            (
                r#"buttons = [
                { button = { play = "a" }, message = "90 0B" },
                { button = { cue = "a" }, message = "ZZ 0C" },
            ]"#,
                "button 1: \"ZZ 0C\"",
            ),
            (
                r#"continuous = [{ control = "crossfader", message = "G6 1F", lsb = "B6 3F" }]"#,
                "continuous 0: \"G6 1F\", lsb \"B6 3F\"",
            ),
            (
                r#"pad_modes = [{ mode = "hot-cue", first_pad = 0x00 }]
               pad_banks = [{ deck = "b", status = 0xE7, pads = 8 }]"#,
                "pad bank 0 (deck B): status = 0xE7, pads = 8",
            ),
        ] {
            let error = mapping(body).unwrap_err();
            let text = error.to_string();
            assert!(
                text.contains(expected),
                "the error {text} should name {expected}"
            );
        }
    }

    #[test]
    fn a_continuous_control_cannot_share_a_message_with_its_own_lsb() {
        let error = mapping(
            r#"continuous = [{ control = "crossfader", message = "B6 1F", lsb = "B6 1F" }]"#,
        )
        .unwrap_err();
        let text = error.to_string();
        assert!(
            matches!(error, MappingError::Invalid { .. })
                && text.contains("continuous 0")
                && text.contains("also its own lsb"),
            "the error {text} should say continuous 0 is also its own lsb"
        );
    }
}
