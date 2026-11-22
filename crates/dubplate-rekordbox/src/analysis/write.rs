//! Writes a track's analysis files, `ANLZ0000.DAT`, `.EXT` and `.2EX`, as a rekordbox USB export
//! carries them: the same sections in the same order, with the headers rekordbox writes.
//!
//! The beat grid, cues, path and waveforms are written from Dubplate's own model of them, so a
//! DJ's grid fix and cues reach the players. The sections Dubplate does not model (the VBR seek
//! index, rekordbox's extended grid, the phrases and the colour waveform settings) are copied
//! byte for byte from rekordbox's own analysis when it has one.

use super::AnalysisFileKind;
use super::beat_grid::BeatGrid;
use super::cues::{AnalysisCue, Rgb, palette_color};
use super::error::FourCc;
use super::file::RawSection;
use super::waveform::{MonochromeColumn, ThreeBandColumn, TinyColumn, Waveforms};

const FILE_HEADER_LENGTH: u32 = 0x1c;
/// Bytes 0x0c to 0x1b of every file header rekordbox writes.
const FILE_HEADER_TAIL: [u8; 16] = [0, 0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];
const PATH_HEADER_LENGTH: u32 = 0x10;
const BEAT_GRID_HEADER_LENGTH: u32 = 0x18;
const BEAT_GRID_UNKNOWN: u32 = 0x0008_0000;
const PREVIEW_HEADER_LENGTH: u32 = 0x14;
const PREVIEW_UNKNOWN: u32 = 0x0001_0000;
const DETAIL_HEADER_LENGTH: u32 = 0x18;
const DETAIL_UNKNOWN: u32 = 0x0096_0000;
const COLOR_DETAIL_UNKNOWN: u32 = 0x0096_0305;
const CUE_LIST_HEADER_LENGTH: u32 = 0x18;
const CUE_ENTRY_HEADER_LENGTH: u32 = 0x1c;
const CUE_ENTRY_LENGTH: u32 = 0x38;
const CUE_ENTRY_UNKNOWN: u32 = 0x0001_0000;
const EXTENDED_CUE_LIST_HEADER_LENGTH: u32 = 0x14;
const EXTENDED_CUE_ENTRY_HEADER_LENGTH: u32 = 0x10;
/// The fixed part of an extended cue entry, up to its name.
const EXTENDED_CUE_FIXED_LENGTH: usize = 0x2c;
/// The hot cue colour and the bytes after it, which rekordbox leaves at zero.
const EXTENDED_CUE_TAIL_LENGTH: usize = 4 + 20;
/// The seven bytes after a cue's memory colour: the first is 1, as in rekordbox's exports.
const EXTENDED_CUE_FLAGS: [u8; 7] = [1, 0, 0, 0, 0, 0, 0];
/// The three bytes after a cue's type, which always read 1000.
const CUE_TYPE_TAIL: [u8; 3] = [0x00, 0x03, 0xe8];
const POSITION_CUE: u8 = 1;
const LOOP_CUE: u8 = 2;
const NO_LOOP: u32 = u32::MAX;
const NO_NEIGHBOUR: u16 = u16::MAX;
const MEMORY_LIST: u32 = 0;
const HOT_LIST: u32 = 1;
const VBR_HEADER_LENGTH: u32 = 0x10;
const VBR_ENTRIES: usize = 400;
/// An empty basic cue list says it has `0xffffffff` memory cues.
const EMPTY_MEMORY_COUNT: u32 = u32::MAX;

/// The sections copied from rekordbox's analysis when they are given, since Dubplate does not
/// model what they hold.
pub const COPIED_SECTIONS: [&str; 4] = ["PVBR", "PQT2", "PSSI", "PWVC"];

/// Why analysis files could not be written.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AnalysisWriteError {
    /// A section to copy is one Dubplate writes from its own model, or one it does not know.
    #[error("the {0} section is not one Dubplate copies; it writes it or leaves it out")]
    NotCopied(FourCc),
    /// A section or file would be longer than its 32-bit length can say.
    #[error("the {0} section is longer than its length field can say")]
    TooLong(FourCc),
}

/// What a track's analysis files hold.
#[derive(Debug, Clone, Copy)]
pub struct AnalysisExport<'a> {
    /// The audio file as players see it on the stick, such as `/Contents/Artist/Album/Track.mp3`.
    pub audio_path: &'a str,
    /// The beat grid, with the DJ's fix applied.
    pub beat_grid: Option<&'a BeatGrid>,
    /// Memory cues, hot cues and saved loops.
    pub cues: &'a [AnalysisCue],
    /// The waveforms; each one that is `None` is left out.
    pub waveforms: &'a Waveforms,
    /// Sections of rekordbox's analysis to copy as they are, from [`COPIED_SECTIONS`].
    pub copied: &'a [RawSection],
}

/// The three analysis files of a track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisFiles {
    /// `ANLZ0000.DAT`: the grid, basic cues and the blue overviews.
    pub dat: Vec<u8>,
    /// `ANLZ0000.EXT`: the cues with colours and names, the detail and colour waveforms, the
    /// phrases.
    pub ext: Vec<u8>,
    /// `ANLZ0000.2EX`: the 3-band waveforms.
    pub two_ex: Vec<u8>,
}

impl AnalysisFiles {
    /// Writes the three files of `export`.
    pub fn write(export: &AnalysisExport<'_>) -> Result<Self, AnalysisWriteError> {
        for section in export.copied {
            if !COPIED_SECTIONS
                .iter()
                .any(|code| code.as_bytes() == section.fourcc.0)
            {
                return Err(AnalysisWriteError::NotCopied(section.fourcc));
            }
        }
        let copied = |fourcc: FourCc| {
            export
                .copied
                .iter()
                .find(|section| section.fourcc == fourcc)
                .map(|section| section.bytes.clone())
        };
        let waveforms = export.waveforms;
        let hot = cue_list(export.cues, HOT_LIST);
        let memory = cue_list(export.cues, MEMORY_LIST);
        let path = path_section(export.audio_path)?;

        let mut dat = vec![
            path.clone(),
            copied(FourCc::VBR_INDEX).unwrap_or_else(empty_vbr_index),
        ];
        dat.extend(export.beat_grid.map(beat_grid_section).transpose()?);
        dat.extend(
            waveforms
                .preview
                .as_deref()
                .map(|columns| preview_section(FourCc::WAVEFORM_PREVIEW, monochrome(columns)))
                .transpose()?,
        );
        dat.extend(
            waveforms
                .tiny_preview
                .as_deref()
                .map(|columns| preview_section(FourCc::TINY_WAVEFORM_PREVIEW, tiny(columns)))
                .transpose()?,
        );
        dat.push(basic_cue_section(HOT_LIST, &hot)?);
        dat.push(basic_cue_section(MEMORY_LIST, &memory)?);

        let mut ext = vec![path.clone()];
        ext.extend(
            waveforms
                .detail
                .as_deref()
                .map(|columns| {
                    detail_section(
                        FourCc::WAVEFORM_DETAIL,
                        DETAIL_HEADER_LENGTH,
                        1,
                        Some(DETAIL_UNKNOWN),
                        &monochrome(columns),
                    )
                })
                .transpose()?,
        );
        ext.push(basic_cue_section(HOT_LIST, &hot)?);
        ext.push(basic_cue_section(MEMORY_LIST, &memory)?);
        ext.push(extended_cue_section(HOT_LIST, &hot)?);
        ext.push(extended_cue_section(MEMORY_LIST, &memory)?);
        ext.extend(copied(FourCc::EXTENDED_BEAT_GRID));
        ext.extend(
            waveforms
                .color_detail
                .as_deref()
                .map(|columns| {
                    let bytes: Vec<u8> = columns
                        .iter()
                        .flat_map(|column| column.0.to_be_bytes())
                        .collect();
                    detail_section(
                        FourCc::COLOR_WAVEFORM_DETAIL,
                        DETAIL_HEADER_LENGTH,
                        2,
                        Some(COLOR_DETAIL_UNKNOWN),
                        &bytes,
                    )
                })
                .transpose()?,
        );
        ext.extend(
            waveforms
                .color_preview
                .as_deref()
                .map(|columns| {
                    let bytes: Vec<u8> = columns.iter().flat_map(|column| column.0).collect();
                    detail_section(
                        FourCc::COLOR_WAVEFORM_PREVIEW,
                        DETAIL_HEADER_LENGTH,
                        6,
                        Some(0),
                        &bytes,
                    )
                })
                .transpose()?,
        );
        ext.extend(copied(FourCc::SONG_STRUCTURE));

        let mut two_ex = vec![path];
        two_ex.extend(
            waveforms
                .three_band_detail
                .as_deref()
                .map(|columns| {
                    detail_section(
                        FourCc::THREE_BAND_WAVEFORM_DETAIL,
                        DETAIL_HEADER_LENGTH,
                        3,
                        Some(DETAIL_UNKNOWN),
                        &three_band(columns),
                    )
                })
                .transpose()?,
        );
        two_ex.extend(
            waveforms
                .three_band_preview
                .as_deref()
                .map(|columns| {
                    detail_section(
                        FourCc::THREE_BAND_WAVEFORM_PREVIEW,
                        PREVIEW_HEADER_LENGTH,
                        3,
                        None,
                        &three_band(columns),
                    )
                })
                .transpose()?,
        );
        two_ex.extend(copied(FourCc::WAVEFORM_COLOR_INFO));

        Ok(Self {
            dat: file(&dat)?,
            ext: file(&ext)?,
            two_ex: file(&two_ex)?,
        })
    }

    /// The bytes of the file of kind `kind`.
    pub fn file(&self, kind: AnalysisFileKind) -> &[u8] {
        match kind {
            AnalysisFileKind::Dat => &self.dat,
            AnalysisFileKind::Ext => &self.ext,
            AnalysisFileKind::TwoEx => &self.two_ex,
        }
    }
}

fn length_of(fourcc: FourCc, length: usize) -> Result<u32, AnalysisWriteError> {
    u32::try_from(length).map_err(|_| AnalysisWriteError::TooLong(fourcc))
}

fn file(sections: &[Vec<u8>]) -> Result<Vec<u8>, AnalysisWriteError> {
    let length = FILE_HEADER_LENGTH as usize + sections.iter().map(Vec::len).sum::<usize>();
    let mut bytes = Vec::with_capacity(length);
    bytes.extend_from_slice(&FourCc::FILE_MAGIC.0);
    bytes.extend_from_slice(&FILE_HEADER_LENGTH.to_be_bytes());
    bytes.extend_from_slice(&length_of(FourCc::FILE_MAGIC, length)?.to_be_bytes());
    bytes.extend_from_slice(&FILE_HEADER_TAIL);
    for section in sections {
        bytes.extend_from_slice(section);
    }
    Ok(bytes)
}

/// A section: its code, its header length, its total length, then `header` and `body`.
fn section(
    fourcc: FourCc,
    header_length: u32,
    header: &[u8],
    body: &[u8],
) -> Result<Vec<u8>, AnalysisWriteError> {
    debug_assert_eq!(12 + header.len(), header_length as usize);
    let length = 12 + header.len() + body.len();
    let mut bytes = Vec::with_capacity(length);
    bytes.extend_from_slice(&fourcc.0);
    bytes.extend_from_slice(&header_length.to_be_bytes());
    bytes.extend_from_slice(&length_of(fourcc, length)?.to_be_bytes());
    bytes.extend_from_slice(header);
    bytes.extend_from_slice(body);
    Ok(bytes)
}

fn utf16_be_with_nul(text: &str) -> Vec<u8> {
    text.encode_utf16()
        .chain([0])
        .flat_map(u16::to_be_bytes)
        .collect()
}

fn path_section(audio_path: &str) -> Result<Vec<u8>, AnalysisWriteError> {
    let path = utf16_be_with_nul(audio_path);
    let length = length_of(FourCc::PATH, path.len())?;
    section(
        FourCc::PATH,
        PATH_HEADER_LENGTH,
        &length.to_be_bytes(),
        &path,
    )
}

fn empty_vbr_index() -> Vec<u8> {
    let body = vec![0_u8; 4 * (VBR_ENTRIES + 1)];
    let mut bytes = Vec::with_capacity(16 + body.len());
    bytes.extend_from_slice(&FourCc::VBR_INDEX.0);
    bytes.extend_from_slice(&VBR_HEADER_LENGTH.to_be_bytes());
    bytes.extend_from_slice(&((16 + body.len()) as u32).to_be_bytes());
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(&body);
    bytes
}

fn beat_grid_section(grid: &BeatGrid) -> Result<Vec<u8>, AnalysisWriteError> {
    let count = length_of(FourCc::BEAT_GRID, grid.beats.len())?;
    let mut header = Vec::with_capacity(12);
    header.extend_from_slice(&0_u32.to_be_bytes());
    header.extend_from_slice(&BEAT_GRID_UNKNOWN.to_be_bytes());
    header.extend_from_slice(&count.to_be_bytes());
    let body: Vec<u8> = grid
        .beats
        .iter()
        .flat_map(|beat| {
            let mut entry = [0_u8; 8];
            entry[..2].copy_from_slice(&beat.beat_in_bar.to_be_bytes());
            entry[2..4].copy_from_slice(&beat.tempo_hundredths.to_be_bytes());
            entry[4..].copy_from_slice(&beat.time_ms.to_be_bytes());
            entry
        })
        .collect();
    section(FourCc::BEAT_GRID, BEAT_GRID_HEADER_LENGTH, &header, &body)
}

fn monochrome(columns: &[MonochromeColumn]) -> Vec<u8> {
    columns.iter().map(|column| column.0).collect()
}

fn tiny(columns: &[TinyColumn]) -> Vec<u8> {
    columns.iter().map(|column| column.0).collect()
}

fn three_band(columns: &[ThreeBandColumn]) -> Vec<u8> {
    columns
        .iter()
        .flat_map(|column| [column.low, column.mid, column.high])
        .collect()
}

fn preview_section(fourcc: FourCc, data: Vec<u8>) -> Result<Vec<u8>, AnalysisWriteError> {
    let mut header = Vec::with_capacity(8);
    header.extend_from_slice(&length_of(fourcc, data.len())?.to_be_bytes());
    header.extend_from_slice(&PREVIEW_UNKNOWN.to_be_bytes());
    section(fourcc, PREVIEW_HEADER_LENGTH, &header, &data)
}

/// A waveform section that gives its entry size and count, then, for most kinds, a fourth
/// header field rekordbox fills with a fixed value.
fn detail_section(
    fourcc: FourCc,
    header_length: u32,
    entry_size: u32,
    unknown: Option<u32>,
    data: &[u8],
) -> Result<Vec<u8>, AnalysisWriteError> {
    let count = length_of(fourcc, data.len() / entry_size as usize)?;
    let mut header = Vec::with_capacity(12);
    header.extend_from_slice(&entry_size.to_be_bytes());
    header.extend_from_slice(&count.to_be_bytes());
    if let Some(unknown) = unknown {
        header.extend_from_slice(&unknown.to_be_bytes());
    }
    section(fourcc, header_length, &header, data)
}

/// The cues of one list, ordered as rekordbox orders them: hot cues by pad, memory cues by time.
fn cue_list(cues: &[AnalysisCue], list: u32) -> Vec<&AnalysisCue> {
    let mut chosen: Vec<&AnalysisCue> = cues
        .iter()
        .filter(|cue| (cue.hot_cue.is_some()) == (list == HOT_LIST))
        .collect();
    chosen.sort_by_key(|cue| (cue.hot_cue, cue.time_ms));
    chosen
}

fn cue_kind_and_end(cue: &AnalysisCue) -> (u8, u32) {
    match cue.loop_end_ms {
        Some(end) => (LOOP_CUE, end),
        None => (POSITION_CUE, NO_LOOP),
    }
}

fn basic_cue_section(list: u32, cues: &[&AnalysisCue]) -> Result<Vec<u8>, AnalysisWriteError> {
    let count =
        u16::try_from(cues.len()).map_err(|_| AnalysisWriteError::TooLong(FourCc::CUE_LIST))?;
    let mut header = Vec::with_capacity(12);
    header.extend_from_slice(&list.to_be_bytes());
    header.extend_from_slice(&0_u16.to_be_bytes());
    header.extend_from_slice(&count.to_be_bytes());
    let memory_count = if cues.is_empty() {
        EMPTY_MEMORY_COUNT
    } else {
        u32::from(count)
    };
    header.extend_from_slice(&memory_count.to_be_bytes());
    let mut body = Vec::with_capacity(cues.len() * CUE_ENTRY_LENGTH as usize);
    for (place, cue) in cues.iter().enumerate() {
        let (kind, end) = cue_kind_and_end(cue);
        let previous = place
            .checked_sub(1)
            .and_then(|previous| u16::try_from(previous).ok())
            .unwrap_or(NO_NEIGHBOUR);
        let next = if place + 1 < cues.len() {
            u16::try_from(place + 1).unwrap_or(NO_NEIGHBOUR)
        } else {
            NO_NEIGHBOUR
        };
        body.extend_from_slice(b"PCPT");
        body.extend_from_slice(&CUE_ENTRY_HEADER_LENGTH.to_be_bytes());
        body.extend_from_slice(&CUE_ENTRY_LENGTH.to_be_bytes());
        body.extend_from_slice(&cue.hot_cue.unwrap_or(0).to_be_bytes());
        body.extend_from_slice(&0_u32.to_be_bytes());
        body.extend_from_slice(&CUE_ENTRY_UNKNOWN.to_be_bytes());
        body.extend_from_slice(&previous.to_be_bytes());
        body.extend_from_slice(&next.to_be_bytes());
        body.push(kind);
        body.extend_from_slice(&CUE_TYPE_TAIL);
        body.extend_from_slice(&cue.time_ms.to_be_bytes());
        body.extend_from_slice(&end.to_be_bytes());
        body.extend_from_slice(&[0; 16]);
    }
    section(FourCc::CUE_LIST, CUE_LIST_HEADER_LENGTH, &header, &body)
}

fn extended_cue_section(list: u32, cues: &[&AnalysisCue]) -> Result<Vec<u8>, AnalysisWriteError> {
    let fourcc = FourCc::EXTENDED_CUE_LIST;
    let count = u16::try_from(cues.len()).map_err(|_| AnalysisWriteError::TooLong(fourcc))?;
    let mut header = Vec::with_capacity(8);
    header.extend_from_slice(&list.to_be_bytes());
    header.extend_from_slice(&count.to_be_bytes());
    header.extend_from_slice(&0_u16.to_be_bytes());
    let mut body = Vec::new();
    for cue in cues {
        let (kind, end) = cue_kind_and_end(cue);
        let name = cue
            .name
            .as_deref()
            .filter(|name| !name.is_empty())
            .map(utf16_be_with_nul)
            .unwrap_or_default();
        let length = length_of(
            fourcc,
            EXTENDED_CUE_FIXED_LENGTH + name.len() + EXTENDED_CUE_TAIL_LENGTH,
        )?;
        let (numerator, denominator) = cue
            .loop_length
            .map_or((0, 0), |length| (length.numerator, length.denominator));
        let (code, led) = match cue.hot_cue_color {
            Some(color) => (color.code, color.led.or_else(|| palette_color(color.code))),
            None => (0, None),
        };
        let Rgb { red, green, blue } = led.unwrap_or(Rgb {
            red: 0,
            green: 0,
            blue: 0,
        });
        body.extend_from_slice(b"PCP2");
        body.extend_from_slice(&EXTENDED_CUE_ENTRY_HEADER_LENGTH.to_be_bytes());
        body.extend_from_slice(&length.to_be_bytes());
        body.extend_from_slice(&cue.hot_cue.unwrap_or(0).to_be_bytes());
        body.push(kind);
        body.extend_from_slice(&CUE_TYPE_TAIL);
        body.extend_from_slice(&cue.time_ms.to_be_bytes());
        body.extend_from_slice(&end.to_be_bytes());
        body.push(if cue.hot_cue.is_none() {
            cue.memory_color.unwrap_or(0)
        } else {
            0
        });
        body.extend_from_slice(&EXTENDED_CUE_FLAGS);
        body.extend_from_slice(&numerator.to_be_bytes());
        body.extend_from_slice(&denominator.to_be_bytes());
        body.extend_from_slice(&length_of(fourcc, name.len())?.to_be_bytes());
        body.extend_from_slice(&name);
        body.extend_from_slice(&[code, red, green, blue]);
        body.extend_from_slice(&[0; EXTENDED_CUE_TAIL_LENGTH - 4]);
    }
    section(fourcc, EXTENDED_CUE_LIST_HEADER_LENGTH, &header, &body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::{AnalysisFile, Beat, Section};

    #[test]
    fn the_file_header_is_rekordbox_s() {
        let waveforms = Waveforms::default();
        let files = AnalysisFiles::write(&AnalysisExport {
            audio_path: "/Contents/A/B/C.mp3",
            beat_grid: None,
            cues: &[],
            waveforms: &waveforms,
            copied: &[],
        })
        .unwrap();
        assert_eq!(&files.dat[..8], b"PMAI\0\0\0\x1c");
        assert_eq!(files.dat[12..28], FILE_HEADER_TAIL);
        let declared =
            u32::from_be_bytes([files.dat[8], files.dat[9], files.dat[10], files.dat[11]]);
        assert_eq!(declared as usize, files.dat.len());
    }

    #[test]
    fn an_empty_cue_list_is_written_as_rekordbox_writes_it() {
        let bytes = basic_cue_section(HOT_LIST, &[]).unwrap();
        assert_eq!(
            bytes,
            [
                b'P', b'C', b'O', b'B', 0, 0, 0, 0x18, 0, 0, 0, 0x18, 0, 0, 0, 1, 0, 0, 0, 0, 0xff,
                0xff, 0xff, 0xff
            ]
        );
        let bytes = extended_cue_section(MEMORY_LIST, &[]).unwrap();
        assert_eq!(
            bytes,
            [
                b'P', b'C', b'O', b'2', 0, 0, 0, 0x14, 0, 0, 0, 0x14, 0, 0, 0, 0, 0, 0, 0, 0
            ]
        );
    }

    #[test]
    fn a_grid_reads_back() {
        let grid = BeatGrid {
            beats: vec![
                Beat {
                    beat_in_bar: 1,
                    tempo_hundredths: 12_200,
                    time_ms: 160,
                },
                Beat {
                    beat_in_bar: 2,
                    tempo_hundredths: 12_200,
                    time_ms: 652,
                },
            ],
        };
        let section_bytes = beat_grid_section(&grid).unwrap();
        let file = file(&[section_bytes]).unwrap();
        let parsed = AnalysisFile::parse(&file).unwrap();
        assert_eq!(parsed.sections, vec![Section::BeatGrid(grid)]);
    }
}
