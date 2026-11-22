//! A prepared set as playlists for the stick: its tracks with the grids, cues and tags the
//! rekordbox XML export would give them.

use std::path::Path;

use dubplate_prep::{
    AudioAnalysis, ExportLibrary, ExportTrack, GridCorrection, Set, with_mix_cues,
};
use dubplate_rekordbox::analysis::{AnalysisCue, HotCueColor};
use dubplate_rekordbox::{HotCuePad, TrackCues};

use crate::request::{RekordboxAnalysis, TrackTags, UsbPlaylist, UsbTrack};

/// The playlists of `set` for the stick: one with the set's tracks in order and, when the set
/// keeps backups, one named after it with them, as the rekordbox XML export names them. Folder
/// tracks take their grid from `dubplate_analysis`, Dubplate's analysis of a file. What could
/// not be resolved as it should comes back as one sentence each.
pub fn playlists_from_set(
    library: &ExportLibrary<'_>,
    set: &Set,
    dubplate_analysis: &mut dyn FnMut(&Path) -> Option<AudioAnalysis>,
) -> (Vec<UsbPlaylist>, Vec<String>) {
    let (tracks, problems) = library.tracks(set);
    let mut main = UsbPlaylist {
        name: set.name.clone(),
        tracks: Vec::new(),
    };
    let mut backups = UsbPlaylist {
        name: format!("{} backups", set.name),
        tracks: Vec::new(),
    };
    for track in &tracks {
        let usb_track = usb_track(track, library, dubplate_analysis);
        if track.entry.is_backup {
            backups.tracks.push(usb_track);
        } else {
            main.tracks.push(usb_track);
        }
    }
    let mut playlists = vec![main];
    if !backups.tracks.is_empty() {
        playlists.push(backups);
    }
    (playlists, problems)
}

fn usb_track(
    export: &ExportTrack,
    library: &ExportLibrary<'_>,
    dubplate_analysis: &mut dyn FnMut(&Path) -> Option<AudioAnalysis>,
) -> UsbTrack {
    let correction = GridCorrection::of(&export.entry);
    let track = &export.track;
    let source = export.location().to_path_buf();
    let (grid, analysis) = match &export.beat_grid {
        Some(grid) => (Some(grid.clone()), None),
        None if export.in_rekordbox => (None, None),
        None => {
            let analysis = dubplate_analysis(&source);
            (
                analysis.as_ref().and_then(AudioAnalysis::rekordbox_grid),
                analysis,
            )
        }
    };
    let grid_is_rekordbox_s = export.beat_grid.is_some() && correction == GridCorrection::default();
    let rekordbox_analysis = library.share_directory().and_then(|share_directory| {
        let analysis_path = track
            .analysis_path
            .as_deref()
            .filter(|path| !path.is_empty())?;
        Some(RekordboxAnalysis {
            share_directory: share_directory.to_path_buf(),
            analysis_path: analysis_path.to_owned(),
        })
    });
    let key = track.key.clone().or_else(|| {
        analysis
            .as_ref()
            .and_then(|analysis| analysis.key)
            .map(|key| key.short_name())
    });
    UsbTrack {
        source,
        tags: TrackTags {
            title: track.title.clone(),
            artist: track.artist.clone(),
            album: track.album.clone(),
            genre: track.genre.clone(),
            label: track.label.clone(),
            key,
            comment: track.comment.clone(),
            rating: track.rating.unwrap_or(0),
            color: None,
            year: None,
            date_added: track.date_added.clone(),
        },
        bpm: track.bpm.or_else(|| {
            analysis
                .as_ref()
                .and_then(|analysis| analysis.bpm.map(f64::from))
        }),
        duration_seconds: track.length_seconds.map(f64::from).or_else(|| {
            analysis
                .as_ref()
                .map(|analysis| f64::from(analysis.duration_seconds))
        }),
        beat_grid: grid.map(|grid| correction.apply(&grid)),
        grid_is_rekordbox_s,
        cues: analysis_cues(&with_mix_cues(&export.entry, &export.cues)),
        rekordbox_analysis,
    }
}

/// A track's cues as the analysis files hold them: hot cues numbered from 1 for pad A, memory
/// cues with their colour's row in the stick's colour table.
pub fn analysis_cues(cues: &TrackCues) -> Vec<AnalysisCue> {
    let hot = cues.hot_cues().map(|(pad, cue)| AnalysisCue {
        hot_cue: Some(pad_number(pad)),
        pad: Some(pad.letter()),
        time_ms: cue.start_ms,
        loop_end_ms: cue.loop_end_ms,
        loop_length: None,
        name: Some(cue.name.clone()).filter(|name| !name.is_empty()),
        memory_color: None,
        hot_cue_color: cue.color.map(|color| HotCueColor {
            code: color.index(),
            display: Some(color.rgb()),
            led: None,
        }),
    });
    let memory = cues.memory_cues().iter().map(|cue| AnalysisCue {
        hot_cue: None,
        pad: None,
        time_ms: cue.start_ms,
        loop_end_ms: cue.loop_end_ms,
        loop_length: None,
        name: Some(cue.name.clone()).filter(|name| !name.is_empty()),
        memory_color: cue.color.map(|color| color.rekordbox_value() + 1),
        hot_cue_color: None,
    });
    hot.chain(memory).collect()
}

fn pad_number(pad: HotCuePad) -> u32 {
    u32::try_from(pad.index()).map_or(1, |index| index + 1)
}

#[cfg(test)]
mod tests {
    use dubplate_rekordbox::{HotCue, MemoryCue, MemoryCueColor, PaletteColor};

    use super::*;

    #[test]
    fn cues_keep_their_pads_colours_and_names() {
        let mut cues = TrackCues::default();
        cues.set_hot_cue(
            HotCuePad::E,
            HotCue {
                color: PaletteColor::from_index(42),
                name: "Drop".to_owned(),
                ..HotCue::at(31_648)
            },
        );
        cues.insert_memory_cue(MemoryCue {
            color: Some(MemoryCueColor::Aqua),
            loop_end_ms: Some(8_000),
            ..MemoryCue::at(4_000)
        });
        let written = analysis_cues(&cues);
        assert_eq!(written[0].hot_cue, Some(5));
        assert_eq!(written[0].pad, Some('E'));
        assert_eq!(written[0].hot_cue_color.map(|color| color.code), Some(42));
        assert_eq!(written[0].name.as_deref(), Some("Drop"));
        assert_eq!(written[1].memory_color, Some(6));
        assert_eq!(written[1].loop_end_ms, Some(8_000));
        assert_eq!(written[1].name, None);
    }
}
