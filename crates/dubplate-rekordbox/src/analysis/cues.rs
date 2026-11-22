use serde::Serialize;

use crate::cue_colors::PaletteColor;
pub use crate::cue_colors::Rgb;

use super::error::{AnalysisError, FourCc};
use super::reader::{SectionReader, utf16_be_string};

/// Whether a cue list holds memory cues or hot cues.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CueListKind {
    /// Memory cues and memory loops.
    Memory,
    /// Hot cues and hot loops.
    Hot,
}

/// A cue list section: `PCOB`, or `PCO2`, its extended form with colours and names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CueList {
    /// Memory cues or hot cues.
    pub kind: CueListKind,
    /// Whether it came from the extended `PCO2` section.
    pub extended: bool,
    /// The cues in the order rekordbox stored them.
    pub cues: Vec<AnalysisCue>,
}

/// The colour of a hot cue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct HotCueColor {
    /// rekordbox's palette code, from 1 to 62. 0 means the default green.
    pub code: u8,
    /// The colour rekordbox shows for `code`, if the code is in its palette.
    pub display: Option<Rgb>,
    /// The colour players light their pads with, stored next to the code.
    pub led: Option<Rgb>,
}

/// The size of a quantized loop, in beats, as a fraction such as 4/1 or 1/2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct LoopLength {
    /// Beats above the line.
    pub numerator: u16,
    /// Beats below the line.
    pub denominator: u16,
}

/// A memory cue, hot cue or saved loop from an analysis file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AnalysisCue {
    /// The hot cue number, 1 for pad A to 8 for pad H, or `None` for a memory cue.
    pub hot_cue: Option<u32>,
    /// The pad letter of a hot cue, from `A` to `H`.
    pub pad: Option<char>,
    /// Where the cue sits, in milliseconds at normal speed.
    pub time_ms: u32,
    /// Where the loop ends, when the cue is a loop.
    pub loop_end_ms: Option<u32>,
    /// The loop's length in beats, when it is a quantized loop.
    pub loop_length: Option<LoopLength>,
    /// The name the DJ gave the cue.
    pub name: Option<String>,
    /// The colour table row of a memory cue or loop, from 1 (pink) to 8 (purple).
    pub memory_color: Option<u8>,
    /// The colour of a hot cue.
    pub hot_cue_color: Option<HotCueColor>,
}

/// The pad letter of an analysis file's hot cue number, where 1 is A.
pub fn hot_cue_pad(hot_cue: u32) -> Option<char> {
    let index = u8::try_from(hot_cue.checked_sub(1)?).ok()?;
    (index < 8).then(|| char::from(b'A' + index))
}

/// The name of a memory cue colour table row.
pub fn memory_color_name(color: u8) -> Option<&'static str> {
    match color {
        1 => Some("pink"),
        2 => Some("red"),
        3 => Some("orange"),
        4 => Some("yellow"),
        5 => Some("green"),
        6 => Some("aqua"),
        7 => Some("blue"),
        8 => Some("purple"),
        _ => None,
    }
}

const LIST_KIND_POSITION: usize = 0x0c;
const CUE_COUNT_POSITION: usize = 0x12;
const FIRST_CUE_POSITION: usize = 0x18;
const EXTENDED_CUE_COUNT_POSITION: usize = 0x10;
const FIRST_EXTENDED_CUE_POSITION: usize = 0x14;
const ENTRY_ENVELOPE_SIZE: usize = 0x0c;
const CUE_ENTRY_MINIMUM_SIZE: usize = 0x28;
const EXTENDED_ENTRY_MINIMUM_SIZE: usize = 0x28;
const COMMENT_LENGTH_POSITION: usize = 0x28;
const COMMENT_POSITION: usize = 0x2c;

pub(crate) fn parse(reader: &SectionReader<'_>) -> Result<CueList, AnalysisError> {
    let kind = list_kind(reader)?;
    let count = reader.u16_at(CUE_COUNT_POSITION, "the cue count")?;
    let cues = parse_entries(
        reader,
        FIRST_CUE_POSITION,
        count,
        *b"PCPT",
        CUE_ENTRY_MINIMUM_SIZE,
        |entry| {
            let hot_cue = entry.u32_at(0x0c, "the hot cue number")?;
            let (time_ms, loop_end_ms) = position(entry, 0x1c, 0x20, 0x24)?;
            Ok(AnalysisCue {
                hot_cue: (hot_cue != 0).then_some(hot_cue),
                pad: hot_cue_pad(hot_cue),
                time_ms,
                loop_end_ms,
                loop_length: None,
                name: None,
                memory_color: None,
                hot_cue_color: None,
            })
        },
    )?;
    Ok(CueList {
        kind,
        extended: false,
        cues,
    })
}

pub(crate) fn parse_extended(reader: &SectionReader<'_>) -> Result<CueList, AnalysisError> {
    let kind = list_kind(reader)?;
    let count = reader.u16_at(EXTENDED_CUE_COUNT_POSITION, "the cue count")?;
    let cues = parse_entries(
        reader,
        FIRST_EXTENDED_CUE_POSITION,
        count,
        *b"PCP2",
        EXTENDED_ENTRY_MINIMUM_SIZE,
        parse_extended_entry,
    )?;
    Ok(CueList {
        kind,
        extended: true,
        cues,
    })
}

fn list_kind(reader: &SectionReader<'_>) -> Result<CueListKind, AnalysisError> {
    match reader.u32_at(LIST_KIND_POSITION, "the cue list type")? {
        0 => Ok(CueListKind::Memory),
        1 => Ok(CueListKind::Hot),
        other => Err(reader.malformed(format!(
            "the cue list type is {other}, but only 0 (memory cues) and 1 (hot cues) exist"
        ))),
    }
}

fn parse_entries(
    reader: &SectionReader<'_>,
    first_position: usize,
    count: u16,
    magic: [u8; 4],
    minimum_size: usize,
    parse_entry: impl Fn(&SectionReader<'_>) -> Result<AnalysisCue, AnalysisError>,
) -> Result<Vec<AnalysisCue>, AnalysisError> {
    let magic_text = String::from_utf8_lossy(&magic).into_owned();
    let mut cues = Vec::with_capacity(usize::from(count));
    let mut position = first_position;
    for number in 1..=count {
        let envelope = reader.entry(
            position,
            ENTRY_ENVELOPE_SIZE,
            &format!("cue {number} of {count}"),
        )?;
        let found = envelope.bytes_at(0, 4, "the entry code")?;
        if found != magic {
            return Err(reader.malformed(format!(
                "cue {number} of {count} at byte {position:#x} starts with {} instead of \
                 {magic_text}",
                FourCc([found[0], found[1], found[2], found[3]]),
            )));
        }
        let entry_length = envelope.u32_at(8, "the entry length")?;
        let entry_length = usize::try_from(entry_length).unwrap_or(usize::MAX);
        if entry_length < minimum_size {
            return Err(reader.malformed(format!(
                "cue {number} of {count} at byte {position:#x} says it is {entry_length} bytes \
                 long, shorter than the {minimum_size} bytes every {magic_text} entry has"
            )));
        }
        let entry = reader.entry(position, entry_length, &format!("cue {number} of {count}"))?;
        cues.push(parse_entry(&entry)?);
        position += entry_length;
    }
    Ok(cues)
}

fn position(
    entry: &SectionReader<'_>,
    type_position: usize,
    time_position: usize,
    loop_end_position: usize,
) -> Result<(u32, Option<u32>), AnalysisError> {
    let cue_type = entry.u8_at(type_position, "the cue type")?;
    let time_ms = entry.u32_at(time_position, "the cue time")?;
    let loop_end = entry.u32_at(loop_end_position, "the loop end")?;
    match cue_type {
        1 => Ok((time_ms, None)),
        2 => Ok((time_ms, Some(loop_end))),
        other => Err(entry.malformed(format!(
            "a cue has type {other}, but only 1 (cue point) and 2 (loop) exist"
        ))),
    }
}

fn parse_extended_entry(entry: &SectionReader<'_>) -> Result<AnalysisCue, AnalysisError> {
    let hot_cue = entry.u32_at(0x0c, "the hot cue number")?;
    let (time_ms, loop_end_ms) = position(entry, 0x10, 0x14, 0x18)?;
    let memory_color = entry.u8_at(0x1c, "the memory cue colour")?;
    let numerator = entry.u16_at(0x24, "the loop numerator")?;
    let denominator = entry.u16_at(0x26, "the loop denominator")?;

    let mut name = None;
    let mut hot_cue_color = None;
    if entry.len() >= COMMENT_POSITION {
        let comment_length = entry.u32_at(COMMENT_LENGTH_POSITION, "the name length")?;
        let comment_length = usize::try_from(comment_length).unwrap_or(usize::MAX);
        let comment = entry.bytes_at(COMMENT_POSITION, comment_length, "the name")?;
        name = utf16_be_string(entry, comment, "the cue name")?;
        let color_position = COMMENT_POSITION + comment_length;
        if hot_cue != 0 && entry.len() > color_position {
            let code = entry.u8_at(color_position, "the hot cue colour code")?;
            let led = if entry.len() >= color_position + 4 {
                let rgb = entry.bytes_at(color_position + 1, 3, "the hot cue LED colour")?;
                Some(Rgb {
                    red: rgb[0],
                    green: rgb[1],
                    blue: rgb[2],
                })
            } else {
                None
            };
            hot_cue_color = hot_cue_color_from(code, led);
        }
    }

    Ok(AnalysisCue {
        hot_cue: (hot_cue != 0).then_some(hot_cue),
        pad: hot_cue_pad(hot_cue),
        time_ms,
        loop_end_ms,
        loop_length: (numerator != 0 && denominator != 0).then_some(LoopLength {
            numerator,
            denominator,
        }),
        name,
        memory_color: (hot_cue == 0 && memory_color != 0).then_some(memory_color),
        hot_cue_color,
    })
}

fn hot_cue_color_from(code: u8, led: Option<Rgb>) -> Option<HotCueColor> {
    let led = led.filter(|rgb| rgb.red != 0 || rgb.green != 0 || rgb.blue != 0);
    if code == 0 && led.is_none() {
        return None;
    }
    Some(HotCueColor {
        code,
        display: palette_color(code),
        led,
    })
}

fn palette_color(code: u8) -> Option<Rgb> {
    PaletteColor::from_index(i64::from(code)).map(PaletteColor::rgb)
}
