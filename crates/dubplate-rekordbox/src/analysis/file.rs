use serde::Serialize;

use super::beat_grid::{self, BeatGrid};
use super::cues::{self, CueList};
use super::error::{AnalysisError, FourCc};
use super::reader::{SectionReader, utf16_be_string};
use super::song_structure::{self, SongStructure};
use super::waveform::{
    self, ColorDetailColumn, ColorPreviewColumn, MonochromeColumn, ThreeBandColumn, TinyColumn,
};

/// A section of an analysis file that Dubplate does not read, kept so it can be reported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkippedSection {
    /// The section's code.
    pub fourcc: FourCc,
    /// Where the section starts in the file.
    pub offset: usize,
    /// The section's length in bytes, header included.
    pub length: u32,
}

/// One section of an analysis file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Section {
    /// `PQTZ`: the beat grid.
    BeatGrid(BeatGrid),
    /// `PCOB` or `PCO2`: memory cues or hot cues.
    CueList(CueList),
    /// `PPTH`: the audio file the analysis was made from.
    AudioPath(Option<String>),
    /// `PWAV`: the blue overview.
    WaveformPreview(Vec<MonochromeColumn>),
    /// `PWV2`: the tiny overview.
    TinyWaveformPreview(Vec<TinyColumn>),
    /// `PWV3`: the scrolling blue waveform.
    WaveformDetail(Vec<MonochromeColumn>),
    /// `PWV4`: the colour overview.
    ColorWaveformPreview(Vec<ColorPreviewColumn>),
    /// `PWV5`: the scrolling colour waveform.
    ColorWaveformDetail(Vec<ColorDetailColumn>),
    /// `PWV6`: the 3-band overview.
    ThreeBandWaveformPreview(Vec<ThreeBandColumn>),
    /// `PWV7`: the scrolling 3-band waveform.
    ThreeBandWaveformDetail(Vec<ThreeBandColumn>),
    /// `PSSI`: the phrases.
    SongStructure(SongStructure),
    /// A section Dubplate does not read.
    Skipped(SkippedSection),
}

/// The sections of one `.DAT`, `.EXT` or `.2EX` analysis file, in file order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisFile {
    /// The sections in the order they appear.
    pub sections: Vec<Section>,
}

const FILE_HEADER_MINIMUM: usize = 12;
const SECTION_HEADER_SIZE: usize = 12;
const PATH_LENGTH_POSITION: usize = 0x0c;
const PATH_POSITION: usize = 0x10;

/// A section exactly as it is in its file, header included, for copying it into another file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawSection {
    /// The section's code.
    pub fourcc: FourCc,
    /// The whole section, from its code to its last byte.
    pub bytes: Vec<u8>,
}

/// A section's place in its file, before its content is read.
struct SectionSlice<'data> {
    fourcc: FourCc,
    offset: usize,
    declared_length: u32,
    bytes: &'data [u8],
}

impl AnalysisFile {
    /// Reads the bytes of an analysis file.
    ///
    /// Sections of unknown kinds are kept as [`Section::Skipped`]. A known section whose bytes do
    /// not match its layout is an error, so nothing half-read is ever returned.
    pub fn parse(bytes: &[u8]) -> Result<Self, AnalysisError> {
        let sections = split_sections(bytes)?
            .into_iter()
            .map(|slice| {
                let reader = SectionReader::new(slice.fourcc, slice.offset, slice.bytes);
                parse_section(&reader, slice.declared_length)
            })
            .collect::<Result<_, _>>()?;
        Ok(Self { sections })
    }

    /// The sections of an analysis file as they are, without reading what they hold, so they
    /// can be copied into another file byte for byte. The file and section headers are checked
    /// as [`Self::parse`] checks them.
    pub fn raw_sections(bytes: &[u8]) -> Result<Vec<RawSection>, AnalysisError> {
        Ok(split_sections(bytes)?
            .into_iter()
            .map(|slice| RawSection {
                fourcc: slice.fourcc,
                bytes: slice.bytes.to_vec(),
            })
            .collect())
    }
}

/// Checks the file header and cuts the rest of the file into its sections.
fn split_sections(bytes: &[u8]) -> Result<Vec<SectionSlice<'_>>, AnalysisError> {
    if let Some(magic) = bytes.get(..4)
        && magic != FourCc::FILE_MAGIC.0
    {
        return Err(AnalysisError::NotAnalysisFile {
            found: FourCc([magic[0], magic[1], magic[2], magic[3]]),
        });
    }
    if bytes.len() < FILE_HEADER_MINIMUM {
        return Err(AnalysisError::BadFileHeader {
            reason: format!(
                "the file is {} bytes, shorter than the {FILE_HEADER_MINIMUM}-byte header",
                bytes.len()
            ),
        });
    }
    let header_length = read_u32(bytes, 4);
    let file_length = read_u32(bytes, 8);
    if usize::try_from(file_length).ok() != Some(bytes.len()) {
        return Err(AnalysisError::FileLengthMismatch {
            declared: file_length,
            actual: bytes.len(),
        });
    }
    let header_length = usize::try_from(header_length).unwrap_or(usize::MAX);
    if !(FILE_HEADER_MINIMUM..=bytes.len()).contains(&header_length) {
        return Err(AnalysisError::BadFileHeader {
            reason: format!(
                "the header says it is {header_length} bytes long, which does not fit \
                 between {FILE_HEADER_MINIMUM} bytes and the file's {} bytes",
                bytes.len()
            ),
        });
    }

    let mut sections = Vec::new();
    let mut offset = header_length;
    while offset < bytes.len() {
        let remaining = &bytes[offset..];
        if remaining.len() < SECTION_HEADER_SIZE {
            return Err(AnalysisError::BadFileHeader {
                reason: format!(
                    "{} bytes are left at byte {offset:#x}, too few for a section header",
                    remaining.len()
                ),
            });
        }
        let fourcc = FourCc([remaining[0], remaining[1], remaining[2], remaining[3]]);
        let section_header_length = read_u32(remaining, 4);
        let declared = read_u32(remaining, 8);
        let section_length = usize::try_from(declared).unwrap_or(usize::MAX);
        if section_length < SECTION_HEADER_SIZE {
            return Err(AnalysisError::Malformed {
                fourcc,
                offset,
                reason: format!(
                    "it says it is {declared} bytes long, shorter than the \
                     {SECTION_HEADER_SIZE}-byte section header"
                ),
            });
        }
        if section_length > remaining.len() {
            return Err(AnalysisError::SectionTruncated {
                fourcc,
                offset,
                declared,
                available: remaining.len(),
            });
        }
        if usize::try_from(section_header_length).map_or(true, |length| {
            !(SECTION_HEADER_SIZE..=section_length).contains(&length)
        }) {
            return Err(AnalysisError::Malformed {
                fourcc,
                offset,
                reason: format!(
                    "its header length {section_header_length} does not fit between \
                     {SECTION_HEADER_SIZE} bytes and its length of {declared} bytes"
                ),
            });
        }
        sections.push(SectionSlice {
            fourcc,
            offset,
            declared_length: declared,
            bytes: &remaining[..section_length],
        });
        offset += section_length;
    }
    Ok(sections)
}

fn parse_section(reader: &SectionReader<'_>, length: u32) -> Result<Section, AnalysisError> {
    let section = match reader.fourcc() {
        FourCc::BEAT_GRID => Section::BeatGrid(beat_grid::parse(reader)?),
        FourCc::CUE_LIST => Section::CueList(cues::parse(reader)?),
        FourCc::EXTENDED_CUE_LIST => Section::CueList(cues::parse_extended(reader)?),
        FourCc::PATH => {
            let length = reader.u32_at(PATH_LENGTH_POSITION, "the path length")?;
            let path = reader.expect_entries(PATH_POSITION, length, 1, "path bytes")?;
            Section::AudioPath(utf16_be_string(reader, path, "the path")?)
        }
        FourCc::WAVEFORM_PREVIEW => Section::WaveformPreview(
            waveform::parse_preview_bytes(reader)?
                .iter()
                .map(|byte| MonochromeColumn(*byte))
                .collect(),
        ),
        FourCc::TINY_WAVEFORM_PREVIEW => Section::TinyWaveformPreview(
            waveform::parse_preview_bytes(reader)?
                .iter()
                .map(|byte| TinyColumn(*byte))
                .collect(),
        ),
        FourCc::WAVEFORM_DETAIL => Section::WaveformDetail(waveform::parse_detail(reader)?),
        FourCc::COLOR_WAVEFORM_PREVIEW => {
            Section::ColorWaveformPreview(waveform::parse_color_preview(reader)?)
        }
        FourCc::COLOR_WAVEFORM_DETAIL => {
            Section::ColorWaveformDetail(waveform::parse_color_detail(reader)?)
        }
        FourCc::THREE_BAND_WAVEFORM_PREVIEW => {
            Section::ThreeBandWaveformPreview(waveform::parse_three_band_preview(reader)?)
        }
        FourCc::THREE_BAND_WAVEFORM_DETAIL => {
            Section::ThreeBandWaveformDetail(waveform::parse_three_band_detail(reader)?)
        }
        FourCc::SONG_STRUCTURE => Section::SongStructure(song_structure::parse(reader)?),
        fourcc => Section::Skipped(SkippedSection {
            fourcc,
            offset: reader.section_offset(),
            length,
        }),
    };
    Ok(section)
}

fn read_u32(bytes: &[u8], position: usize) -> u32 {
    u32::from_be_bytes([
        bytes[position],
        bytes[position + 1],
        bytes[position + 2],
        bytes[position + 3],
    ])
}
