//! MIDI messages as bytes: parsing the ones controllers send and building the ones they light up from.

use std::fmt::{self, Write as _};
use std::str::FromStr;

use serde::Deserialize;

use crate::value::U7;

const NOTE_OFF: u8 = 0x8;
const NOTE_ON: u8 = 0x9;
const CONTROL_CHANGE: u8 = 0xB;
const CHANNEL_COUNT: usize = 16;
const NUMBER_COUNT: usize = 128;

/// Whether a message is a note or a control change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Kind {
    Note,
    ControlChange,
}

/// The note or control change number on a MIDI channel that one control sends or one light
/// listens to. Written in mapping files as the status and first data byte in hex, `"90 0B"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct Address {
    kind: Kind,
    channel: u8,
    number: U7,
}

impl Address {
    /// How many distinct addresses there are, for tables indexed by [`Address::index`].
    pub(crate) const COUNT: usize = 2 * CHANNEL_COUNT * NUMBER_COUNT;

    /// The address of `number` under a note on (9n) or control change (Bn) status byte.
    pub(crate) fn from_status(status: u8, number: U7) -> Result<Address, String> {
        let kind = match status >> 4 {
            NOTE_ON => Kind::Note,
            CONTROL_CHANGE => Kind::ControlChange,
            _ => {
                return Err(format!(
                    "status {status:02X} is neither a note (9n) nor a control change (Bn)"
                ));
            }
        };
        Ok(Address {
            kind,
            channel: status & 0x0F,
            number,
        })
    }

    pub(crate) const fn index(self) -> usize {
        let kind = match self.kind {
            Kind::Note => 0,
            Kind::ControlChange => 1,
        };
        (kind * CHANNEL_COUNT + self.channel as usize) * NUMBER_COUNT + self.number.get() as usize
    }

    pub(crate) const fn same_channel(self, other: Address) -> bool {
        self.channel == other.channel
    }

    /// The three bytes that send `value` to this address.
    pub(crate) const fn message(self, value: U7) -> [u8; 3] {
        let status = match self.kind {
            Kind::Note => NOTE_ON << 4,
            Kind::ControlChange => CONTROL_CHANGE << 4,
        };
        [status | self.channel, self.number.get(), value.get()]
    }
}

impl fmt::Display for Address {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [status, number, _] = self.message(U7::MAX);
        write!(formatter, "{status:02X} {number:02X}")
    }
}

impl FromStr for Address {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let invalid = || format!("\"{text}\" is not a status and number in hex such as \"90 0B\"");
        let mut bytes = text
            .split_whitespace()
            .map(|byte| u8::from_str_radix(byte, 16));
        let (Some(Ok(status)), Some(Ok(number)), None) = (bytes.next(), bytes.next(), bytes.next())
        else {
            return Err(invalid());
        };
        Address::from_status(status, U7::new(number).ok_or_else(invalid)?)
    }
}

impl TryFrom<String> for Address {
    type Error = String;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        text.parse()
    }
}

/// A note or control change message: where it came from and its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ChannelMessage {
    pub(crate) address: Address,
    pub(crate) value: U7,
}

impl ChannelMessage {
    /// Reads a note on, note off or control change. A note off reads as value 0, like the note
    /// on with velocity 0 most controllers send for a release.
    pub(crate) fn parse(bytes: &[u8]) -> Option<ChannelMessage> {
        // Normalise a note off to a note on with velocity 0: a mapping address can only be a
        // 9n or Bn status, so without this a pad released with a real note off would match
        // nothing.
        let bytes = match *bytes {
            [status, number, _] if status >> 4 == NOTE_OFF => [status | 0x10, number, 0],
            [status, number, data] => [status, number, data],
            _ => return None,
        };
        let [status, number, data] = bytes;
        let (kind, value) = match status >> 4 {
            NOTE_ON => (Kind::Note, data),
            CONTROL_CHANGE => (Kind::ControlChange, data),
            _ => return None,
        };
        Some(ChannelMessage {
            address: Address {
                kind,
                channel: status & 0x0F,
                number: U7::new(number)?,
            },
            value: U7::new(value)?,
        })
    }
}

/// The bytes of one incoming MIDI message, kept inline so it can cross threads without
/// allocating. System exclusive messages longer than the inline space keep their start.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RawMessage {
    bytes: [u8; RawMessage::CAPACITY],
    length: usize,
}

impl RawMessage {
    /// How many bytes are kept.
    pub const CAPACITY: usize = 32;

    pub(crate) fn new(message: &[u8]) -> RawMessage {
        let mut bytes = [0; Self::CAPACITY];
        let kept = message.len().min(Self::CAPACITY);
        bytes[..kept].copy_from_slice(&message[..kept]);
        RawMessage {
            bytes,
            length: message.len(),
        }
    }

    /// The bytes kept, at most [`RawMessage::CAPACITY`].
    pub fn bytes(&self) -> &[u8] {
        &self.bytes[..self.length.min(Self::CAPACITY)]
    }

    /// The message's full length, which is more than [`RawMessage::bytes`] holds when truncated.
    pub fn len(&self) -> usize {
        self.length
    }

    /// Whether the message had no bytes.
    pub fn is_empty(&self) -> bool {
        self.length == 0
    }
}

impl fmt::Debug for RawMessage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "RawMessage({self})")
    }
}

impl fmt::Display for RawMessage {
    /// Hex bytes separated by spaces, `B0 21 41`, with `…` when the message was truncated.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut text = String::with_capacity(3 * Self::CAPACITY + 4);
        for (position, byte) in self.bytes().iter().enumerate() {
            if position > 0 {
                text.push(' ');
            }
            write!(text, "{byte:02X}")?;
        }
        if self.length > Self::CAPACITY {
            text.push_str(" …");
        }
        formatter.pad(&text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_parse_from_hex_status_and_number() {
        let address: Address = "B6 1F".parse().unwrap();
        assert_eq!(address.kind, Kind::ControlChange);
        assert_eq!(address.to_string(), "B6 1F");
        assert_eq!("9f 01".parse::<Address>().unwrap().to_string(), "9F 01");
    }

    #[test]
    fn addresses_reject_other_messages_and_bad_text() {
        for text in ["80 0B", "E0 00", "90", "90 0B 7F", "90 80", "zz 0B", ""] {
            assert!(text.parse::<Address>().is_err(), "{text} should not parse");
        }
    }

    #[test]
    fn address_indexes_are_distinct_and_in_range() {
        let mut seen = vec![false; Address::COUNT];
        for status in [0x90_u8, 0x9F, 0xB0, 0xBF] {
            for number in [0_u8, 127] {
                let address: Address = format!("{status:02X} {number:02X}").parse().unwrap();
                assert!(!seen[address.index()]);
                seen[address.index()] = true;
            }
        }
    }

    #[test]
    fn note_off_reads_as_release() {
        let message = ChannelMessage::parse(&[0x80, 0x0B, 0x40]).unwrap();
        assert_eq!(message.address.kind, Kind::Note);
        assert_eq!(message.value.get(), 0);
    }

    #[test]
    fn only_three_byte_channel_messages_parse() {
        assert!(ChannelMessage::parse(&[0x90, 0x0B]).is_none());
        assert!(ChannelMessage::parse(&[0xF0, 0x00, 0xF7]).is_none());
        assert!(ChannelMessage::parse(&[0xE0, 0x00, 0x40]).is_none());
        assert!(ChannelMessage::parse(&[0x90, 0x0B, 0x80]).is_none());
    }

    #[test]
    fn raw_messages_print_as_hex_and_mark_truncation() {
        assert_eq!(RawMessage::new(&[0xB0, 0x21, 0x41]).to_string(), "B0 21 41");
        let long = [0xF0; 40];
        let raw = RawMessage::new(&long);
        assert_eq!(raw.len(), 40);
        assert_eq!(raw.bytes().len(), RawMessage::CAPACITY);
        assert!(raw.to_string().ends_with(" …"));
    }
}
