use super::error::{AnalysisError, FourCc};

/// Big-endian, bounds-checked reads inside one section, at positions counted like the
/// format's documentation: from the start of the section, or of the entry being read. `base` is
/// where that entry starts in the section.
#[derive(Clone, Copy)]
pub(crate) struct SectionReader<'data> {
    fourcc: FourCc,
    section_offset: usize,
    base: usize,
    bytes: &'data [u8],
}

impl<'data> SectionReader<'data> {
    pub(crate) fn new(fourcc: FourCc, section_offset: usize, bytes: &'data [u8]) -> Self {
        Self {
            fourcc,
            section_offset,
            base: 0,
            bytes,
        }
    }

    pub(crate) fn fourcc(&self) -> FourCc {
        self.fourcc
    }

    pub(crate) fn section_offset(&self) -> usize {
        self.section_offset
    }

    pub(crate) fn len(&self) -> usize {
        self.bytes.len()
    }

    pub(crate) fn malformed(&self, reason: impl Into<String>) -> AnalysisError {
        AnalysisError::Malformed {
            fourcc: self.fourcc,
            offset: self.section_offset,
            reason: reason.into(),
        }
    }

    pub(crate) fn bytes_at(
        &self,
        position: usize,
        length: usize,
        field: &str,
    ) -> Result<&'data [u8], AnalysisError> {
        position
            .checked_add(length)
            .and_then(|end| self.bytes.get(position..end))
            .ok_or_else(|| {
                let size = self.bytes.len();
                self.malformed(if self.base == 0 {
                    format!(
                        "{field} at byte {position:#x} needs {length} bytes but the section \
                         ends at byte {size:#x}"
                    )
                } else {
                    format!(
                        "{field} at byte {position:#x} of the entry at byte {:#x} needs \
                         {length} bytes but the entry is {size} bytes long",
                        self.base
                    )
                })
            })
    }

    pub(crate) fn u8_at(&self, position: usize, field: &str) -> Result<u8, AnalysisError> {
        Ok(self.bytes_at(position, 1, field)?[0])
    }

    pub(crate) fn u16_at(&self, position: usize, field: &str) -> Result<u16, AnalysisError> {
        let bytes = self.bytes_at(position, 2, field)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    pub(crate) fn u32_at(&self, position: usize, field: &str) -> Result<u32, AnalysisError> {
        let bytes = self.bytes_at(position, 4, field)?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// A reader over `length` bytes from `position`, for an entry inside the section.
    pub(crate) fn entry(
        &self,
        position: usize,
        length: usize,
        field: &str,
    ) -> Result<Self, AnalysisError> {
        let bytes = self.bytes_at(position, length, field)?;
        Ok(Self {
            fourcc: self.fourcc,
            section_offset: self.section_offset,
            base: self.base + position,
            bytes,
        })
    }

    /// Checks that `count` entries of `entry_size` bytes from `start` fill the section exactly.
    pub(crate) fn expect_entries(
        &self,
        start: usize,
        count: u32,
        entry_size: usize,
        what: &str,
    ) -> Result<&'data [u8], AnalysisError> {
        let needed = usize::try_from(count)
            .ok()
            .and_then(|count| count.checked_mul(entry_size))
            .and_then(|size| size.checked_add(start));
        match needed {
            Some(needed) if needed == self.bytes.len() => Ok(&self.bytes[start..]),
            _ => Err(self.malformed(format!(
                "{count} {what} of {entry_size} bytes after byte {start:#x} need a section of \
                 {} bytes but it is {} bytes",
                needed.map_or_else(|| "too many".to_owned(), |needed| needed.to_string()),
                self.bytes.len(),
            ))),
        }
    }
}

pub(crate) fn utf16_be_string(
    reader: &SectionReader<'_>,
    bytes: &[u8],
    field: &str,
) -> Result<Option<String>, AnalysisError> {
    if !bytes.len().is_multiple_of(2) {
        return Err(reader.malformed(format!(
            "{field} is {} bytes long, which is not whole UTF-16 characters",
            bytes.len()
        )));
    }
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_be_bytes(*pair))
        .collect();
    let text = String::from_utf16(&units)
        .map_err(|_| reader.malformed(format!("{field} is not valid UTF-16")))?;
    let text = text.trim_end_matches('\0');
    Ok((!text.is_empty()).then(|| text.to_owned()))
}
