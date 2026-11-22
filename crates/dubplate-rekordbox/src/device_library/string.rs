use super::error::DeviceLibraryError;

const LONG_ASCII_KIND: u8 = 0x40;
const LONG_UTF16_KIND: u8 = 0x90;
const LONG_HEADER_LENGTH: usize = 4;
/// The longest text the one-byte form holds: its first byte is `2 × length + 3`.
const SHORT_ASCII_MAXIMUM: usize = 126;
/// The byte an ISRC starts with inside a UTF-16 frame, where its text is ASCII.
const ISRC_MARKER: u8 = 0x03;

/// A DeviceSQL string as rekordbox frames it, in the encoding rekordbox picks for the text:
/// short ASCII up to 126 characters, long ASCII beyond, and UTF-16 for any text with a
/// character outside ASCII.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceString<'text> {
    /// One length byte, odd, then the ASCII text.
    ShortAscii(&'text str),
    /// `0x40`, a two-byte total length, a zero byte, then the ASCII text.
    LongAscii(&'text str),
    /// `0x90`, a two-byte total length, a zero byte, then the text in UTF-16LE.
    Utf16(&'text str),
}

impl<'text> DeviceString<'text> {
    /// `text` in the encoding rekordbox uses for it.
    pub fn for_text(text: &'text str) -> Self {
        if !text.is_ascii() {
            Self::Utf16(text)
        } else if text.len() <= SHORT_ASCII_MAXIMUM {
            Self::ShortAscii(text)
        } else {
            Self::LongAscii(text)
        }
    }

    /// Whether the string uses a four-byte header, which rekordbox aligns to four bytes inside
    /// a track row.
    pub fn is_long(self) -> bool {
        !matches!(self, Self::ShortAscii(_))
    }

    /// The length of the framed string in bytes.
    pub fn encoded_len(self) -> usize {
        match self {
            Self::ShortAscii(text) => 1 + text.len(),
            Self::LongAscii(text) => LONG_HEADER_LENGTH + text.len(),
            Self::Utf16(text) => LONG_HEADER_LENGTH + 2 * text.encode_utf16().count(),
        }
    }

    /// Appends the framed string to `out`.
    ///
    /// Fails when a long string is longer than its two-byte length can say.
    pub fn write_to(self, out: &mut Vec<u8>) -> Result<(), DeviceLibraryError> {
        match self {
            Self::ShortAscii(text) => {
                let length = u8::try_from(2 * text.len() + 3)
                    .map_err(|_| DeviceLibraryError::StringTooLong(text.len()))?;
                out.push(length);
                out.extend_from_slice(text.as_bytes());
            }
            Self::LongAscii(text) => {
                write_long_header(out, LONG_ASCII_KIND, self.encoded_len())?;
                out.extend_from_slice(text.as_bytes());
            }
            Self::Utf16(text) => {
                write_long_header(out, LONG_UTF16_KIND, self.encoded_len())?;
                out.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
            }
        }
        Ok(())
    }
}

fn write_long_header(
    out: &mut Vec<u8>,
    kind: u8,
    total_length: usize,
) -> Result<(), DeviceLibraryError> {
    let length =
        u16::try_from(total_length).map_err(|_| DeviceLibraryError::StringTooLong(total_length))?;
    out.push(kind);
    out.extend_from_slice(&length.to_le_bytes());
    out.push(0);
    Ok(())
}

/// Reads the string framed at `position` in `bytes`.
pub(crate) fn read_string(bytes: &[u8], position: usize) -> Result<String, DeviceLibraryError> {
    let malformed = |reason: &str| DeviceLibraryError::MalformedString {
        position,
        reason: reason.to_owned(),
    };
    let kind = *bytes
        .get(position)
        .ok_or_else(|| malformed("it starts past its row"))?;
    if kind & 1 == 1 {
        let length = usize::from(kind >> 1)
            .checked_sub(1)
            .ok_or_else(|| malformed("its length byte is 1"))?;
        let text = bytes
            .get(position + 1..position + 1 + length)
            .ok_or_else(|| malformed("its text runs past its row"))?;
        return ascii(text).ok_or_else(|| malformed("its short text is not ASCII"));
    }
    let header = bytes
        .get(position..position + LONG_HEADER_LENGTH)
        .ok_or_else(|| malformed("its header runs past its row"))?;
    let total = usize::from(u16::from_le_bytes([header[1], header[2]]));
    if total < LONG_HEADER_LENGTH {
        return Err(malformed("its length is shorter than its header"));
    }
    let payload = bytes
        .get(position + LONG_HEADER_LENGTH..position + total)
        .ok_or_else(|| malformed("its text runs past its row"))?;
    match kind {
        LONG_ASCII_KIND => ascii(payload).ok_or_else(|| malformed("its long text is not ASCII")),
        LONG_UTF16_KIND if payload.first() == Some(&ISRC_MARKER) => {
            let isrc = payload[1..].strip_suffix(&[0]).unwrap_or(&payload[1..]);
            ascii(isrc).ok_or_else(|| malformed("its ISRC is not ASCII"))
        }
        LONG_UTF16_KIND => {
            if payload.len() % 2 != 0 {
                return Err(malformed("its UTF-16 text has an odd number of bytes"));
            }
            let units: Vec<u16> = payload
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_le_bytes(*pair))
                .collect();
            String::from_utf16(&units).map_err(|_| malformed("its text is not valid UTF-16"))
        }
        _ => Err(malformed(&format!(
            "its kind byte {kind:#04x} is not a string kind"
        ))),
    }
}

fn ascii(bytes: &[u8]) -> Option<String> {
    bytes
        .is_ascii()
        .then(|| String::from_utf8_lossy(bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encoded(text: &str) -> Vec<u8> {
        let mut out = Vec::new();
        DeviceString::for_text(text).write_to(&mut out).unwrap();
        out
    }

    #[test]
    fn rekordbox_picks_the_encoding_from_the_text() {
        assert_eq!(encoded(""), [0x03]);
        assert_eq!(encoded("Am"), [0x07, b'A', b'm']);
        let long = "a".repeat(127);
        let bytes = encoded(&long);
        assert_eq!(&bytes[..4], &[0x40, 131, 0, 0]);
        assert_eq!(bytes.len(), 131);
        assert_eq!(encoded("Stavöstranos")[..4], [0x90, 28, 0, 0]);
    }

    #[test]
    fn every_encoding_reads_back() {
        for text in [
            "",
            "Demo Track 1",
            &"x".repeat(126),
            &"y".repeat(300),
            "Stavöstranos 🎧",
        ] {
            let bytes = encoded(text);
            assert_eq!(read_string(&bytes, 0).unwrap(), text);
            assert_eq!(bytes.len(), DeviceString::for_text(text).encoded_len());
        }
    }

    #[test]
    fn an_isrc_hidden_in_a_utf16_frame_reads_as_ascii() {
        let bytes = [0x90, 10, 0, 0, 0x03, b'G', b'B', b'X', b'X', 0x00];
        assert_eq!(read_string(&bytes, 0).unwrap(), "GBXX");
    }

    #[test]
    fn a_string_running_past_its_row_is_an_error() {
        assert!(read_string(&[0x0b, b'a'], 0).is_err());
        assert!(read_string(&[0x90, 40, 0, 0, b'a', 0], 0).is_err());
    }
}
