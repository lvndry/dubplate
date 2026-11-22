//! Reads rekordbox's per-track analysis files: `ANLZ0000.DAT` and its `.EXT` and `.2EX`
//! companions, which hold the beat grid, cues, phrases and waveforms.
//!
//! The files are big-endian and made of tagged sections, as documented by Deep Symmetry's
//! analysis of the format.
//!
//! [`AnalysisFiles::write`] writes the three files of a track as a rekordbox USB export carries
//! them, and [`derive_waveforms`] works out every waveform kind from a 3-band detail waveform
//! for tracks rekordbox never analysed.

mod beat_grid;
mod cues;
mod derive;
mod error;
mod file;
mod reader;
mod song_structure;
mod waveform;
mod write;

use std::fmt;
use std::path::{Component, Path, PathBuf};

use serde::{Serialize, Serializer};

pub use beat_grid::{Beat, BeatGrid, GridPosition};
pub use cues::{
    AnalysisCue, CueList, CueListKind, HotCueColor, LoopLength, Rgb, hot_cue_pad, memory_color_name,
};
pub use derive::derive_waveforms;
pub use error::{AnalysisError, FourCc};
pub use file::{AnalysisFile, RawSection, Section, SkippedSection};
pub use song_structure::{LightingBank, Mood, Phrase, PhraseKind, SongStructure};
pub use waveform::{
    ColorDetailColumn, ColorPreviewColumn, DETAIL_COLUMNS_PER_SECOND, MonochromeColumn,
    ThreeBandColumn, TinyColumn, Waveforms,
};
pub use write::{AnalysisExport, AnalysisFiles, AnalysisWriteError, COPIED_SECTIONS};

use crate::error::Error;

/// Which of a track's three analysis files a section came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisFileKind {
    /// `.DAT`: beat grid, basic cues and the blue waveform overview.
    Dat,
    /// `.EXT`: extended cues, phrases and the detail and colour waveforms.
    Ext,
    /// `.2EX`: the 3-band waveforms.
    TwoEx,
}

impl AnalysisFileKind {
    /// The file extension, such as `DAT`.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Dat => "DAT",
            Self::Ext => "EXT",
            Self::TwoEx => "2EX",
        }
    }
}

impl fmt::Display for AnalysisFileKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, ".{}", self.extension())
    }
}

impl Serialize for AnalysisFileKind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.extension())
    }
}

/// An analysis file that was read, with the sections Dubplate skipped in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AnalysisSource {
    /// `.DAT`, `.EXT` or `.2EX`.
    pub kind: AnalysisFileKind,
    /// Where the file is.
    pub path: PathBuf,
    /// Sections of kinds Dubplate does not read.
    pub skipped_sections: Vec<SkippedSection>,
}

/// Everything rekordbox's analysis files say about one track.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TrackAnalysis {
    /// The files that were read. The `.EXT` and `.2EX` files are optional.
    pub sources: Vec<AnalysisSource>,
    /// The audio file the analysis was made from.
    pub audio_path: Option<String>,
    /// The beat grid.
    pub beat_grid: Option<BeatGrid>,
    /// Memory cues, hot cues and loops, ordered by time. They come from the extended `PCO2`
    /// lists when those hold any cue, and otherwise from the basic `PCOB` lists.
    pub cues: Vec<AnalysisCue>,
    /// The phrases.
    pub song_structure: Option<SongStructure>,
    /// The waveforms.
    pub waveforms: Waveforms,
}

impl TrackAnalysis {
    /// Combines the parsed analysis files of one track, `.DAT` first.
    pub fn from_files(files: Vec<(AnalysisFileKind, PathBuf, AnalysisFile)>) -> Self {
        let mut analysis = Self {
            sources: Vec::new(),
            audio_path: None,
            beat_grid: None,
            cues: Vec::new(),
            song_structure: None,
            waveforms: Waveforms::default(),
        };
        let mut basic_cues = Vec::new();
        let mut extended_cues = Vec::new();
        for (kind, path, file) in files {
            let mut skipped_sections = Vec::new();
            for section in file.sections {
                let waveforms = &mut analysis.waveforms;
                match section {
                    Section::BeatGrid(grid) => fill(&mut analysis.beat_grid, grid),
                    Section::CueList(list) if list.extended => extended_cues.extend(list.cues),
                    Section::CueList(list) => basic_cues.extend(list.cues),
                    Section::AudioPath(path) => {
                        if analysis.audio_path.is_none() {
                            analysis.audio_path = path;
                        }
                    }
                    Section::WaveformPreview(columns) => fill(&mut waveforms.preview, columns),
                    Section::TinyWaveformPreview(columns) => {
                        fill(&mut waveforms.tiny_preview, columns);
                    }
                    Section::WaveformDetail(columns) => fill(&mut waveforms.detail, columns),
                    Section::ColorWaveformPreview(columns) => {
                        fill(&mut waveforms.color_preview, columns);
                    }
                    Section::ColorWaveformDetail(columns) => {
                        fill(&mut waveforms.color_detail, columns);
                    }
                    Section::ThreeBandWaveformPreview(columns) => {
                        fill(&mut waveforms.three_band_preview, columns);
                    }
                    Section::ThreeBandWaveformDetail(columns) => {
                        fill(&mut waveforms.three_band_detail, columns);
                    }
                    Section::SongStructure(structure) => {
                        fill(&mut analysis.song_structure, structure);
                    }
                    Section::Skipped(skipped) => skipped_sections.push(skipped),
                }
            }
            analysis.sources.push(AnalysisSource {
                kind,
                path,
                skipped_sections,
            });
        }
        let mut cues = if extended_cues.is_empty() {
            basic_cues
        } else {
            extended_cues
        };
        cues.sort_by_key(|cue| (cue.time_ms, cue.hot_cue));
        analysis.cues = cues;
        analysis
    }

    /// Reads the analysis files of the track whose `analysis_path` is given, as rekordbox
    /// stores it (`/PIONEER/USBANLZ/…/ANLZ0000.DAT`), from rekordbox's `share` directory.
    ///
    /// The files are only opened for reading. The `.DAT` file must exist; the `.EXT` and
    /// `.2EX` files are read when they exist.
    pub fn read(share_directory: &Path, analysis_path: &str) -> Result<Self, Error> {
        let dat_path = resolve(share_directory, analysis_path)?;
        let mut files = Vec::new();
        for kind in [
            AnalysisFileKind::Dat,
            AnalysisFileKind::Ext,
            AnalysisFileKind::TwoEx,
        ] {
            let path = dat_path.with_extension(kind.extension());
            let bytes = match std::fs::read(&path) {
                Ok(bytes) => bytes,
                Err(error)
                    if kind != AnalysisFileKind::Dat
                        && error.kind() == std::io::ErrorKind::NotFound =>
                {
                    continue;
                }
                Err(source) => return Err(Error::AnalysisRead { path, source }),
            };
            let file = AnalysisFile::parse(&bytes).map_err(|source| Error::Analysis {
                path: path.clone(),
                source,
            })?;
            files.push((kind, path, file));
        }
        Ok(Self::from_files(files))
    }
}

fn fill<Value>(slot: &mut Option<Value>, value: Value) {
    if slot.is_none() {
        *slot = Some(value);
    }
}

fn resolve(share_directory: &Path, analysis_path: &str) -> Result<PathBuf, Error> {
    let relative = Path::new(analysis_path.trim_start_matches('/'));
    let stays_inside = relative
        .components()
        .all(|component| matches!(component, Component::Normal(_)));
    if !stays_inside || relative.as_os_str().is_empty() {
        return Err(Error::AnalysisPath(analysis_path.to_owned()));
    }
    Ok(share_directory.join(relative))
}
