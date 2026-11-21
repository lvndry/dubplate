//! MIDI DJ controllers for Dubplate.
//!
//! - [`Action`] says what the DJ did, in terms that do not depend on the controller: a button
//!   with its [`ButtonState`], a fader at a 14-bit [`Position`], a jog wheel turned by signed
//!   [`Ticks`]. A [`ControlEvent`] adds whether SHIFT was held.
//! - A [`Mapping`] says how one controller model speaks MIDI. It is data, loaded from TOML, so
//!   adding a controller or saving a user's MIDI learn means writing a file of the same shape.
//!   The Pioneer DDJ-FLX4 is built in.
//! - A [`Decoder`] turns incoming bytes into [`ControlEvent`]s without allocating, and
//!   [`Mapping::encode`] turns [`Feedback`] (lights, level meters) into the bytes that show it.
//! - [`ControllerInput`] and [`ControllerOutput`] open the controller's ports. The input decodes
//!   on the MIDI thread and hands each [`MidiEvent`] over through a lock-free queue.

mod action;
mod decoder;
mod device;
mod mapping;
mod message;
mod value;

pub use action::{
    Action, Button, ButtonState, Continuous, ControlEvent, Deck, Direction, Encoder, EqBand,
    FaderStart, JogMode, Layer, PadIndex, PadMode, Switch,
};
pub use decoder::Decoder;
pub use device::{
    ControllerInput, ControllerOutput, InputEvents, MidiError, MidiEvent, PortDirection,
    find_input_port, input_ports, output_ports,
};
pub use mapping::{Feedback, Mapping, MappingError};
pub use message::RawMessage;
pub use value::{Position, Ticks, U7, U14};
