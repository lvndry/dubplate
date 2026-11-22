//! Export to rekordbox: the set written as a rekordbox XML file where the DJ chooses in the
//! system's save dialog, then a notice saying how to bring it into rekordbox.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use dubplate_prep::{
    ExportLibrary, REKORDBOX_IMPORT_STEPS, SetExport, default_export_folder, export_file_name,
};
use gpui::Context;

use super::notice::NoticePlace;
use super::{DubplateApp, error_chain};
use crate::collection::RekordboxLibrary;
use crate::library::RekordboxSource;

/// How long the notice after an export stays, long enough to read the steps into rekordbox.
const EXPORTED_NOTICE_DURATION: Duration = Duration::from_secs(20);
/// The extension rekordbox's file chooser looks for.
const XML_EXTENSION: &str = "xml";

impl DubplateApp {
    /// Asks where to save the set as rekordbox XML, starting in `~/Music/Dubplate Exports`, and
    /// writes it there in the background.
    pub(super) fn export_set(&mut self, cx: &mut Context<'_, Self>) {
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
        let folder = default_export_folder()
            .filter(|folder| std::fs::create_dir_all(folder).is_ok())
            .or_else(dirs::home_dir)
            .unwrap_or_default();
        let chosen = cx.prompt_for_new_path(&folder, Some(&export_file_name(&set.name)));
        let loaded_rekordbox = match &self.sources.rekordbox {
            RekordboxSource::Ready(library) => Some(Arc::clone(library)),
            RekordboxSource::Off | RekordboxSource::Reading | RekordboxSource::Failed(_) => None,
        };
        let needs_rekordbox = set.entries.iter().any(|entry| entry.rekordbox_id.is_some());
        let scans = self.sources.chosen_scans();
        let relinks = self.settings.library.relink_map();
        cx.spawn(async move |this, cx| {
            let path = match chosen.await {
                Ok(Ok(Some(path))) => with_extension(path, XML_EXTENSION),
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(error)) => {
                    this.update(cx, |this, cx| {
                        this.show_notice(
                            NoticePlace::Window,
                            format!("The save dialog could not open: {error}"),
                            cx,
                        );
                    })
                    .ok();
                    return;
                }
            };
            let written = cx
                .background_executor()
                .spawn(async move {
                    let rekordbox = match loaded_rekordbox {
                        Some(library) => Some(library),
                        None if needs_rekordbox => {
                            let library = RekordboxLibrary::load_default().map_err(|error| {
                                format!(
                                    "rekordbox could not be read for the set's grids and cues: {}",
                                    error_chain(&error)
                                )
                            })?;
                            Some(Arc::new(library))
                        }
                        None => None,
                    };
                    let export = ExportLibrary::new(
                        rekordbox.as_deref().map(RekordboxLibrary::collection),
                        &scans,
                        &relinks,
                    )
                    .export(&set);
                    export
                        .write_to(&path)
                        .map_err(|error| format!("The set could not be exported: {error}"))?;
                    Ok::<_, String>((export, path))
                })
                .await;
            this.update(cx, |this, cx| match written {
                Ok((export, path)) => this.show_notice_for(
                    NoticePlace::Window,
                    exported_notice(&export, &path),
                    EXPORTED_NOTICE_DURATION,
                    cx,
                ),
                Err(problem) => this.show_notice(NoticePlace::Window, problem, cx),
            })
            .ok();
        })
        .detach();
    }
}

/// `path`, with `.extension` added when the DJ typed a name without it.
pub(super) fn with_extension(path: PathBuf, extension: &str) -> PathBuf {
    let has_extension = path
        .extension()
        .is_some_and(|existing| existing.eq_ignore_ascii_case(extension));
    if has_extension {
        return path;
    }
    let mut name = path.into_os_string();
    name.push(".");
    name.push(extension);
    PathBuf::from(name)
}

/// What the DJ reads after an export: how many tracks went where, the first problem if any,
/// and the steps into rekordbox.
fn exported_notice(export: &SetExport, path: &Path) -> String {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let plural = if export.track_count == 1 { "" } else { "s" };
    let mut notice = format!(
        "Exported {} track{plural} to {file_name}.",
        export.track_count
    );
    if let Some(first) = export.problems.first() {
        notice.push(' ');
        notice.push_str(first);
        if export.problems.len() > 1 {
            notice.push_str(&format!(" And {} more.", export.problems.len() - 1));
        }
    }
    notice.push(' ');
    notice.push_str(REKORDBOX_IMPORT_STEPS);
    notice
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_without_the_extension_gets_it() {
        assert_eq!(
            with_extension(PathBuf::from("/Exports/Friday"), XML_EXTENSION),
            PathBuf::from("/Exports/Friday.xml")
        );
        assert_eq!(
            with_extension(PathBuf::from("/Exports/Fri.day"), XML_EXTENSION),
            PathBuf::from("/Exports/Fri.day.xml")
        );
        assert_eq!(
            with_extension(PathBuf::from("/Exports/Friday.XML"), XML_EXTENSION),
            PathBuf::from("/Exports/Friday.XML")
        );
        assert_eq!(
            with_extension(PathBuf::from("/Exports/Friday tracklist"), "csv"),
            PathBuf::from("/Exports/Friday tracklist.csv")
        );
    }

    #[test]
    fn the_notice_counts_the_tracks_and_says_what_went_wrong() {
        let export = SetExport {
            xml: String::new(),
            track_count: 12,
            new_to_rekordbox: 2,
            problems: vec![
                "One is not at /a.".to_owned(),
                "Two is not at /b.".to_owned(),
            ],
        };
        let notice = exported_notice(&export, Path::new("/Exports/Friday.xml"));
        assert!(
            notice.starts_with("Exported 12 tracks to Friday.xml. One is not at /a. And 1 more."),
            "{notice}"
        );
        assert!(notice.ends_with(REKORDBOX_IMPORT_STEPS));
    }
}
