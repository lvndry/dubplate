use anyhow::{Context, Result, bail};
use dubplate_rekordbox::analysis::{
    AnalysisCue, BeatGrid, DETAIL_COLUMNS_PER_SECOND, SongStructure, TrackAnalysis, Waveforms,
    memory_color_name,
};
use dubplate_rekordbox::{Cue, Library, Track};

pub(crate) fn show(library: &Library, query: &str, json: bool) -> Result<()> {
    let tracks = library.tracks()?;
    let track = find_track(&tracks, query)?;
    let analysis = library
        .analysis(track)
        .with_context(|| format!("reading the analysis of {}", describe(track)))?;
    let mut master_db_cues: Vec<Cue> = library
        .cues()?
        .into_iter()
        .filter(|cue| cue.track_id == track.id)
        .collect();
    master_db_cues.sort_by_key(|cue| (cue.start_ms, cue.kind.pad()));

    if json {
        let master_db_cues: Vec<_> = master_db_cues
            .iter()
            .map(|cue| serde_json::json!({ "pad": cue.kind.pad(), "cue": cue }))
            .collect();
        let document = serde_json::json!({
            "track": track,
            "analysis": analysis,
            "master_db_cues": master_db_cues,
        });
        println!("{}", serde_json::to_string_pretty(&document)?);
        return Ok(());
    }

    println!("Track       {}", describe(track));
    for source in &analysis.sources {
        println!("Read        {}", source.path.display());
    }
    if let Some(audio_path) = &analysis.audio_path {
        println!("Analysed    {audio_path}");
    }
    println!();
    print_beat_grid(analysis.beat_grid.as_ref(), track);
    println!();
    print_cues(&analysis, &master_db_cues);
    println!();
    print_phrases(
        analysis.song_structure.as_ref(),
        analysis.beat_grid.as_ref(),
    );
    println!();
    print_waveforms(&analysis.waveforms);
    print_skipped(&analysis);
    Ok(())
}

fn find_track<'tracks>(tracks: &'tracks [Track], query: &str) -> Result<&'tracks Track> {
    if let Some(track) = tracks.iter().find(|track| track.id == query) {
        return Ok(track);
    }
    let titled: Vec<&Track> = tracks
        .iter()
        .filter(|track| track.title.as_deref() == Some(query))
        .collect();
    match titled.as_slice() {
        [] => bail!("no track has the ID or the exact title \"{query}\""),
        [track] => Ok(track),
        several => {
            let choices: Vec<String> = several.iter().map(|track| describe(track)).collect();
            bail!(
                "{} tracks are titled \"{query}\". Pass one of their IDs instead:\n  {}",
                several.len(),
                choices.join("\n  ")
            )
        }
    }
}

fn describe(track: &Track) -> String {
    format!(
        "{} by {} (ID {})",
        track.title.as_deref().unwrap_or("Untitled"),
        track.artist.as_deref().unwrap_or("unknown artist"),
        track.id
    )
}

fn print_beat_grid(grid: Option<&BeatGrid>, track: &Track) {
    println!("Beat grid");
    let Some(grid) = grid else {
        println!("  The analysis files have no beat grid.");
        return;
    };
    println!("  Beats            {}", grid.beats.len());
    match grid.first_downbeat() {
        Some(downbeat) => println!("  First downbeat   {}", clock(downbeat.time_ms)),
        None => println!("  First downbeat   none"),
    }
    if let Some((lowest, highest)) = grid.tempo_range_hundredths() {
        if lowest == highest {
            println!("  Tempo            {} BPM", hundredths(lowest));
        } else {
            println!(
                "  Tempo            {} to {} BPM",
                hundredths(lowest),
                hundredths(highest)
            );
        }
    }
    if let Some(bpm) = track.bpm {
        println!("  master.db tempo  {bpm:.2} BPM");
    }
}

fn print_cues(analysis: &TrackAnalysis, master_db_cues: &[Cue]) {
    let grid = analysis.beat_grid.as_ref();
    if !analysis.cues.is_empty() {
        println!("Cues in the analysis files");
        println!(
            "  {:<6} {:<11} {:<16} {:<22} Name",
            "Pad", "Time", "On the grid", "Colour"
        );
        for cue in &analysis.cues {
            print_analysis_cue(cue, grid);
        }
        return;
    }
    println!("Cues");
    println!("  The analysis files hold no cues. rekordbox 6 and 7 keep this collection's cues");
    println!("  in master.db, which has these:");
    if master_db_cues.is_empty() {
        println!("  none");
        return;
    }
    println!(
        "  {:<6} {:<11} {:<16} {:<22} Name",
        "Pad", "Time", "On the grid", "Colour"
    );
    for cue in master_db_cues {
        let pad = cue
            .kind
            .pad()
            .map_or_else(|| "memory".to_owned(), |pad| pad.letter().to_string());
        let colour = match (cue.hot_cue_color, cue.memory_color) {
            (Some(color), _) => format!("{} (index {})", color.rgb(), color.index()),
            (None, Some(color)) => color.name().to_owned(),
            (None, None) => String::new(),
        };
        let time = with_loop_end(cue.start_ms, cue.loop_end_ms);
        println!(
            "  {pad:<6} {time:<11} {:<16} {colour:<22} {}",
            grid_position(grid, cue.start_ms),
            cue.comment.as_deref().unwrap_or("")
        );
    }
}

fn print_analysis_cue(cue: &AnalysisCue, grid: Option<&BeatGrid>) {
    let pad = match (cue.pad, cue.hot_cue) {
        (Some(letter), _) => letter.to_string(),
        (None, Some(number)) => format!("#{number}"),
        (None, None) => "memory".to_owned(),
    };
    let colour = match (&cue.hot_cue_color, cue.memory_color) {
        (Some(color), _) => match color.display.or(color.led) {
            Some(rgb) => format!("{rgb} (code {})", color.code),
            None => format!("default (code {})", color.code),
        },
        (None, Some(color)) => {
            memory_color_name(color).map_or_else(|| format!("colour {color}"), str::to_owned)
        }
        (None, None) => String::new(),
    };
    let time = with_loop_end(cue.time_ms, cue.loop_end_ms);
    println!(
        "  {pad:<6} {time:<11} {:<16} {colour:<22} {}",
        grid_position(grid, cue.time_ms),
        cue.name.as_deref().unwrap_or("")
    );
}

fn with_loop_end(start_ms: u32, loop_end_ms: Option<u32>) -> String {
    match loop_end_ms {
        Some(end) => format!("{} to {}", clock(start_ms), clock(end)),
        None => clock(start_ms),
    }
}

fn grid_position(grid: Option<&BeatGrid>, time_ms: u32) -> String {
    let Some(position) = grid.and_then(|grid| grid.position_of(time_ms)) else {
        return String::new();
    };
    format!(
        "{}.{} {:+} ms",
        position.bar, position.beat_in_bar, position.offset_ms
    )
}

fn print_phrases(structure: Option<&SongStructure>, grid: Option<&BeatGrid>) {
    let Some(structure) = structure else {
        println!("Phrases");
        println!("  The analysis files have no phrases.");
        return;
    };
    println!(
        "Phrases ({:?} mood, {:?} lighting bank, last phrase ends on beat {}{})",
        structure.mood,
        structure.bank,
        structure.end_beat,
        if structure.was_masked {
            ", stored masked"
        } else {
            ""
        }
    );
    println!(
        "  {:>3} {:<10} {:<11} {:<11} Beats",
        "#", "Phrase", "Bars", "Starts"
    );
    for phrase in &structure.phrases {
        let start_bar = grid.and_then(|grid| grid.bar_of_beat(u32::from(phrase.start_beat)));
        let last_bar = grid.and_then(|grid| {
            grid.bar_of_beat(u32::from(phrase.end_beat.saturating_sub(1)))
                .or_else(|| grid.bar_of_beat(u32::try_from(grid.beats.len()).ok()?))
        });
        let bars = match (start_bar, last_bar) {
            (Some(start), Some(last)) if last > start => format!("{start} to {last}"),
            (Some(start), _) => start.to_string(),
            _ => String::new(),
        };
        let starts = grid
            .and_then(|grid| grid.beat(u32::from(phrase.start_beat)))
            .map(|beat| clock(beat.time_ms))
            .unwrap_or_default();
        let fill = phrase
            .fill_start_beat
            .map(|beat| format!(", fill-in from beat {beat}"))
            .unwrap_or_default();
        println!(
            "  {:>3} {:<10} {bars:<11} {starts:<11} {} to {}{fill}",
            phrase.index,
            phrase.kind.to_string(),
            phrase.start_beat,
            phrase.end_beat.saturating_sub(1).max(phrase.start_beat),
        );
    }
}

fn print_waveforms(waveforms: &Waveforms) {
    println!("Waveforms");
    let rows = [
        (
            "Blue overview (PWAV)",
            waveforms.preview.as_ref().map(Vec::len),
            false,
        ),
        (
            "Tiny overview (PWV2)",
            waveforms.tiny_preview.as_ref().map(Vec::len),
            false,
        ),
        (
            "Blue detail (PWV3)",
            waveforms.detail.as_ref().map(Vec::len),
            true,
        ),
        (
            "Colour overview (PWV4)",
            waveforms.color_preview.as_ref().map(Vec::len),
            false,
        ),
        (
            "Colour detail (PWV5)",
            waveforms.color_detail.as_ref().map(Vec::len),
            true,
        ),
        (
            "3-band overview (PWV6)",
            waveforms.three_band_preview.as_ref().map(Vec::len),
            false,
        ),
        (
            "3-band detail (PWV7)",
            waveforms.three_band_detail.as_ref().map(Vec::len),
            true,
        ),
    ];
    for (name, columns, is_detail) in rows {
        let description = match columns {
            None => "not present".to_owned(),
            Some(columns) if is_detail => format!(
                "{columns} columns ({:.2} s at {DETAIL_COLUMNS_PER_SECOND} a second)",
                columns as f64 / f64::from(DETAIL_COLUMNS_PER_SECOND)
            ),
            Some(columns) => format!("{columns} columns"),
        };
        println!("  {name:<24} {description}");
    }
}

fn print_skipped(analysis: &TrackAnalysis) {
    let skipped: Vec<String> = analysis
        .sources
        .iter()
        .flat_map(|source| {
            source.skipped_sections.iter().map(move |section| {
                format!(
                    "{} {} ({} bytes at byte {:#x})",
                    source.kind, section.fourcc, section.length, section.offset
                )
            })
        })
        .collect();
    if skipped.is_empty() {
        return;
    }
    println!();
    println!("Sections Dubplate does not read yet");
    for line in skipped {
        println!("  {line}");
    }
}

fn clock(milliseconds: u32) -> String {
    let minutes = milliseconds / 60_000;
    let seconds = (milliseconds % 60_000) / 1000;
    let rest = milliseconds % 1000;
    format!("{minutes}:{seconds:02}.{rest:03}")
}

fn hundredths(value: u16) -> String {
    format!("{}.{:02}", value / 100, value % 100)
}
