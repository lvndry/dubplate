//! Reads analysis files written byte by byte from Deep Symmetry's description of the format.

mod common;

use std::path::Path;

use common::{SALTWATER_ANALYSIS, open_fixture};
use dubplate_rekordbox::Error;
use dubplate_rekordbox::analysis::{
    AnalysisError, AnalysisFile, AnalysisFileKind, Beat, CueListKind, FourCc, LightingBank,
    LoopLength, Mood, PhraseKind, Rgb, Section, SkippedSection, ThreeBandColumn, TrackAnalysis,
};

fn u16_bytes(value: u16) -> [u8; 2] {
    value.to_be_bytes()
}

fn u32_bytes(value: u32) -> [u8; 4] {
    value.to_be_bytes()
}

fn utf16_with_nul(text: &str) -> Vec<u8> {
    text.encode_utf16()
        .chain([0])
        .flat_map(u16::to_be_bytes)
        .collect()
}

/// A tagged section: its code, header length, total length, then everything after byte 0x0c.
fn section(fourcc: &[u8; 4], header_length: u32, body: &[u8]) -> Vec<u8> {
    let mut bytes = fourcc.to_vec();
    bytes.extend(u32_bytes(header_length));
    bytes.extend(u32_bytes(12 + body.len() as u32));
    bytes.extend(body);
    bytes
}

/// A whole file: `PMAI`, a 0x1c-byte header with the file length, then the sections.
fn analysis_file(sections: &[Vec<u8>]) -> Vec<u8> {
    let content: Vec<u8> = sections.concat();
    let mut bytes = b"PMAI".to_vec();
    bytes.extend(u32_bytes(0x1c));
    bytes.extend(u32_bytes(0x1c + content.len() as u32));
    bytes.extend([0, 0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
    bytes.extend(content);
    bytes
}

fn beat_grid(beats: &[(u16, u16, u32)]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend(u32_bytes(0));
    body.extend(u32_bytes(0x0008_0000));
    body.extend(u32_bytes(beats.len() as u32));
    for (beat_in_bar, tempo, time) in beats {
        body.extend(u16_bytes(*beat_in_bar));
        body.extend(u16_bytes(*tempo));
        body.extend(u32_bytes(*time));
    }
    section(b"PQTZ", 0x18, &body)
}

fn basic_cue(hot_cue: u32, cue_type: u8, time: u32, loop_time: u32) -> Vec<u8> {
    let mut entry = b"PCPT".to_vec();
    entry.extend(u32_bytes(0x1c));
    entry.extend(u32_bytes(0x38));
    entry.extend(u32_bytes(hot_cue));
    entry.extend(u32_bytes(0));
    entry.extend(u32_bytes(0x0001_0000));
    entry.extend(u16_bytes(0xffff));
    entry.extend(u16_bytes(0xffff));
    entry.push(cue_type);
    entry.extend([0x00, 0x03, 0xe8]);
    entry.extend(u32_bytes(time));
    entry.extend(u32_bytes(loop_time));
    entry.extend([0; 16]);
    assert_eq!(entry.len(), 0x38);
    entry
}

fn basic_cue_list(list_type: u32, entries: &[Vec<u8>]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend(u32_bytes(list_type));
    body.extend(u16_bytes(0));
    body.extend(u16_bytes(entries.len() as u16));
    body.extend(u32_bytes(0xffff_ffff));
    body.extend(entries.concat());
    section(b"PCOB", 0x18, &body)
}

struct ExtendedCue<'name> {
    hot_cue: u32,
    cue_type: u8,
    time: u32,
    loop_time: u32,
    memory_color: u8,
    loop_length: (u16, u16),
    name: Option<&'name str>,
    hot_cue_color: [u8; 4],
}

impl ExtendedCue<'_> {
    fn bytes(&self) -> Vec<u8> {
        let comment = self.name.map(utf16_with_nul).unwrap_or_default();
        let mut after_envelope = Vec::new();
        after_envelope.extend(u32_bytes(self.hot_cue));
        after_envelope.push(self.cue_type);
        after_envelope.extend([0x00, 0x03, 0xe8]);
        after_envelope.extend(u32_bytes(self.time));
        after_envelope.extend(u32_bytes(self.loop_time));
        after_envelope.push(self.memory_color);
        after_envelope.extend([1, 0, 0, 0, 0, 0, 0]);
        after_envelope.extend(u16_bytes(self.loop_length.0));
        after_envelope.extend(u16_bytes(self.loop_length.1));
        after_envelope.extend(u32_bytes(comment.len() as u32));
        after_envelope.extend(&comment);
        after_envelope.extend(self.hot_cue_color);
        after_envelope.extend([0; 12]);
        let mut entry = b"PCP2".to_vec();
        entry.extend(u32_bytes(0x10));
        entry.extend(u32_bytes(12 + after_envelope.len() as u32));
        entry.extend(after_envelope);
        entry
    }
}

fn extended_cue_list(list_type: u32, entries: &[Vec<u8>]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend(u32_bytes(list_type));
    body.extend(u16_bytes(entries.len() as u16));
    body.extend(u16_bytes(0));
    body.extend(entries.concat());
    section(b"PCO2", 0x14, &body)
}

fn path_section(path: &str) -> Vec<u8> {
    let path = utf16_with_nul(path);
    let mut body = u32_bytes(path.len() as u32).to_vec();
    body.extend(path);
    section(b"PPTH", 0x10, &body)
}

fn preview(fourcc: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut body = u32_bytes(data.len() as u32).to_vec();
    body.extend(u32_bytes(0x0010_0000));
    body.extend(data);
    section(fourcc, 0x14, &body)
}

fn sized_entries(fourcc: &[u8; 4], entry_size: u32, unknown: Option<u32>, data: &[u8]) -> Vec<u8> {
    let mut body = u32_bytes(entry_size).to_vec();
    body.extend(u32_bytes(data.len() as u32 / entry_size));
    let header_length = match unknown {
        Some(value) => {
            body.extend(u32_bytes(value));
            0x18
        }
        None => 0x14,
    };
    body.extend(data);
    section(fourcc, header_length, &body)
}

struct PhraseEntry {
    index: u16,
    beat: u16,
    kind: u16,
    flags: (u8, u8, u8),
    extra_beats: (u8, [u16; 3]),
    fill: Option<u16>,
}

impl PhraseEntry {
    fn bytes(&self) -> Vec<u8> {
        let mut entry = Vec::new();
        entry.extend(u16_bytes(self.index));
        entry.extend(u16_bytes(self.beat));
        entry.extend(u16_bytes(self.kind));
        entry.extend([0, self.flags.0, 0, self.flags.1, 0, self.extra_beats.0]);
        for beat in self.extra_beats.1 {
            entry.extend(u16_bytes(beat));
        }
        entry.extend([0, self.flags.2, 0, u8::from(self.fill.is_some())]);
        entry.extend(u16_bytes(self.fill.unwrap_or(0)));
        assert_eq!(entry.len(), 24);
        entry
    }
}

fn song_structure(mood: u16, end_beat: u16, bank: u8, phrases: &[PhraseEntry]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend(u32_bytes(24));
    body.extend(u16_bytes(phrases.len() as u16));
    body.extend(u16_bytes(mood));
    body.extend([0; 6]);
    body.extend(u16_bytes(end_beat));
    body.extend([0, 0, bank, 0]);
    for phrase in phrases {
        body.extend(phrase.bytes());
    }
    section(b"PSSI", 0x20, &body)
}

/// Masks a `PSSI` section the way rekordbox 6 exports it: every byte from 0x12 on is XORed with
/// the base pattern plus the phrase count.
fn mask_song_structure(section: &mut [u8]) {
    const PATTERN: [u8; 19] = [
        0xcb, 0xe1, 0xee, 0xfa, 0xe5, 0xee, 0xad, 0xee, 0xe9, 0xd2, 0xe9, 0xeb, 0xe1, 0xe9, 0xf3,
        0xe8, 0xe9, 0xf4, 0xe1,
    ];
    let count = u16::from_be_bytes([section[0x10], section[0x11]]);
    for (index, byte) in section[0x12..].iter_mut().enumerate() {
        *byte ^= PATTERN[index % PATTERN.len()].wrapping_add(count as u8);
    }
}

fn three_phrases() -> Vec<PhraseEntry> {
    vec![
        PhraseEntry {
            index: 1,
            beat: 1,
            kind: 1,
            flags: (1, 0, 0),
            extra_beats: (0, [0; 3]),
            fill: None,
        },
        PhraseEntry {
            index: 2,
            beat: 33,
            kind: 2,
            flags: (0, 1, 0),
            extra_beats: (1, [37, 41, 45]),
            fill: Some(61),
        },
        PhraseEntry {
            index: 3,
            beat: 65,
            kind: 5,
            flags: (0, 0, 0),
            extra_beats: (0, [0; 3]),
            fill: None,
        },
    ]
}

fn only_section(bytes: &[u8]) -> Section {
    let mut file = AnalysisFile::parse(bytes).expect("parse");
    assert_eq!(file.sections.len(), 1);
    file.sections.remove(0)
}

fn parse_error(bytes: &[u8]) -> AnalysisError {
    AnalysisFile::parse(bytes).expect_err("the file should be rejected")
}

#[test]
fn reads_a_beat_grid() {
    let file = analysis_file(&[beat_grid(&[
        (1, 12200, 41),
        (2, 12200, 533),
        (3, 12200, 1025),
        (4, 12250, 1516),
    ])]);

    let Section::BeatGrid(grid) = only_section(&file) else {
        panic!("expected a beat grid");
    };

    assert_eq!(
        grid.beats[0],
        Beat {
            beat_in_bar: 1,
            tempo_hundredths: 12200,
            time_ms: 41
        }
    );
    assert_eq!(grid.beats[3].time_ms, 1516);
    assert_eq!(grid.first_downbeat().unwrap().time_ms, 41);
    assert_eq!(grid.tempo_range_hundredths(), Some((12200, 12250)));
    assert_eq!(grid.beats[0].bpm(), 122.0);
}

#[test]
fn places_moments_on_bars_and_beats() {
    let file = analysis_file(&[beat_grid(&[
        (3, 12000, 0),
        (4, 12000, 500),
        (1, 12000, 1000),
        (2, 12000, 1500),
        (3, 12000, 2000),
        (4, 12000, 2500),
        (1, 12000, 3000),
    ])]);
    let Section::BeatGrid(grid) = only_section(&file) else {
        panic!("expected a beat grid");
    };

    assert_eq!(grid.first_downbeat().unwrap().time_ms, 1000);
    assert_eq!(grid.bar_of_beat(1), Some(0));
    assert_eq!(grid.bar_of_beat(3), Some(1));
    assert_eq!(grid.bar_of_beat(7), Some(2));
    assert_eq!(grid.bar_of_beat(8), None);
    assert_eq!(grid.beat(0), None);

    let position = grid.position_of(2990).unwrap();
    assert_eq!((position.beat_number, position.bar), (7, 2));
    assert_eq!((position.beat_in_bar, position.offset_ms), (1, -10));
    let position = grid.position_of(1704).unwrap();
    assert_eq!((position.beat_number, position.offset_ms), (4, 204));
    assert_eq!(grid.position_of(9000).unwrap().offset_ms, 6000);
}

#[test]
fn reads_basic_hot_cues_memory_cues_and_loops() {
    let file = analysis_file(&[
        basic_cue_list(
            1,
            &[
                basic_cue(1, 1, 41, 0xffff_ffff),
                basic_cue(8, 2, 9000, 13000),
            ],
        ),
        basic_cue_list(0, &[basic_cue(0, 1, 200_697, 0xffff_ffff)]),
    ]);
    let parsed = AnalysisFile::parse(&file).unwrap();

    let Section::CueList(hot) = &parsed.sections[0] else {
        panic!("expected a cue list");
    };
    assert_eq!(hot.kind, CueListKind::Hot);
    assert!(!hot.extended);
    assert_eq!(hot.cues[0].pad, Some('A'));
    assert_eq!(hot.cues[0].time_ms, 41);
    assert_eq!(hot.cues[0].loop_end_ms, None);
    assert_eq!(hot.cues[1].pad, Some('H'));
    assert_eq!(hot.cues[1].loop_end_ms, Some(13000));

    let Section::CueList(memory) = &parsed.sections[1] else {
        panic!("expected a cue list");
    };
    assert_eq!(memory.kind, CueListKind::Memory);
    assert_eq!(memory.cues[0].hot_cue, None);
    assert_eq!(memory.cues[0].pad, None);
    assert_eq!(memory.cues[0].time_ms, 200_697);
}

#[test]
fn reads_extended_cues_with_names_and_colours() {
    let drop = ExtendedCue {
        hot_cue: 4,
        cue_type: 1,
        time: 104_303,
        loop_time: 0xffff_ffff,
        memory_color: 0,
        loop_length: (0, 0),
        name: Some("Drop"),
        hot_cue_color: [0x2a, 0xe6, 0x28, 0x28],
    };
    let roll = ExtendedCue {
        hot_cue: 2,
        cue_type: 2,
        time: 188_893,
        loop_time: 190_860,
        memory_color: 0,
        loop_length: (4, 1),
        name: None,
        hot_cue_color: [0, 0, 0, 0],
    };
    let breakdown = ExtendedCue {
        hot_cue: 0,
        cue_type: 1,
        time: 200_697,
        loop_time: 0xffff_ffff,
        memory_color: 6,
        loop_length: (0, 0),
        name: Some("Break ☀"),
        hot_cue_color: [0, 0, 0, 0],
    };
    let file = analysis_file(&[
        extended_cue_list(1, &[drop.bytes(), roll.bytes()]),
        extended_cue_list(0, &[breakdown.bytes()]),
    ]);
    let parsed = AnalysisFile::parse(&file).unwrap();

    let Section::CueList(hot) = &parsed.sections[0] else {
        panic!("expected a cue list");
    };
    assert!(hot.extended);
    let drop = &hot.cues[0];
    assert_eq!(drop.pad, Some('D'));
    assert_eq!(drop.time_ms, 104_303);
    assert_eq!(drop.name.as_deref(), Some("Drop"));
    let color = drop.hot_cue_color.unwrap();
    assert_eq!(color.code, 0x2a);
    assert_eq!(color.display.unwrap().to_string(), "#e62828");
    assert_eq!(
        color.led,
        Some(Rgb {
            red: 0xe6,
            green: 0x28,
            blue: 0x28
        })
    );
    let roll = &hot.cues[1];
    assert_eq!(roll.pad, Some('B'));
    assert_eq!(roll.loop_end_ms, Some(190_860));
    assert_eq!(
        roll.loop_length,
        Some(LoopLength {
            numerator: 4,
            denominator: 1
        })
    );
    assert_eq!(roll.hot_cue_color, None);

    let Section::CueList(memory) = &parsed.sections[1] else {
        panic!("expected a cue list");
    };
    assert_eq!(memory.cues[0].name.as_deref(), Some("Break ☀"));
    assert_eq!(memory.cues[0].memory_color, Some(6));
    assert_eq!(memory.cues[0].hot_cue_color, None);
}

#[test]
fn reads_extended_cues_that_end_before_the_name_or_the_colour() {
    let full = ExtendedCue {
        hot_cue: 3,
        cue_type: 1,
        time: 236_106,
        loop_time: 0xffff_ffff,
        memory_color: 0,
        loop_length: (0, 0),
        name: Some("C"),
        hot_cue_color: [0x01, 0x30, 0x5a, 0xff],
    }
    .bytes();
    let without_name = shortened_entry(&full, 0x28);
    let without_colour = shortened_entry(&full, 0x2c + 4);
    let file = analysis_file(&[extended_cue_list(1, &[without_name, without_colour])]);

    let Section::CueList(list) = only_section(&file) else {
        panic!("expected a cue list");
    };

    assert_eq!(list.cues[0].time_ms, 236_106);
    assert_eq!(list.cues[0].name, None);
    assert_eq!(list.cues[0].hot_cue_color, None);
    assert_eq!(list.cues[1].name.as_deref(), Some("C"));
    assert_eq!(list.cues[1].hot_cue_color, None);
}

fn shortened_entry(entry: &[u8], length: usize) -> Vec<u8> {
    let mut shortened = entry[..length].to_vec();
    shortened[8..12].copy_from_slice(&u32_bytes(length as u32));
    shortened
}

#[test]
fn reads_the_audio_path() {
    let file = analysis_file(&[path_section("?/Artist - Title.m4a")]);

    assert_eq!(
        only_section(&file),
        Section::AudioPath(Some("?/Artist - Title.m4a".to_owned()))
    );
}

#[test]
fn reads_every_waveform() {
    let file = analysis_file(&[
        preview(b"PWAV", &[0b1110_0101, 0x1f]),
        preview(b"PWV2", &[0x0b, 0x0d, 0x0c]),
        sized_entries(b"PWV3", 1, Some(0x0096_0000), &[0xe0, 0x68, 0x56]),
        sized_entries(
            b"PWV4",
            6,
            Some(0),
            &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
        ),
        sized_entries(b"PWV5", 2, Some(0x0096_0305), &[0b1010_1100, 0b1111_1100]),
        sized_entries(b"PWV6", 3, None, &[10, 3, 8, 21, 6, 10]),
        sized_entries(b"PWV7", 3, Some(0x0096_0000), &[3, 2, 0]),
    ]);
    let analysis = TrackAnalysis::from_files(vec![(
        AnalysisFileKind::Ext,
        "ANLZ0000.EXT".into(),
        AnalysisFile::parse(&file).unwrap(),
    )]);
    let waveforms = analysis.waveforms;

    let preview = waveforms.preview.unwrap();
    assert_eq!((preview[0].height(), preview[0].whiteness()), (5, 7));
    assert_eq!(preview[1].height(), 31);
    assert_eq!(waveforms.tiny_preview.unwrap()[1].height(), 13);
    assert_eq!(waveforms.detail.unwrap().len(), 3);
    let color_preview = waveforms.color_preview.unwrap();
    assert_eq!(color_preview.len(), 2);
    assert_eq!(color_preview[1].0, [7, 8, 9, 10, 11, 12]);
    let color_detail = waveforms.color_detail.unwrap()[0];
    assert_eq!(
        (
            color_detail.red(),
            color_detail.green(),
            color_detail.blue(),
            color_detail.height()
        ),
        (5, 3, 1, 31)
    );
    assert_eq!(
        waveforms.three_band_preview.unwrap()[1],
        ThreeBandColumn {
            low: 21,
            mid: 6,
            high: 10
        }
    );
    assert_eq!(waveforms.three_band_detail.unwrap().len(), 1);
}

#[test]
fn reads_phrases() {
    let file = analysis_file(&[song_structure(1, 97, 3, &three_phrases())]);

    let Section::SongStructure(structure) = only_section(&file) else {
        panic!("expected phrases");
    };

    assert_eq!(structure.mood, Mood::High);
    assert_eq!(structure.bank, LightingBank::Hot);
    assert_eq!(structure.end_beat, 97);
    assert!(!structure.was_masked);
    let kinds: Vec<String> = structure
        .phrases
        .iter()
        .map(|phrase| phrase.kind.to_string())
        .collect();
    assert_eq!(kinds, ["Intro 1", "Up 3", "Chorus 2"]);
    let up = &structure.phrases[1];
    assert_eq!((up.start_beat, up.end_beat), (33, 65));
    assert_eq!(up.fill_start_beat, Some(61));
    assert_eq!(up.lighting_beats, [37, 41, 45]);
    assert_eq!(structure.phrases[2].end_beat, 97);
}

#[test]
fn unmasks_phrases_from_rekordbox_6() {
    let plain = song_structure(1, 97, 3, &three_phrases());
    let mut masked = plain.clone();
    mask_song_structure(&mut masked);
    assert_ne!(masked, plain);

    let Section::SongStructure(from_plain) = only_section(&analysis_file(&[plain])) else {
        panic!("expected phrases");
    };
    let Section::SongStructure(from_masked) = only_section(&analysis_file(&[masked])) else {
        panic!("expected phrases");
    };

    assert!(from_masked.was_masked);
    assert_eq!(from_masked.phrases, from_plain.phrases);
    assert_eq!(from_masked.mood, Mood::High);
    assert_eq!(from_masked.bank, LightingBank::Hot);
}

#[test]
fn names_phrases_for_mid_and_low_moods() {
    let phrase = |index: u16, kind: u16| PhraseEntry {
        index,
        beat: index * 32,
        kind,
        flags: (0, 0, 0),
        extra_beats: (0, [0; 3]),
        fill: None,
    };
    let mid = song_structure(2, 200, 0, &[phrase(1, 1), phrase(2, 7), phrase(3, 8)]);
    let low = song_structure(3, 200, 0, &[phrase(1, 4), phrase(2, 6), phrase(3, 10)]);

    let Section::SongStructure(mid) = only_section(&analysis_file(&[mid])) else {
        panic!("expected phrases");
    };
    let Section::SongStructure(low) = only_section(&analysis_file(&[low])) else {
        panic!("expected phrases");
    };

    let mid_kinds: Vec<_> = mid.phrases.iter().map(|phrase| phrase.kind).collect();
    assert_eq!(
        mid_kinds,
        [
            PhraseKind::Intro(None),
            PhraseKind::Verse(6),
            PhraseKind::Bridge
        ]
    );
    let low_kinds: Vec<_> = low.phrases.iter().map(|phrase| phrase.kind).collect();
    assert_eq!(
        low_kinds,
        [
            PhraseKind::Verse(1),
            PhraseKind::Verse(2),
            PhraseKind::Outro(None)
        ]
    );
    assert_eq!(mid.bank, LightingBank::Default);
}

#[test]
fn skips_and_reports_unknown_sections() {
    let unknown = section(b"PQT2", 0x38, &[7; 60]);
    let file = analysis_file(&[unknown, beat_grid(&[(1, 12800, 0)])]);

    let parsed = AnalysisFile::parse(&file).unwrap();

    assert_eq!(
        parsed.sections[0],
        Section::Skipped(SkippedSection {
            fourcc: FourCc(*b"PQT2"),
            offset: 0x1c,
            length: 72,
        })
    );
    assert!(matches!(parsed.sections[1], Section::BeatGrid(_)));
}

#[test]
fn prefers_extended_cues_and_orders_cues_by_time() {
    let extended = |hot_cue: u32, time: u32| {
        ExtendedCue {
            hot_cue,
            cue_type: 1,
            time,
            loop_time: 0xffff_ffff,
            memory_color: 0,
            loop_length: (0, 0),
            name: None,
            hot_cue_color: [0; 4],
        }
        .bytes()
    };
    let dat = analysis_file(&[
        beat_grid(&[(1, 12200, 41)]),
        basic_cue_list(1, &[basic_cue(1, 1, 41, 0xffff_ffff)]),
    ]);
    let ext = analysis_file(&[
        basic_cue_list(1, &[basic_cue(1, 1, 41, 0xffff_ffff)]),
        extended_cue_list(
            1,
            &[extended(3, 236_106), extended(1, 41), extended(8, 104_303)],
        ),
        extended_cue_list(0, &[]),
    ]);

    let analysis = TrackAnalysis::from_files(vec![
        (
            AnalysisFileKind::Dat,
            "ANLZ0000.DAT".into(),
            AnalysisFile::parse(&dat).unwrap(),
        ),
        (
            AnalysisFileKind::Ext,
            "ANLZ0000.EXT".into(),
            AnalysisFile::parse(&ext).unwrap(),
        ),
    ]);

    let pads: Vec<_> = analysis.cues.iter().map(|cue| cue.pad).collect();
    assert_eq!(pads, [Some('A'), Some('H'), Some('C')]);
    assert!(analysis.beat_grid.is_some());
}

#[test]
fn rejects_a_file_that_is_not_an_analysis_file() {
    let error = parse_error(b"RIFF\0\0\0\x1c\0\0\0\x1c");

    assert_eq!(
        error,
        AnalysisError::NotAnalysisFile {
            found: FourCc(*b"RIFF")
        }
    );
    assert_eq!(
        error.to_string(),
        "not a rekordbox analysis file: it starts with RIFF instead of PMAI"
    );
}

#[test]
fn rejects_a_truncated_file() {
    let mut file = analysis_file(&[beat_grid(&[(1, 12200, 41), (2, 12200, 533)])]);
    let full_length = file.len();
    file.truncate(full_length - 3);

    let error = parse_error(&file);

    assert_eq!(
        error,
        AnalysisError::FileLengthMismatch {
            declared: full_length as u32,
            actual: full_length - 3,
        }
    );
}

#[test]
fn rejects_a_section_longer_than_the_file() {
    let mut grid = beat_grid(&[(1, 12200, 41), (2, 12200, 533)]);
    let real_length = grid.len() as u32;
    grid[8..12].copy_from_slice(&u32_bytes(real_length + 8));
    let file = analysis_file(&[grid]);

    let error = parse_error(&file);

    assert_eq!(
        error,
        AnalysisError::SectionTruncated {
            fourcc: FourCc(*b"PQTZ"),
            offset: 0x1c,
            declared: real_length + 8,
            available: real_length as usize,
        }
    );
    assert_eq!(
        error.to_string(),
        "the PQTZ section at byte 0x1c says it is 48 bytes long but only 40 bytes are left in \
         the file"
    );
}

#[test]
fn rejects_a_section_shorter_than_its_header() {
    let mut grid = beat_grid(&[]);
    grid[8..12].copy_from_slice(&u32_bytes(4));
    let file = analysis_file(&[grid]);

    let error = parse_error(&file);

    assert!(
        error
            .to_string()
            .contains("says it is 4 bytes long, shorter than the 12-byte section header"),
        "{error}"
    );
}

#[test]
fn rejects_a_section_header_longer_than_the_section() {
    let mut grid = beat_grid(&[]);
    grid[4..8].copy_from_slice(&u32_bytes(0x40));
    let file = analysis_file(&[grid]);

    let error = parse_error(&file);

    assert_eq!(
        error.to_string(),
        "the PQTZ section at byte 0x1c is malformed: its header length 64 does not fit between \
         12 bytes and its length of 24 bytes"
    );
}

#[test]
fn rejects_a_beat_grid_whose_count_does_not_match_its_length() {
    let mut grid = beat_grid(&[(1, 12200, 41), (2, 12200, 533)]);
    grid[0x14..0x18].copy_from_slice(&u32_bytes(3));
    let file = analysis_file(&[grid]);

    let error = parse_error(&file);

    assert_eq!(
        error.to_string(),
        "the PQTZ section at byte 0x1c is malformed: 3 beats of 8 bytes after byte 0x18 need a \
         section of 48 bytes but it is 40 bytes"
    );
}

#[test]
fn rejects_a_cue_entry_with_the_wrong_code() {
    let mut entry = basic_cue(1, 1, 41, 0xffff_ffff);
    entry[0..4].copy_from_slice(b"PCP2");
    let file = analysis_file(&[basic_cue_list(1, &[entry])]);

    let error = parse_error(&file);

    assert_eq!(
        error.to_string(),
        "the PCOB section at byte 0x1c is malformed: cue 1 of 1 at byte 0x18 starts with PCP2 \
         instead of PCPT"
    );
}

#[test]
fn rejects_a_cue_entry_longer_than_its_section() {
    let mut entry = ExtendedCue {
        hot_cue: 1,
        cue_type: 1,
        time: 41,
        loop_time: 0xffff_ffff,
        memory_color: 0,
        loop_length: (0, 0),
        name: Some("Intro"),
        hot_cue_color: [0; 4],
    }
    .bytes();
    entry[8..12].copy_from_slice(&u32_bytes(0x400));
    let file = analysis_file(&[extended_cue_list(1, &[entry])]);

    let error = parse_error(&file);

    assert!(
        matches!(&error, AnalysisError::Malformed { fourcc, .. } if *fourcc == FourCc(*b"PCO2")),
        "{error}"
    );
    assert!(
        error
            .to_string()
            .contains("cue 1 of 1 at byte 0x14 needs 1024 bytes"),
        "{error}"
    );
}

#[test]
fn rejects_a_cue_name_longer_than_its_entry() {
    let mut entry = ExtendedCue {
        hot_cue: 1,
        cue_type: 1,
        time: 41,
        loop_time: 0xffff_ffff,
        memory_color: 0,
        loop_length: (0, 0),
        name: Some("Intro"),
        hot_cue_color: [0; 4],
    }
    .bytes();
    entry[0x28..0x2c].copy_from_slice(&u32_bytes(0x200));
    let file = analysis_file(&[extended_cue_list(1, &[entry])]);

    let error = parse_error(&file);

    assert!(
        error
            .to_string()
            .contains("the name at byte 0x2c of the entry at byte 0x14 needs 512 bytes"),
        "{error}"
    );
}

#[test]
fn rejects_an_unknown_cue_type() {
    let file = analysis_file(&[basic_cue_list(1, &[basic_cue(1, 7, 41, 0)])]);

    let error = parse_error(&file);

    assert!(
        error
            .to_string()
            .contains("a cue has type 7, but only 1 (cue point) and 2 (loop) exist"),
        "{error}"
    );
}

#[test]
fn rejects_a_waveform_with_an_unexpected_entry_size() {
    let file = analysis_file(&[sized_entries(b"PWV5", 3, Some(0), &[0; 6])]);

    let error = parse_error(&file);

    assert_eq!(
        error.to_string(),
        "the PWV5 section at byte 0x1c is malformed: waveform entries are 3 bytes, but this \
         kind has 2"
    );
}

#[test]
fn rejects_a_waveform_cut_short() {
    let mut waveform = sized_entries(b"PWV7", 3, Some(0x0096_0000), &[1, 2, 3, 4, 5, 6]);
    waveform[0x10..0x14].copy_from_slice(&u32_bytes(5));
    let file = analysis_file(&[waveform]);

    let error = parse_error(&file);

    assert!(
        error
            .to_string()
            .contains("5 waveform columns of 3 bytes after byte 0x18 need a section of 39 bytes"),
        "{error}"
    );
}

#[test]
fn rejects_phrases_with_an_unexpected_entry_size() {
    let mut structure = song_structure(1, 97, 0, &three_phrases());
    structure[0x0c..0x10].copy_from_slice(&u32_bytes(20));
    let file = analysis_file(&[structure]);

    let error = parse_error(&file);

    assert!(
        error
            .to_string()
            .contains("phrase entries are 20 bytes, but this reader knows only 24"),
        "{error}"
    );
}

#[test]
fn rejects_stray_bytes_after_the_last_section() {
    let mut file = analysis_file(&[beat_grid(&[(1, 12200, 41)])]);
    file.extend([0, 0, 0, 0, 0]);
    let length = file.len() as u32;
    file[8..12].copy_from_slice(&u32_bytes(length));

    let error = parse_error(&file);

    assert!(
        error
            .to_string()
            .contains("5 bytes are left at byte 0x3c, too few for a section header"),
        "{error}"
    );
}

fn write_analysis(share: &Path, kind: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = share
        .join(SALTWATER_ANALYSIS.trim_start_matches('/'))
        .with_extension(kind);
    std::fs::create_dir_all(path.parent().expect("analysis directory"))
        .expect("create the analysis directory");
    std::fs::write(&path, bytes).expect("write the analysis file");
    path
}

#[test]
fn reads_a_tracks_analysis_from_the_share_directory_without_changing_it() {
    let (directory, library) = open_fixture();
    let share = directory.path().join("share");
    assert_eq!(library.share_directory(), share);
    let dat = write_analysis(
        &share,
        "DAT",
        &analysis_file(&[
            path_section("?/saltwater.flac"),
            beat_grid(&[(1, 12400, 120), (2, 12400, 604)]),
            section(b"PVBR", 0x10, &[0; 16]),
        ]),
    );
    let ext = write_analysis(
        &share,
        "EXT",
        &analysis_file(&[song_structure(1, 3, 0, &three_phrases()[..1])]),
    );
    let mut permissions = std::fs::metadata(&dat).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&dat, permissions.clone()).unwrap();
    std::fs::set_permissions(&ext, permissions).unwrap();
    let dat_before = std::fs::read(&dat).unwrap();
    let ext_before = std::fs::read(&ext).unwrap();

    let tracks = library.tracks().unwrap();
    let saltwater = tracks.iter().find(|track| track.id == "t1").unwrap();
    let analysis = library.analysis(saltwater).unwrap();

    assert_eq!(analysis.audio_path.as_deref(), Some("?/saltwater.flac"));
    assert_eq!(analysis.beat_grid.unwrap().beats.len(), 2);
    assert_eq!(analysis.song_structure.unwrap().phrases.len(), 1);
    let kinds: Vec<_> = analysis.sources.iter().map(|source| source.kind).collect();
    assert_eq!(kinds, [AnalysisFileKind::Dat, AnalysisFileKind::Ext]);
    assert_eq!(
        analysis.sources[0].skipped_sections[0].fourcc,
        FourCc(*b"PVBR")
    );
    assert_eq!(std::fs::read(&dat).unwrap(), dat_before);
    assert_eq!(std::fs::read(&ext).unwrap(), ext_before);
}

#[test]
fn reports_a_track_without_analysis() {
    let (_directory, library) = open_fixture();
    let tracks = library.tracks().unwrap();
    let low_orbit = tracks.iter().find(|track| track.id == "t2").unwrap();

    let result = library.analysis(low_orbit);

    assert!(matches!(result, Err(Error::NoAnalysis(id)) if id == "t2"));
}

#[test]
fn reports_a_missing_analysis_file() {
    let (_directory, library) = open_fixture();
    let tracks = library.tracks().unwrap();
    let saltwater = tracks.iter().find(|track| track.id == "t1").unwrap();

    let result = library.analysis(saltwater);

    assert!(
        matches!(&result, Err(Error::AnalysisRead { path, .. }) if path.ends_with("ANLZ0000.DAT")),
        "{result:?}"
    );
}

#[test]
fn names_the_file_when_an_analysis_file_is_malformed() {
    let (directory, library) = open_fixture();
    let share = directory.path().join("share");
    write_analysis(
        &share,
        "DAT",
        &analysis_file(&[beat_grid(&[(1, 12400, 120)])]),
    );
    let ext = write_analysis(&share, "EXT", b"PMAI\0\0\0\x1c\0\0\0\x10");
    let tracks = library.tracks().unwrap();
    let saltwater = tracks.iter().find(|track| track.id == "t1").unwrap();

    let result = library.analysis(saltwater);

    let Err(Error::Analysis { path, source }) = result else {
        panic!("expected an analysis error, got {result:?}");
    };
    assert_eq!(path, ext);
    assert_eq!(
        source,
        AnalysisError::FileLengthMismatch {
            declared: 16,
            actual: 12
        }
    );
}

#[test]
fn refuses_an_analysis_path_that_leaves_the_share_directory() {
    let directory = tempfile::tempdir().unwrap();

    let result = TrackAnalysis::read(directory.path(), "/PIONEER/../../secret.DAT");

    assert!(matches!(result, Err(Error::AnalysisPath(_))));
}
