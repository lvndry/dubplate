//! Writes analysis files and reads them back with Dubplate's reader, compares a small track
//! with its golden description, and rewrites real rekordbox analysis files to check every
//! section Dubplate writes comes out byte for byte as rekordbox wrote it.

use std::path::{Path, PathBuf};

use dubplate_rekordbox::analysis::{
    AnalysisCue, AnalysisExport, AnalysisFile, AnalysisFileKind, AnalysisFiles, AnalysisWriteError,
    Beat, BeatGrid, COPIED_SECTIONS, FourCc, HotCueColor, LoopLength, RawSection, Rgb, Section,
    ThreeBandColumn, TrackAnalysis, derive_waveforms,
};

const AUDIO_PATH: &str = "/Contents/Shimza/Kabza Sessions/Akufanele.mp3";

fn beat(time_ms: u32, beat_in_bar: u16, tempo_hundredths: u16) -> Beat {
    Beat {
        beat_in_bar,
        tempo_hundredths,
        time_ms,
    }
}

fn grid() -> BeatGrid {
    let mut beats: Vec<Beat> = (0..16)
        .map(|index| beat(160 + index * 492, (index % 4) as u16 + 1, 12_200))
        .collect();
    beats.push(beat(160 + 16 * 492, 1, 12_300));
    BeatGrid { beats }
}

fn hot_cue(pad: u32, time_ms: u32) -> AnalysisCue {
    AnalysisCue {
        hot_cue: Some(pad),
        pad: char::from_u32(u32::from('A') + pad - 1),
        time_ms,
        loop_end_ms: None,
        loop_length: None,
        name: None,
        memory_color: None,
        hot_cue_color: None,
    }
}

fn memory_cue(time_ms: u32) -> AnalysisCue {
    AnalysisCue {
        hot_cue: None,
        pad: None,
        ..hot_cue(1, time_ms)
    }
}

/// A coloured, named hot cue on A, a quantized hot loop on B, a hot cue on H, a coloured named
/// memory cue and a memory loop.
fn cues() -> Vec<AnalysisCue> {
    vec![
        AnalysisCue {
            name: Some("Drop".to_owned()),
            hot_cue_color: Some(HotCueColor {
                code: 42,
                display: Some(Rgb {
                    red: 0xe6,
                    green: 0x28,
                    blue: 0x28,
                }),
                led: Some(Rgb {
                    red: 0xff,
                    green: 0x00,
                    blue: 0x00,
                }),
            }),
            ..hot_cue(1, 31_648)
        },
        AnalysisCue {
            loop_end_ms: Some(33_616),
            loop_length: Some(LoopLength {
                numerator: 4,
                denominator: 1,
            }),
            ..hot_cue(2, 31_648)
        },
        hot_cue(8, 200_000),
        AnalysisCue {
            name: Some("Mix in — vocals".to_owned()),
            memory_color: Some(6),
            ..memory_cue(160)
        },
        AnalysisCue {
            loop_end_ms: Some(8_032),
            ..memory_cue(4_096)
        },
    ]
}

fn detail() -> Vec<ThreeBandColumn> {
    (0..30_000_u32)
        .map(|index| ThreeBandColumn {
            low: if index % 74 < 6 { 240 } else { 30 },
            mid: (index % 180) as u8,
            high: 70,
        })
        .collect()
}

/// A plain phrase section of a high-mood track: one Intro phrase on beat 1, ending on beat 17.
fn song_structure() -> RawSection {
    let mut body = Vec::new();
    body.extend_from_slice(&24_u32.to_be_bytes());
    body.extend_from_slice(&1_u16.to_be_bytes());
    body.extend_from_slice(&1_u16.to_be_bytes());
    body.extend_from_slice(&[0; 6]);
    body.extend_from_slice(&17_u16.to_be_bytes());
    body.extend_from_slice(&[0, 0, 0, 0]);
    let mut phrase = [0_u8; 24];
    phrase[0..2].copy_from_slice(&1_u16.to_be_bytes());
    phrase[2..4].copy_from_slice(&1_u16.to_be_bytes());
    phrase[4..6].copy_from_slice(&1_u16.to_be_bytes());
    phrase[7] = 1;
    body.extend_from_slice(&phrase);
    let mut bytes = b"PSSI".to_vec();
    bytes.extend_from_slice(&0x20_u32.to_be_bytes());
    bytes.extend_from_slice(&(12 + body.len() as u32).to_be_bytes());
    bytes.extend_from_slice(&body);
    RawSection {
        fourcc: FourCc(*b"PSSI"),
        bytes,
    }
}

fn write_small() -> AnalysisFiles {
    let grid = grid();
    let cues = cues();
    let waveforms = derive_waveforms(&detail());
    AnalysisFiles::write(&AnalysisExport {
        audio_path: AUDIO_PATH,
        beat_grid: Some(&grid),
        cues: &cues,
        waveforms: &waveforms,
        copied: &[song_structure()],
    })
    .expect("the small track writes")
}

fn read_back(files: &AnalysisFiles) -> TrackAnalysis {
    TrackAnalysis::from_files(
        [
            AnalysisFileKind::Dat,
            AnalysisFileKind::Ext,
            AnalysisFileKind::TwoEx,
        ]
        .into_iter()
        .map(|kind| {
            let file = AnalysisFile::parse(files.file(kind)).expect("a written file reads");
            (kind, PathBuf::from(kind.extension()), file)
        })
        .collect(),
    )
}

#[test]
fn every_section_reads_back() {
    let files = write_small();
    let analysis = read_back(&files);
    assert_eq!(analysis.audio_path.as_deref(), Some(AUDIO_PATH));
    assert_eq!(analysis.beat_grid, Some(grid()));
    let mut expected = cues();
    expected.sort_by_key(|cue| (cue.time_ms, cue.hot_cue));
    assert_eq!(analysis.cues, expected);
    assert_eq!(analysis.waveforms, derive_waveforms(&detail()));
    let phrases = analysis.song_structure.expect("the phrases are copied");
    assert_eq!(phrases.phrases.len(), 1);
    assert_eq!(phrases.end_beat, 17);
    for source in &analysis.sources {
        assert!(source.skipped_sections.iter().all(|skipped| {
            COPIED_SECTIONS
                .iter()
                .any(|code| code.as_bytes() == skipped.fourcc.0)
        }));
    }
}

#[test]
fn the_basic_cue_lists_carry_every_cue_for_older_players() {
    let files = write_small();
    let dat = AnalysisFile::parse(&files.dat).unwrap();
    let lists: Vec<usize> = dat
        .sections
        .iter()
        .filter_map(|section| match section {
            Section::CueList(list) => Some(list.cues.len()),
            _ => None,
        })
        .collect();
    assert_eq!(lists, [3, 2], "hot cues first, then memory cues");
}

#[test]
fn a_hot_cue_colour_without_its_led_colour_lights_in_its_palette_colour() {
    let grid = grid();
    let cue = AnalysisCue {
        hot_cue_color: Some(HotCueColor {
            code: 1,
            display: None,
            led: None,
        }),
        ..hot_cue(1, 1_000)
    };
    let waveforms = Default::default();
    let files = AnalysisFiles::write(&AnalysisExport {
        audio_path: AUDIO_PATH,
        beat_grid: Some(&grid),
        cues: &[cue],
        waveforms: &waveforms,
        copied: &[],
    })
    .unwrap();
    let color = read_back(&files).cues[0].hot_cue_color.unwrap();
    assert_eq!(color.code, 1);
    assert_eq!(color.led, color.display);
    assert!(color.led.is_some());
}

#[test]
fn only_the_sections_dubplate_does_not_model_are_copied() {
    let waveforms = Default::default();
    let result = AnalysisFiles::write(&AnalysisExport {
        audio_path: AUDIO_PATH,
        beat_grid: None,
        cues: &[],
        waveforms: &waveforms,
        copied: &[RawSection {
            fourcc: FourCc(*b"PQTZ"),
            bytes: Vec::new(),
        }],
    });
    assert!(matches!(result, Err(AnalysisWriteError::NotCopied(_))));
}

fn describe(bytes: &[u8]) -> String {
    let mut text = format!("{} bytes\n", bytes.len());
    for section in AnalysisFile::raw_sections(bytes).expect("a written file splits") {
        let header_length =
            u32::from_be_bytes(section.bytes[4..8].try_into().expect("four bytes")) as usize;
        let header: String = section.bytes[12..header_length]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        text.push_str(&format!(
            "{} header {header_length} length {} fields {header}\n",
            section.fourcc,
            section.bytes.len()
        ));
        if matches!(&section.fourcc.0, b"PCOB" | b"PCO2" | b"PPTH" | b"PQTZ") {
            for chunk in section.bytes[header_length..].chunks(32) {
                let hex: String = chunk.iter().map(|byte| format!("{byte:02x}")).collect();
                text.push_str(&format!("  {hex}\n"));
            }
        }
    }
    text
}

fn golden_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("write_analysis")
        .join(name)
}

/// Compares `actual` with the golden file `name`; `DUBPLATE_BLESS=1` writes it instead.
fn assert_golden(name: &str, actual: &str) {
    let path = golden_path(name);
    if std::env::var_os("DUBPLATE_BLESS").is_some() {
        let folder = path.parent().expect("a golden file sits in a folder");
        std::fs::create_dir_all(folder).expect("the golden folder can be made");
        std::fs::write(&path, actual).expect("the golden file can be written");
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}; run with DUBPLATE_BLESS=1", path.display()));
    assert!(
        expected == actual,
        "{} changed; review the difference and run with DUBPLATE_BLESS=1 to accept it",
        path.display()
    );
}

/// Writes the small track's files into the folder `DUBPLATE_ANALYSIS_SAMPLES` names, for
/// independent readers to check.
fn write_samples(files: &AnalysisFiles) {
    let Some(folder) = std::env::var_os("DUBPLATE_ANALYSIS_SAMPLES") else {
        return;
    };
    std::fs::create_dir_all(&folder).expect("the sample folder can be made");
    for kind in [
        AnalysisFileKind::Dat,
        AnalysisFileKind::Ext,
        AnalysisFileKind::TwoEx,
    ] {
        let path = Path::new(&folder).join(format!("ANLZ0000.{}", kind.extension()));
        std::fs::write(path, files.file(kind)).expect("the sample can be written");
    }
}

#[test]
fn a_small_track_matches_its_golden_files() {
    let files = write_small();
    write_samples(&files);
    assert_golden("ANLZ0000.DAT.txt", &describe(&files.dat));
    assert_golden("ANLZ0000.EXT.txt", &describe(&files.ext));
    assert_golden("ANLZ0000.2EX.txt", &describe(&files.two_ex));
}

fn analysis_files_under(folder: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            analysis_files_under(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "DAT")
            && path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("ANLZ"))
        {
            found.push(path);
        }
    }
}

/// Rewrites every analysis file under the folder `DUBPLATE_REKORDBOX_ANALYSIS` names from what
/// Dubplate reads of it, and checks every section Dubplate writes is rekordbox's byte for byte,
/// in rekordbox's order. Cue lists are compared only when empty, since the files Dubplate was
/// checked against hold no cues.
#[test]
#[ignore = "needs rekordbox analysis files; set DUBPLATE_REKORDBOX_ANALYSIS"]
fn rewrites_real_rekordbox_analysis_byte_for_byte() {
    let Some(root) = std::env::var_os("DUBPLATE_REKORDBOX_ANALYSIS") else {
        return;
    };
    let mut dat_files = Vec::new();
    analysis_files_under(Path::new(&root), &mut dat_files);
    assert!(!dat_files.is_empty(), "no analysis file found");
    let mut compared_sections = 0;
    for dat in &dat_files {
        let kinds = [
            AnalysisFileKind::Dat,
            AnalysisFileKind::Ext,
            AnalysisFileKind::TwoEx,
        ];
        let originals: Vec<(AnalysisFileKind, Vec<u8>)> = kinds
            .into_iter()
            .filter_map(|kind| {
                Some((
                    kind,
                    std::fs::read(dat.with_extension(kind.extension())).ok()?,
                ))
            })
            .collect();
        let analysis = TrackAnalysis::from_files(
            originals
                .iter()
                .map(|(kind, bytes)| {
                    let file = AnalysisFile::parse(bytes)
                        .unwrap_or_else(|error| panic!("{}: {error}", dat.display()));
                    (*kind, dat.with_extension(kind.extension()), file)
                })
                .collect(),
        );
        let copied: Vec<RawSection> = originals
            .iter()
            .flat_map(|(_, bytes)| AnalysisFile::raw_sections(bytes).expect("splits"))
            .filter(|section| {
                COPIED_SECTIONS
                    .iter()
                    .any(|code| code.as_bytes() == section.fourcc.0)
            })
            .collect();
        let audio_path = analysis.audio_path.clone().unwrap_or_default();
        let written = AnalysisFiles::write(&AnalysisExport {
            audio_path: &audio_path,
            beat_grid: analysis.beat_grid.as_ref(),
            cues: &analysis.cues,
            waveforms: &analysis.waveforms,
            copied: &copied,
        })
        .expect("a real track rewrites");
        for (kind, original) in &originals {
            let ours = AnalysisFile::raw_sections(written.file(*kind)).expect("splits");
            let theirs = AnalysisFile::raw_sections(original).expect("splits");
            let comparable: Vec<&RawSection> = theirs
                .iter()
                .filter(|section| {
                    let cue_list = matches!(&section.fourcc.0, b"PCOB" | b"PCO2");
                    let empty_cue_list = cue_list && section.bytes.len() <= 0x18;
                    (!cue_list || empty_cue_list)
                        && ours.iter().any(|mine| mine.fourcc == section.fourcc)
                })
                .collect();
            let our_order: Vec<FourCc> = ours
                .iter()
                .filter(|section| {
                    comparable
                        .iter()
                        .any(|theirs| theirs.fourcc == section.fourcc)
                })
                .map(|section| section.fourcc)
                .collect();
            let mut their_order: Vec<FourCc> =
                comparable.iter().map(|section| section.fourcc).collect();
            their_order.dedup();
            let mut our_order_unique = our_order.clone();
            our_order_unique.dedup();
            assert_eq!(our_order_unique, their_order, "{} {kind}", dat.display());
            for section in comparable {
                let mine = ours
                    .iter()
                    .filter(|mine| mine.fourcc == section.fourcc)
                    .find(|mine| mine.bytes == section.bytes);
                assert!(
                    mine.is_some(),
                    "{} {kind}: our {} differs from rekordbox's",
                    dat.display(),
                    section.fourcc
                );
                compared_sections += 1;
            }
        }
    }
    eprintln!(
        "{compared_sections} sections of {} tracks match rekordbox's",
        dat_files.len()
    );
}
