use std::fmt;

use serde::{Serialize, Serializer};

use super::error::AnalysisError;
use super::reader::SectionReader;

/// The overall character rekordbox gave a track in phrase analysis, which decides the phrase
/// names it uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mood {
    /// Intro, Up, Down, Chorus and Outro.
    High,
    /// Intro, Verse 1 to 6, Bridge, Chorus and Outro.
    Mid,
    /// Intro, Verse 1 and 2, Bridge, Chorus and Outro.
    Low,
    /// A value this version of Dubplate does not know.
    Unknown(u16),
}

impl Mood {
    fn from_raw(raw: u16) -> Self {
        match raw {
            1 => Self::High,
            2 => Self::Mid,
            3 => Self::Low,
            other => Self::Unknown(other),
        }
    }
}

/// The lighting bank the DJ chose for a track in rekordbox's Lighting mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LightingBank {
    /// No choice made, which rekordbox treats as Cool.
    Default,
    /// Cool.
    Cool,
    /// Natural.
    Natural,
    /// Hot.
    Hot,
    /// Subtle.
    Subtle,
    /// Warm.
    Warm,
    /// Vivid.
    Vivid,
    /// Club 1.
    Club1,
    /// Club 2.
    Club2,
    /// A value this version of Dubplate does not know.
    Unknown(u8),
}

impl LightingBank {
    fn from_raw(raw: u8) -> Self {
        match raw {
            0 => Self::Default,
            1 => Self::Cool,
            2 => Self::Natural,
            3 => Self::Hot,
            4 => Self::Subtle,
            5 => Self::Warm,
            6 => Self::Vivid,
            7 => Self::Club1,
            8 => Self::Club2,
            other => Self::Unknown(other),
        }
    }
}

/// What a phrase is, named as rekordbox shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhraseKind {
    /// Intro, with variant 1 or 2 in high-mood tracks.
    Intro(Option<u8>),
    /// Up 1, Up 2 or Up 3, in high-mood tracks.
    Up(u8),
    /// Down, in high-mood tracks.
    Down,
    /// Verse 1 to 6, in mid- and low-mood tracks.
    Verse(u8),
    /// Bridge, in mid- and low-mood tracks.
    Bridge,
    /// Chorus, with variant 1 or 2 in high-mood tracks.
    Chorus(Option<u8>),
    /// Outro, with variant 1 or 2 in high-mood tracks.
    Outro(Option<u8>),
    /// A phrase number this version of Dubplate does not know for the track's mood.
    Unknown(u16),
}

impl fmt::Display for PhraseKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let with_variant =
            |formatter: &mut fmt::Formatter<'_>, name: &str, variant: &Option<u8>| match variant {
                Some(number) => write!(formatter, "{name} {number}"),
                None => formatter.write_str(name),
            };
        match self {
            Self::Intro(variant) => with_variant(formatter, "Intro", variant),
            Self::Up(number) => write!(formatter, "Up {number}"),
            Self::Down => formatter.write_str("Down"),
            Self::Verse(number) => write!(formatter, "Verse {number}"),
            Self::Bridge => formatter.write_str("Bridge"),
            Self::Chorus(variant) => with_variant(formatter, "Chorus", variant),
            Self::Outro(variant) => with_variant(formatter, "Outro", variant),
            Self::Unknown(raw) => write!(formatter, "Unknown phrase {raw}"),
        }
    }
}

impl Serialize for PhraseKind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// One phrase of a track's song structure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Phrase {
    /// The phrase's number, from 1.
    pub index: u16,
    /// What the phrase is.
    pub kind: PhraseKind,
    /// rekordbox's raw phrase number, read with the track's mood.
    pub raw_kind: u16,
    /// The beat the phrase starts on, counted from 1 along the beat grid.
    pub start_beat: u16,
    /// The beat the next phrase starts on, or where the last phrase ends.
    pub end_beat: u16,
    /// The beat a fill-in starts on, when the phrase ends with one.
    pub fill_start_beat: Option<u16>,
    /// Beats inside an Up 3 phrase where rekordbox's lighting changes.
    pub lighting_beats: Vec<u16>,
}

/// The phrases rekordbox found in a track, from the `PSSI` section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SongStructure {
    /// The mood that names the phrases.
    pub mood: Mood,
    /// The beat where the last phrase ends.
    pub end_beat: u16,
    /// The lighting bank.
    pub bank: LightingBank,
    /// Whether rekordbox stored the section XOR-masked, as rekordbox 6 does on export.
    pub was_masked: bool,
    /// The phrases in order.
    pub phrases: Vec<Phrase>,
}

const ENTRY_SIZE_POSITION: usize = 0x0c;
const ENTRY_COUNT_POSITION: usize = 0x10;
const MASKED_FROM: usize = 0x12;
const MOOD_POSITION: usize = 0x12;
const END_BEAT_POSITION: usize = 0x1a;
const BANK_POSITION: usize = 0x1e;
const FIRST_PHRASE_POSITION: usize = 0x20;
const PHRASE_SIZE: usize = 24;
const HIGHEST_PLAIN_MOOD: u16 = 20;
const MASK: [u8; 19] = [
    0xcb, 0xe1, 0xee, 0xfa, 0xe5, 0xee, 0xad, 0xee, 0xe9, 0xd2, 0xe9, 0xeb, 0xe1, 0xe9, 0xf3, 0xe8,
    0xe9, 0xf4, 0xe1,
];

pub(crate) fn parse(reader: &SectionReader<'_>) -> Result<SongStructure, AnalysisError> {
    let entry_size = reader.u32_at(ENTRY_SIZE_POSITION, "the phrase entry size")?;
    if usize::try_from(entry_size).ok() != Some(PHRASE_SIZE) {
        return Err(reader.malformed(format!(
            "phrase entries are {entry_size} bytes, but this reader knows only {PHRASE_SIZE}"
        )));
    }
    let count = reader.u16_at(ENTRY_COUNT_POSITION, "the phrase count")?;
    reader.expect_entries(
        FIRST_PHRASE_POSITION,
        u32::from(count),
        PHRASE_SIZE,
        "phrases",
    )?;

    let raw_mood = reader.u16_at(MOOD_POSITION, "the mood")?;
    let was_masked = raw_mood > HIGHEST_PLAIN_MOOD;
    let section = reader.bytes_at(0, reader.len(), "the section")?;
    let mut plain = section.to_vec();
    if was_masked {
        let count_byte = count.to_be_bytes()[1];
        for (index, byte) in plain[MASKED_FROM..].iter_mut().enumerate() {
            *byte ^= MASK[index % MASK.len()].wrapping_add(count_byte);
        }
    }
    let plain_reader = SectionReader::new(reader.fourcc(), reader.section_offset(), &plain);

    let mood = Mood::from_raw(plain_reader.u16_at(MOOD_POSITION, "the mood")?);
    let end_beat = plain_reader.u16_at(END_BEAT_POSITION, "the end beat")?;
    let bank = LightingBank::from_raw(plain_reader.u8_at(BANK_POSITION, "the lighting bank")?);

    let mut phrases: Vec<Phrase> = Vec::with_capacity(usize::from(count));
    for number in 0..usize::from(count) {
        let entry = plain_reader.entry(
            FIRST_PHRASE_POSITION + number * PHRASE_SIZE,
            PHRASE_SIZE,
            "a phrase",
        )?;
        phrases.push(parse_phrase(&entry, mood)?);
    }
    for index in 0..phrases.len() {
        phrases[index].end_beat = phrases
            .get(index + 1)
            .map_or(end_beat, |next| next.start_beat);
    }

    Ok(SongStructure {
        mood,
        end_beat,
        bank,
        was_masked,
        phrases,
    })
}

fn parse_phrase(entry: &SectionReader<'_>, mood: Mood) -> Result<Phrase, AnalysisError> {
    let raw_kind = entry.u16_at(0x04, "the phrase kind")?;
    let first_flag = entry.u8_at(0x07, "the first variant flag")?;
    let second_flag = entry.u8_at(0x09, "the second variant flag")?;
    let extra_beats_flag = entry.u8_at(0x0b, "the lighting beat count flag")?;
    let third_flag = entry.u8_at(0x13, "the third variant flag")?;
    let fill = entry.u8_at(0x15, "the fill-in flag")?;
    let kind = phrase_kind(mood, raw_kind, first_flag, second_flag, third_flag);
    let lighting_beats = if kind == PhraseKind::Up(3) {
        let beat_count = if extra_beats_flag == 0 { 1 } else { 3 };
        (0..beat_count)
            .map(|offset| entry.u16_at(0x0c + offset * 2, "a lighting beat"))
            .collect::<Result<Vec<_>, _>>()?
    } else {
        Vec::new()
    };
    Ok(Phrase {
        index: entry.u16_at(0x00, "the phrase number")?,
        kind,
        raw_kind,
        start_beat: entry.u16_at(0x02, "the phrase start beat")?,
        end_beat: 0,
        fill_start_beat: if fill == 0 {
            None
        } else {
            Some(entry.u16_at(0x16, "the fill-in beat")?)
        },
        lighting_beats,
    })
}

fn phrase_kind(
    mood: Mood,
    raw_kind: u16,
    first_flag: u8,
    second_flag: u8,
    third_flag: u8,
) -> PhraseKind {
    let one_or_two = if first_flag == 1 { 1 } else { 2 };
    match mood {
        Mood::High => match raw_kind {
            1 => PhraseKind::Intro(Some(one_or_two)),
            2 => match (second_flag, third_flag) {
                (0, 0) => PhraseKind::Up(1),
                (0, _) => PhraseKind::Up(2),
                _ => PhraseKind::Up(3),
            },
            3 => PhraseKind::Down,
            5 => PhraseKind::Chorus(Some(one_or_two)),
            6 => PhraseKind::Outro(Some(one_or_two)),
            other => PhraseKind::Unknown(other),
        },
        Mood::Low => match raw_kind {
            1 => PhraseKind::Intro(None),
            2..=4 => PhraseKind::Verse(1),
            5..=7 => PhraseKind::Verse(2),
            8 => PhraseKind::Bridge,
            9 => PhraseKind::Chorus(None),
            10 => PhraseKind::Outro(None),
            other => PhraseKind::Unknown(other),
        },
        Mood::Mid | Mood::Unknown(_) => match raw_kind {
            1 => PhraseKind::Intro(None),
            verse @ 2..=7 => PhraseKind::Verse(u8::try_from(verse - 1).unwrap_or(0)),
            8 => PhraseKind::Bridge,
            9 => PhraseKind::Chorus(None),
            10 => PhraseKind::Outro(None),
            other => PhraseKind::Unknown(other),
        },
    }
}
