//! A track's analysis files for the stick: the DJ's grid and cues, with rekordbox's waveforms
//! and phrases when rekordbox analysed the track, else waveforms Dubplate draws from the audio.

use std::path::Path;

use dubplate_engine::{decode_file_to_end, three_band_levels};
use dubplate_rekordbox::TrackAnalysis;
use dubplate_rekordbox::analysis::{
    AnalysisExport, AnalysisFile, AnalysisFiles, COPIED_SECTIONS, FourCc, RawSection,
    ThreeBandColumn, Waveforms, derive_waveforms,
};

use crate::error::UsbExportError;
use crate::request::UsbTrack;

/// rekordbox's extended grid, which describes rekordbox's own grid only.
const EXTENDED_BEAT_GRID: FourCc = FourCc(*b"PQT2");

/// What a track's analysis brings beside its files.
#[derive(Debug, Clone, Default)]
pub(crate) struct DecodedFacts {
    /// The length in seconds, when the audio was decoded.
    pub duration_seconds: Option<f64>,
}

/// Where the waveforms and copied sections of a track come from.
struct WaveformSource {
    waveforms: Waveforms,
    copied: Vec<RawSection>,
    facts: DecodedFacts,
}

/// The analysis files of `track`, whose audio is `device_path` on the stick, and what Dubplate
/// learned decoding it. Problems the DJ should hear about, such as a waveform that could not
/// be drawn, are added to `warnings`.
pub(crate) fn analysis_files(
    track: &UsbTrack,
    device_path: &str,
    warnings: &mut Vec<String>,
) -> Result<(AnalysisFiles, DecodedFacts), UsbExportError> {
    let source = rekordbox_waveforms(track, warnings)
        .unwrap_or_else(|| drawn_waveforms(&track.source, warnings));
    let cues = &track.cues;
    let files = AnalysisFiles::write(&AnalysisExport {
        audio_path: device_path,
        beat_grid: track.beat_grid.as_ref(),
        cues,
        waveforms: &source.waveforms,
        copied: &source.copied,
    })
    .map_err(|source| UsbExportError::Analysis {
        path: track.source.clone(),
        source,
    })?;
    Ok((files, source.facts))
}

/// rekordbox's waveforms and the sections Dubplate copies, or `None` when rekordbox has no
/// analysis Dubplate can read.
fn rekordbox_waveforms(track: &UsbTrack, warnings: &mut Vec<String>) -> Option<WaveformSource> {
    let rekordbox = track.rekordbox_analysis.as_ref()?;
    let analysis = match TrackAnalysis::read(&rekordbox.share_directory, &rekordbox.analysis_path) {
        Ok(analysis) => analysis,
        Err(error) => {
            warnings.push(format!(
                "{}: rekordbox's analysis could not be read, so Dubplate drew the waveforms \
                 itself ({error}).",
                track.source.display()
            ));
            return None;
        }
    };
    let mut copied = Vec::new();
    for source in &analysis.sources {
        let sections = match std::fs::read(&source.path)
            .map_err(|error| error.to_string())
            .and_then(|bytes| AnalysisFile::raw_sections(&bytes).map_err(|error| error.to_string()))
        {
            Ok(sections) => sections,
            Err(error) => {
                warnings.push(format!(
                    "{}: the phrases and seek index of {} could not be copied ({error}).",
                    track.source.display(),
                    source.path.display()
                ));
                Vec::new()
            }
        };
        copied.extend(sections.into_iter().filter(|section| {
            let copiable = COPIED_SECTIONS
                .iter()
                .any(|code| code.as_bytes() == section.fourcc.0);
            let stale_grid = section.fourcc == EXTENDED_BEAT_GRID && !track.grid_is_rekordbox_s;
            copiable && !stale_grid
        }));
    }
    copied.dedup_by(|later, earlier| later.fourcc == earlier.fourcc);
    Some(WaveformSource {
        waveforms: analysis.waveforms,
        copied,
        facts: DecodedFacts::default(),
    })
}

/// Waveforms drawn from the decoded audio, or none when it cannot be decoded.
fn drawn_waveforms(source: &Path, warnings: &mut Vec<String>) -> WaveformSource {
    let empty = || WaveformSource {
        waveforms: Waveforms::default(),
        copied: Vec::new(),
        facts: DecodedFacts::default(),
    };
    let audio = match decode_file_to_end(source) {
        Ok(audio) => audio,
        Err(error) => {
            warnings.push(format!(
                "{}: the audio could not be decoded, so the stick has no waveform for it \
                 ({error}).",
                source.display()
            ));
            return empty();
        }
    };
    let detail: Vec<ThreeBandColumn> = three_band_levels(&audio)
        .into_iter()
        .map(|levels| ThreeBandColumn {
            low: levels.low,
            mid: levels.mid,
            high: levels.high,
        })
        .collect();
    WaveformSource {
        waveforms: derive_waveforms(&detail),
        copied: Vec::new(),
        facts: DecodedFacts {
            duration_seconds: Some(audio.decoded_seconds()),
        },
    }
}
