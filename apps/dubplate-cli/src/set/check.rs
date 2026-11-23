//! The pre-gig report for a saved set. Positions in both output formats start at one.

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use dubplate_prep::{
    SetCheckReport, SetCheckRow, SetCheckTrack, SetStore, SetWarning, WarningKind, WarningPosition,
    check_set,
};
use serde_json::{Value, json};

use super::{check_sources, data_folder, find_set};

pub(super) fn run(name: &str, as_json: bool, library: Option<PathBuf>) -> Result<ExitCode> {
    let data = data_folder()?;
    let path = data.sets();
    if !path.try_exists()? {
        bail!(
            "No saved sets found in {}. Create a set in Dubplate first.",
            path.display()
        );
    }
    let store =
        SetStore::open_read_only(&path).with_context(|| format!("opening {}", path.display()))?;
    let set = find_set(&store, name)?;
    drop(store);
    let (sources, notes) = check_sources::load(&data, &set, library)?;
    let report = check_set(&set, |entry| {
        sources
            .get(&entry.track_path)
            .map_or_else(SetCheckTrack::default, check_sources::CheckSource::facts)
    });
    let mut out = io::stdout().lock();
    if as_json {
        serde_json::to_writer_pretty(&mut out, &json_report(&set.name, &report, &notes))?;
        writeln!(out)?;
    } else {
        print_report(&mut out, &set.name, &report, &notes)?;
    }
    Ok(if report.is_ready() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(2)
    })
}

fn print_report(
    out: &mut impl Write,
    name: &str,
    report: &SetCheckReport,
    notes: &[String],
) -> io::Result<()> {
    let ready = report
        .tracks
        .iter()
        .filter(|row| row.ready.is_ready())
        .count();
    writeln!(
        out,
        "{name} — {ready}/{} main tracks ready",
        report.tracks.len()
    )?;
    if report.tracks.is_empty() {
        writeln!(out, "No main tracks in the running order.")?;
    }
    print_rows(out, &report.tracks)?;
    if !report.warnings.is_empty() {
        writeln!(out, "\nWarnings")?;
        for warning in &report.warnings {
            let location = match warning.position {
                WarningPosition::Track { index } => {
                    format!("#{} {}", index + 1, report.tracks[index].title)
                }
                WarningPosition::Seam { after } => format!(
                    "#{} {} → #{} {}",
                    after + 1,
                    report.tracks[after].title,
                    after + 2,
                    report.tracks[after + 1].title
                ),
            };
            writeln!(out, "  {location} — {}", warning.kind.label())?;
        }
    }
    if !report.backups.is_empty() {
        writeln!(out, "\nBackups")?;
        print_rows(out, &report.backups)?;
    }
    for note in notes {
        writeln!(out, "Note: {note}")?;
    }
    Ok(())
}

fn print_rows(out: &mut impl Write, rows: &[SetCheckRow]) -> io::Result<()> {
    for (index, row) in rows.iter().enumerate() {
        let status = if row.ready.is_ready() {
            String::from("Ready")
        } else {
            row.ready
                .problems
                .iter()
                .map(|p| p.label())
                .collect::<Vec<_>>()
                .join(", ")
        };
        writeln!(out, "  {:>2}. {} — {status}", index + 1, row.title)?;
    }
    Ok(())
}

fn json_report(name: &str, report: &SetCheckReport, notes: &[String]) -> Value {
    json!({
        "set": name,
        "ready": report.is_ready(),
        "tracks": json_rows(&report.tracks),
        "backups": json_rows(&report.backups),
        "warnings": report.warnings.iter().map(json_warning).collect::<Vec<_>>(),
        "notes": notes,
    })
}

fn json_rows(tracks: &[SetCheckRow]) -> Vec<Value> {
    tracks
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let problems: Vec<_> = row
                .ready
                .problems
                .iter()
                .map(|problem| problem.label())
                .collect();
            json!({
                "position": index + 1,
                "title": row.title,
                "path": row.track_path.to_string_lossy(),
                "ready": row.ready.is_ready(),
                "problems": problems,
            })
        })
        .collect()
}

fn json_warning(warning: &SetWarning) -> Value {
    let mut fields = match warning.kind {
        WarningKind::KeyClash { outgoing, incoming } => {
            json!({"kind":"key_clash", "outgoing_key":outgoing.camelot(), "incoming_key":incoming.camelot()})
        }
        WarningKind::TempoGap { percent } => json!({"kind":"tempo_gap", "percent":percent}),
        WarningKind::VocalsOverlap => json!({"kind":"vocals_overlap"}),
        WarningKind::EnergyDrop { by } => json!({"kind":"energy_drop", "by":by}),
        WarningKind::Duplicate { first_position } => {
            json!({"kind":"duplicate", "first_position":first_position+1})
        }
        WarningKind::SimilarVersion { first_position } => {
            json!({"kind":"similar_version", "first_position":first_position+1})
        }
    };
    match warning.position {
        WarningPosition::Track { index } => fields["position"] = json!(index + 1),
        WarningPosition::Seam { after } => {
            fields["from"] = json!(after + 1);
            fields["to"] = json!(after + 2);
        }
    }
    fields["message"] = json!(warning.kind.label());
    fields
}
