//! Export to USB: a set or a crate written to a stick or folder the DJ picks, as CDJs and XDJs
//! read a rekordbox export, with a notice that follows the export and says what it did.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use dubplate_prep::{AnalysisCache, ExportLibrary, Set, SetEntry, SetId};
use dubplate_usb::{
    UsbExportProgress, UsbExportReport, UsbExportStep, export_to_usb, local_today,
    playlists_from_set,
};
use gpui::{Context, PathPromptOptions};

use super::notice::NoticePlace;
use super::{DubplateApp, error_chain};
use crate::collection::{CrateIndex, CrateKind, RekordboxLibrary, TrackOrigin};
use crate::library::RekordboxSource;
use crate::settings::Settings;

/// How long the notice after an export stays.
const EXPORTED_NOTICE_DURATION: Duration = Duration::from_secs(20);

impl DubplateApp {
    /// Asks for a stick or folder and exports the set there, as Command-Shift-E does.
    pub(super) fn export_set_to_usb(&mut self, cx: &mut Context<'_, Self>) {
        let Some(editor) = &self.set_editor else {
            self.show_notice(NoticePlace::Window, "There is no set to export.", cx);
            return;
        };
        let set = editor.set().clone();
        if set.entries.is_empty() {
            self.show_notice(
                NoticePlace::Window,
                "Add tracks to the set before exporting it.",
                cx,
            );
            return;
        }
        self.export_to_usb(set, cx);
    }

    /// Asks for a stick or folder and exports the crate at `crate_index` there as a playlist
    /// of the same name, each track with its grid as the DJ fixed it and its cues.
    pub(super) fn export_crate_to_usb(
        &mut self,
        crate_index: CrateIndex,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(collection) = self.collection().cloned() else {
            return;
        };
        let chosen = collection.crate_at(crate_index);
        if matches!(chosen.kind, CrateKind::Folder | CrateKind::Smart) || chosen.tracks.is_empty() {
            self.show_notice(
                NoticePlace::Window,
                format!("{} has no tracks to export.", chosen.name),
                cx,
            );
            return;
        }
        let entries = chosen
            .tracks
            .iter()
            .filter_map(|index| {
                let found = collection.track(*index);
                let track_path = found.track.file_path.clone()?;
                let mut entry = SetEntry {
                    rekordbox_id: (found.origin == TrackOrigin::Rekordbox)
                        .then(|| found.track.id.clone()),
                    ..SetEntry::default()
                };
                self.track_grid(&track_path).write_to(&mut entry);
                entry.track_path = track_path;
                Some(entry)
            })
            .collect();
        let set = Set {
            id: SetId(0),
            name: chosen.name.to_string(),
            target_minutes: 0,
            created_at: 0,
            updated_at: 0,
            entries,
            seams: Vec::new(),
        };
        self.export_to_usb(set, cx);
    }

    pub(super) fn export_to_usb(&mut self, set: Set, cx: &mut Context<'_, Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Export here".into()),
        });
        let loaded_rekordbox = match &self.sources.rekordbox {
            RekordboxSource::Ready(library) => Some(Arc::clone(library)),
            RekordboxSource::Off | RekordboxSource::Reading | RekordboxSource::Failed(_) => None,
        };
        let needs_rekordbox = set.entries.iter().any(|entry| entry.rekordbox_id.is_some());
        let scans = self.sources.chosen_scans();
        let relinks = self.settings.library.relink_map();
        let track_cues = self.track_cues.clone();
        cx.spawn(async move |this, cx| {
            let destination = match chosen.await {
                Ok(Ok(Some(mut folders))) if !folders.is_empty() => folders.swap_remove(0),
                Ok(Ok(_)) | Err(_) => return,
                Ok(Err(error)) => {
                    this.update(cx, |this, cx| {
                        this.show_notice(
                            NoticePlace::Window,
                            format!("The folder picker could not open: {error}"),
                            cx,
                        );
                    })
                    .ok();
                    return;
                }
            };
            let stick_name = display_name(&destination);
            let (progress_sender, progress_receiver) = async_channel::unbounded();
            let followed = stick_name.clone();
            let progress_notices = cx.spawn({
                let this = this.clone();
                async move |cx| {
                    while let Ok(progress) = progress_receiver.recv().await {
                        let text = progress_text(&followed, &progress);
                        if this
                            .update(cx, |this, cx| {
                                this.show_notice_for(NoticePlace::Window, text, EXPORTED_NOTICE_DURATION, cx);
                            })
                            .is_err()
                        {
                            return;
                        }
                    }
                }
            });
            let exported = cx
                .background_executor()
                .spawn(async move {
                    let rekordbox = match loaded_rekordbox {
                        Some(library) => Some(library),
                        None if needs_rekordbox => {
                            let library = RekordboxLibrary::load_default().map_err(|error| {
                                format!(
                                    "rekordbox could not be read for the tracks' grids and cues: {}",
                                    error_chain(&error)
                                )
                            })?;
                            Some(Arc::new(library))
                        }
                        None => None,
                    };
                    let library = ExportLibrary::new(
                        rekordbox.as_deref().map(RekordboxLibrary::collection),
                        &scans,
                        &relinks,
                    )
                    .with_track_cues(&track_cues);
                    let cache = Settings::analysis_cache_path()
                        .ok()
                        .and_then(|path| AnalysisCache::open(&path).ok());
                    let (playlists, problems) = playlists_from_set(&library, &set, &mut |path| {
                        cache.as_ref().and_then(|cache| cache.get(path).ok().flatten())
                    });
                    let today = local_today();
                    let mut report = export_to_usb(&destination, &playlists, today, &mut |progress| {
                        let _ = progress_sender.try_send(progress);
                    })
                    .map_err(|error| format!("The export to {} stopped: {error}", display_name(&destination)))?;
                    report.warnings.splice(0..0, problems);
                    Ok::<UsbExportReport, String>(report)
                })
                .await;
            drop(progress_notices);
            this.update(cx, |this, cx| {
                let text = match exported {
                    Ok(report) => exported_notice(&stick_name, &report),
                    Err(problem) => problem,
                };
                this.show_notice_for(NoticePlace::Window, text, EXPORTED_NOTICE_DURATION, cx);
            })
            .ok();
        })
        .detach();
    }
}

/// The name of the stick or folder, as Finder shows it.
fn display_name(destination: &Path) -> String {
    destination.file_name().map_or_else(
        || destination.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

fn progress_text(stick: &str, progress: &UsbExportProgress) -> String {
    match &progress.step {
        UsbExportStep::Track { title } => format!(
            "Exporting to {stick}: {} of {} tracks \u{b7} {title}",
            progress.done + 1,
            progress.total.saturating_sub(1)
        ),
        UsbExportStep::Library => format!("Exporting to {stick}: writing the library\u{2026}"),
    }
}

/// What the DJ reads after an export: what went where, and the first problem if any.
fn exported_notice(stick: &str, report: &UsbExportReport) -> String {
    let plural = if report.tracks == 1 { "" } else { "s" };
    let mut notice = format!(
        "Exported {} track{plural} to {stick} for CDJs ({} copied, {} already there).",
        report.tracks, report.copied, report.already_there
    );
    if let Some(first) = report.warnings.first() {
        notice.push(' ');
        notice.push_str(first);
        if report.warnings.len() > 1 {
            notice.push_str(&format!(" And {} more.", report.warnings.len() - 1));
        }
    }
    notice
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn the_notice_counts_the_tracks_and_says_what_went_wrong() {
        let report = UsbExportReport {
            destination: PathBuf::from("/Volumes/STICK"),
            tracks: 12,
            copied: 10,
            already_there: 2,
            playlists: vec!["Saturday".to_owned()],
            warnings: vec![
                "Uthando has no beat grid.".to_owned(),
                "Another.".to_owned(),
            ],
        };
        assert_eq!(
            exported_notice("STICK", &report),
            "Exported 12 tracks to STICK for CDJs (10 copied, 2 already there). Uthando has no \
             beat grid. And 1 more."
        );
    }

    #[test]
    fn progress_reads_track_by_track() {
        let progress = UsbExportProgress {
            done: 2,
            total: 13,
            step: UsbExportStep::Track {
                title: "Uthando".to_owned(),
            },
        };
        assert_eq!(
            progress_text("STICK", &progress),
            "Exporting to STICK: 3 of 12 tracks \u{b7} Uthando"
        );
        assert_eq!(display_name(Path::new("/Volumes/STICK")), "STICK");
    }
}
