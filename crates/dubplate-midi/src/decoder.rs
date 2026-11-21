//! Decoding incoming MIDI messages on the MIDI thread.

use std::sync::Arc;

use crate::action::{Action, ButtonState, ControlEvent};
use crate::mapping::{Input, Mapping};
use crate::message::ChannelMessage;
use crate::value::{Position, U7, U14};

/// Turns incoming MIDI messages into [`ControlEvent`]s for one mapping. It remembers the first
/// half of each 14-bit control, so give each input port its own decoder. Decoding never
/// allocates, so it can run on the MIDI thread.
#[derive(Debug)]
pub struct Decoder {
    mapping: Arc<Mapping>,
    most_significant: Box<[U7]>,
}

impl Decoder {
    /// A decoder for `mapping`.
    pub fn new(mapping: Arc<Mapping>) -> Decoder {
        let most_significant =
            vec![U7::default(); mapping.fourteen_bit_controls()].into_boxed_slice();
        Decoder {
            mapping,
            most_significant,
        }
    }

    /// What the message means, or `None` when it is not in the mapping or is the first half of
    /// a 14-bit control.
    pub fn decode(&mut self, bytes: &[u8]) -> Option<ControlEvent> {
        let message = ChannelMessage::parse(bytes)?;
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
