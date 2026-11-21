//! Decoding incoming MIDI messages on the MIDI thread.

use std::sync::Arc;

use crate::action::{Action, ButtonState, ControlEvent};
use crate::mapping::{Input, Mapping};
use crate::message::ChannelMessage;
use crate::value::{Position, U7, U14};

/// Turns incoming MIDI messages into [`ControlEvent`]s for one mapping. It remembers the first
/// half of each 14-bit control, so give each input port its own decoder. Decoding never
/// allocates, so it can run on the MIDI thread.
///
/// The decoder relies on the controller sending the most significant half of a 14-bit control
/// first, and both halves on every move; the DDJ-FLX4 does, per Pioneer's "DDJ-FLX4 List of
/// MIDI messages". When the least significant half arrives first, it decodes as if the most
/// significant half were 0.
#[derive(Debug)]
pub struct Decoder {
    mapping: Arc<Mapping>,
    most_significant: Box<[U7]>,
    running_status: Option<u8>,
}

impl Decoder {
    /// A decoder for `mapping`.
    pub fn new(mapping: Arc<Mapping>) -> Decoder {
        let most_significant =
            vec![U7::default(); mapping.fourteen_bit_controls()].into_boxed_slice();
        Decoder {
            mapping,
            most_significant,
            running_status: None,
        }
    }

    /// What the message means, or `None` when it is not in the mapping or is the first half of
    /// a 14-bit control.
    pub fn decode(&mut self, bytes: &[u8]) -> Option<ControlEvent> {
        // A message whose first byte is a data byte is missing its status byte: the controller
        // left it out under MIDI running status, so the last channel status the decoder saw
        // stands in for it. As MIDI 1.0 specifies, system exclusive and system common messages
        // cancel running status, and real-time messages leave it alone.
        let message = match bytes {
            [status, ..] if (0x80..0xF0).contains(status) => {
                self.running_status = Some(*status);
                ChannelMessage::parse(bytes)?
            }
            [status, ..] if (0xF0..0xF8).contains(status) => {
                self.running_status = None;
                return None;
            }
            [first, second] if *first < 0x80 => {
                let status = self.running_status?;
                ChannelMessage::parse(&[status, *first, *second])?
            }
            _ => ChannelMessage::parse(bytes)?,
        };

        let slot = self.mapping.slot(message.address)?;
        let value = message.value;
        let action = match slot.input {
            Input::Button(button) => Action::Button {
                button,
                state: if value.get() > 0 {
                    ButtonState::Pressed
                } else {
                    ButtonState::Released
                },
            },
            Input::Switch(switch) => Action::Switch {
                switch,
                on: value.get() > 0,
            },
            Input::Continuous(control) => Action::Continuous {
                control,
                position: Position::new(U14::from(value)),
            },
            Input::MostSignificant { pair } => {
                *self.most_significant.get_mut(pair)? = value;
                return None;
            }
            Input::LeastSignificant { control, pair } => Action::Continuous {
                control,
                position: Position::new(U14::from_halves(*self.most_significant.get(pair)?, value)),
            },
            Input::Encoder(encoder, encoding) => Action::Encoder {
                encoder,
                ticks: encoding.decode(value),
            },
        };
        Some(ControlEvent {
            action,
            layer: slot.layer,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::action::{Button, ButtonState, Continuous, Deck, Layer};

    fn u7(value: u8) -> U7 {
        U7::new(value).unwrap()
    }

    fn decoder(body: &str) -> Decoder {
        let mapping = Mapping::from_toml(&format!("name = \"Test\"\nports = [\"Test\"]\n{body}"))
            .expect("the test mapping parses");
        Decoder::new(Arc::new(mapping))
    }

    #[test]
    fn a_two_byte_message_borrows_the_last_channel_status() {
        let mut decoder =
            decoder(r#"continuous = [{ control = "crossfader", message = "B6 1F" }]"#);
        for (bytes, value) in [
            (&[0xB6, 0x1F, 0x34][..], 0x34),
            (&[0x1F, 0x40][..], 0x40),
            (&[0x1F, 0x50][..], 0x50),
        ] {
            let action = Action::Continuous {
                control: Continuous::Crossfader,
                position: Position::new(U14::from(u7(value))),
            };
            assert_eq!(
                decoder.decode(bytes),
                Some(ControlEvent {
                    action,
                    layer: Layer::Normal
                }),
                "decoding {bytes:02X?}"
            );
        }
    }

    #[test]
    fn a_two_byte_message_with_no_stored_status_decodes_to_nothing() {
        let mut decoder =
            decoder(r#"continuous = [{ control = "crossfader", message = "B6 1F" }]"#);
        assert_eq!(decoder.decode(&[0x1F, 0x40]), None);
    }

    #[test]
    fn system_exclusive_cancels_running_status_and_real_time_keeps_it() {
        let mut decoder = decoder(r#"buttons = [{ button = { cue = "a" }, message = "90 0C" }]"#);
        let cue = |state| {
            Some(ControlEvent {
                action: Action::Button {
                    button: Button::Cue(Deck::A),
                    state,
                },
                layer: Layer::Normal,
            })
        };
        assert_eq!(
            decoder.decode(&[0x90, 0x0C, 0x7F]),
            cue(ButtonState::Pressed)
        );
        assert_eq!(decoder.decode(&[0xF8]), None);
        assert_eq!(decoder.decode(&[0x0C, 0x00]), cue(ButtonState::Released));
        assert_eq!(decoder.decode(&[0xF0, 0x00, 0x40, 0x05, 0xF7]), None);
        assert_eq!(decoder.decode(&[0x0C, 0x7F]), None);
    }

    #[test]
    fn a_button_press_and_release_share_running_status_with_their_notes() {
        let mut decoder = decoder(
            r#"buttons = [{ button = { pad = { deck = "a", mode = "hot-cue", pad = 1 } }, message = "97 00" }]"#,
        );
        assert_eq!(
            decoder.decode(&[0x97, 0x00, 0x7F]),
            Some(ControlEvent {
                action: Action::Button {
                    button: Button::Pad {
                        deck: Deck::A,
                        mode: crate::action::PadMode::HotCue,
                        pad: crate::action::PadIndex::new(1).unwrap()
                    },
                    state: ButtonState::Pressed,
                },
                layer: Layer::Normal,
            })
        );
        assert_eq!(
            decoder.decode(&[0x00, 0x00]),
            Some(ControlEvent {
                action: Action::Button {
                    button: Button::Pad {
                        deck: Deck::A,
                        mode: crate::action::PadMode::HotCue,
                        pad: crate::action::PadIndex::new(1).unwrap()
                    },
                    state: ButtonState::Released,
                },
                layer: Layer::Normal,
            })
        );
    }

    #[test]
    fn a_least_significant_half_before_any_most_significant_half_uses_zero() {
        let mut decoder = decoder(
            r#"continuous = [{ control = "crossfader", message = "B6 1F", lsb = "B6 3F" }]"#,
        );
        let event = decoder.decode(&[0xB6, 0x3F, 0x40]).unwrap();
        assert_eq!(
            event.action,
            Action::Continuous {
                control: Continuous::Crossfader,
                position: Position::new(U14::from_halves(u7(0), u7(0x40))),
            }
        );
    }

    #[test]
    fn a_new_most_significant_half_replaces_the_stored_one() {
        let mut decoder = decoder(
            r#"continuous = [{ control = "crossfader", message = "B6 1F", lsb = "B6 3F" }]"#,
        );
        assert_eq!(decoder.decode(&[0xB6, 0x1F, 0x10]), None);
        assert_eq!(
            decoder.decode(&[0xB6, 0x3F, 0x00]).unwrap().action,
            Action::Continuous {
                control: Continuous::Crossfader,
                position: Position::new(U14::from_halves(u7(0x10), u7(0))),
            }
        );
        assert_eq!(decoder.decode(&[0xB6, 0x1F, 0x20]), None);
        assert_eq!(
            decoder.decode(&[0xB6, 0x3F, 0x40]).unwrap().action,
            Action::Continuous {
                control: Continuous::Crossfader,
                position: Position::new(U14::from_halves(u7(0x20), u7(0x40))),
            }
        );
    }
}
