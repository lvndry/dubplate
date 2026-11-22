use std::fmt;

use serde::{Serialize, Serializer};

/// The four-character code that names a section of an analysis file, such as `PQTZ`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct FourCc(pub [u8; 4]);

impl FourCc {
    pub(crate) const BEAT_GRID: Self = Self(*b"PQTZ");
    pub(crate) const CUE_LIST: Self = Self(*b"PCOB");
    pub(crate) const EXTENDED_CUE_LIST: Self = Self(*b"PCO2");
    pub(crate) const PATH: Self = Self(*b"PPTH");
    pub(crate) const WAVEFORM_PREVIEW: Self = Self(*b"PWAV");
    pub(crate) const TINY_WAVEFORM_PREVIEW: Self = Self(*b"PWV2");
    pub(crate) const WAVEFORM_DETAIL: Self = Self(*b"PWV3");
    pub(crate) const COLOR_WAVEFORM_PREVIEW: Self = Self(*b"PWV4");
    pub(crate) const COLOR_WAVEFORM_DETAIL: Self = Self(*b"PWV5");
    pub(crate) const THREE_BAND_WAVEFORM_PREVIEW: Self = Self(*b"PWV6");
    pub(crate) const THREE_BAND_WAVEFORM_DETAIL: Self = Self(*b"PWV7");
    pub(crate) const SONG_STRUCTURE: Self = Self(*b"PSSI");
    pub(crate) const FILE_MAGIC: Self = Self(*b"PMAI");
    pub(crate) const VBR_INDEX: Self = Self(*b"PVBR");
    pub(crate) const EXTENDED_BEAT_GRID: Self = Self(*b"PQT2");
    pub(crate) const WAVEFORM_COLOR_INFO: Self = Self(*b"PWVC");
}

impl fmt::Display for FourCc {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.iter().all(|byte| byte.is_ascii_graphic()) {
            self.0
                .iter()
                .try_for_each(|byte| write!(formatter, "{}", char::from(*byte)))
        } else {
            write!(
                formatter,
                "0x{:02x}{:02x}{:02x}{:02x}",
                self.0[0], self.0[1], self.0[2], self.0[3]
            )
        }
    }
}

impl fmt::Debug for FourCc {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "FourCc({self})")
    }
}

impl Serialize for FourCc {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// Why the bytes of an analysis file could not be read.
///
/// Byte positions are given in hexadecimal, like the offsets in the format's documentation.
/// `offset` is where the section starts in the file and `position` is counted from the start of
/// the section.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AnalysisError {
    /// The file does not start with `PMAI`.
    #[error("not a rekordbox analysis file: it starts with {found} instead of PMAI")]
    NotAnalysisFile {
        /// The first four bytes of the file.
        found: FourCc,
    },
    /// The file header is cut short or contradicts itself.
    #[error("the analysis file header is unusable: {reason}")]
    BadFileHeader {
        /// What is wrong with it.
        reason: String,
    },
    /// The length the header announces is not the length of the file.
    #[error("the analysis file header says the file is {declared} bytes long but it is {actual}")]
    FileLengthMismatch {
        /// The length in the header.
        declared: u32,
        /// The length of the bytes that were read.
        actual: usize,
    },
    /// A section announces more bytes than the file has left.
    #[error(
        "the {fourcc} section at byte {offset:#x} says it is {declared} bytes long \
         but only {available} bytes are left in the file"
    )]
    SectionTruncated {
        /// The section's code.
        fourcc: FourCc,
        /// Where the section starts in the file.
        offset: usize,
        /// The length the section announces.
        declared: u32,
        /// The bytes left in the file from the section's start.
        available: usize,
    },
    /// A section's content does not match its layout.
    #[error("the {fourcc} section at byte {offset:#x} is malformed: {reason}")]
    Malformed {
        /// The section's code.
        fourcc: FourCc,
        /// Where the section starts in the file.
        offset: usize,
        /// What is wrong with it.
        reason: String,
    },
}
