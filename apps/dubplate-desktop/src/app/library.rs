//! The window's side of the library: choosing music folders, scanning them in the background,
//! importing rekordbox when the DJ asks, and rebuilding the collection the browser shows.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use dubplate_library::{FolderScan, LibraryCache};
use gpui::{Context, PathPromptOptions};

use super::{DubplateApp, NoticePlace, error_chain};
use crate::browser::LibraryStatus;
use crate::collection::{Collection, RekordboxLibrary, folder_name};
use crate::library::{RekordboxSource, ScanRequest};
use crate::prep;
use crate::relink::{missing_tracks, relink_map};
use crate::settings::Settings;

/// How often the scan progress is redrawn while a scan runs.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);

impl DubplateApp {
    /// Opens the library the DJ chose: the last scans from the cache at once, then rekordbox and
    /// a rescan of every folder in the background to pick up what changed.
    pub(super) fn start_library(&mut self, cx: &mut Context<'_, Self>) {
        if self.sources.is_unchosen() {
            self.show_library(LibraryStatus::Unchosen, cx);
            return;
        }
        let cache_path = Settings::library_cache_path().ok();
        cx.spawn(async move |this, cx| {
            let cache = cx
                .background_executor()
                .spawn(async move {
                    cache_path
                        .as_deref()
                        .map(LibraryCache::load)
                        .unwrap_or_default()
                })
                .await;
            this.update(cx, |this, cx| {
                this.sources.restore(cache.scans);
                if matches!(this.sources.rekordbox, RekordboxSource::Reading) {
                    this.read_rekordbox(cx);
                }
                this.rebuild_collection(cx);
                let folders = this.sources.folders().to_vec();
                this.scan_folders(folders, cx);
            })
            .ok();
        })
        .detach();
    }

    /// Opens the system's folder picker and adds the folders chosen to the library.
    pub(super) fn add_music_folders(&mut self, cx: &mut Context<'_, Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: Some("Add to Dubplate".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(folders))) = chosen.await else {
                return;
            };
            this.update(cx, |this, cx| {
                let added = this.sources.add(folders);
                if added.is_empty() {
                    return;
                }
                this.remember_sources(cx);
                this.scan_folders(added, cx);
                this.rebuild_collection(cx);
            })
            .ok();
        })
        .detach();
    }

    /// Drops a folder's tracks from the library. The files stay where they are.
    pub(super) fn remove_music_folder(&mut self, folder: &Path, cx: &mut Context<'_, Self>) {
        self.sources.remove(folder);
        self.remember_sources(cx);
        self.save_library_cache(cx);
        self.rebuild_collection(cx);
    }

    /// Scans a folder again.
    pub(super) fn rescan_music_folder(&mut self, folder: PathBuf, cx: &mut Context<'_, Self>) {
        self.scan_folders(vec![folder], cx);
    }

    /// Makes rekordbox's collection part of the library, or takes it out.
    pub(super) fn set_rekordbox_import(&mut self, import: bool, cx: &mut Context<'_, Self>) {
        self.settings.import_rekordbox = import;
        self.save_settings(cx);
        if import {
            self.sources.rekordbox = RekordboxSource::Reading;
            self.read_rekordbox(cx);
        } else {
            self.sources.rekordbox = RekordboxSource::Off;
        }
        self.rebuild_collection(cx);
    }

    /// Writes the chosen sources to the settings.
    fn remember_sources(&mut self, cx: &mut Context<'_, Self>) {
        self.settings.music_folders = self.sources.folders().to_vec();
        self.save_settings(cx);
    }

    fn read_rekordbox(&mut self, cx: &mut Context<'_, Self>) {
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { RekordboxLibrary::load_default() })
                .await;
            this.update(cx, |this, cx| {
                if !matches!(this.sources.rekordbox, RekordboxSource::Reading) {
                    return;
                }
                this.sources.rekordbox = match result {
                    Ok(library) => RekordboxSource::Ready(Arc::new(library)),
                    Err(error) => {
                        let problem = error_chain(&error);
                        this.show_notice(
                            NoticePlace::Window,
                            format!("rekordbox could not be read: {problem}"),
                            cx,
                        );
                        RekordboxSource::Failed(problem)
                    }
                };
                this.rebuild_collection(cx);
            })
            .ok();
        })
        .detach();
    }

    fn scan_folders(&mut self, folders: Vec<PathBuf>, cx: &mut Context<'_, Self>) {
        if let Some(request) = self.sources.request_scan(folders) {
            self.run_scan(request, cx);
        }
        cx.notify();
    }

    fn run_scan(&mut self, request: ScanRequest, cx: &mut Context<'_, Self>) {
        cx.spawn(async move |this, cx| {
            let results = cx
                .background_executor()
                .spawn(async move { request.run() })
                .await;
            this.update(cx, |this, cx| this.finish_scan(results, cx))
                .ok();
        })
        .detach();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(PROGRESS_INTERVAL).await;
                let scanning = this.update(cx, |this, cx| {
                    cx.notify();
                    this.sources.scanning().is_some()
                });
                if !matches!(scanning, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    fn finish_scan(
        &mut self,
        results: Vec<(PathBuf, Result<FolderScan, String>)>,
        cx: &mut Context<'_, Self>,
    ) {
        if let Some((folder, Err(problem))) = results.iter().find(|(_, result)| result.is_err()) {
            self.show_notice(
                NoticePlace::Window,
                format!("{} could not be scanned: {problem}", folder_name(folder)),
                cx,
            );
        }
        let next = self.sources.finish_scan(results);
        self.rebuild_collection(cx);
        self.save_library_cache(cx);
        if let Some(next) = next {
            self.run_scan(next, cx);
        }
    }

    fn save_library_cache(&self, cx: &mut Context<'_, Self>) {
        let Ok(path) = Settings::library_cache_path() else {
            return;
        };
        let cache = LibraryCache::new(self.sources.chosen_scans());
        cx.spawn(async move |this, cx| {
            let saved = cx
                .background_executor()
                .spawn(async move { cache.save(&path) })
                .await;
            if let Err(error) = saved {
                this.update(cx, |this, cx| {
                    this.show_notice(
                        NoticePlace::Window,
                        format!("The library could not be saved for next time: {error}"),
                        cx,
                    );
                })
                .ok();
            }
        })
        .detach();
    }

    /// Builds the collection again from the sources, on a background thread, and shows it.
    pub(super) fn rebuild_collection(&mut self, cx: &mut Context<'_, Self>) {
        self.library_generation += 1;
        let generation = self.library_generation;
        if self.sources.is_unchosen() {
            self.show_library(LibraryStatus::Unchosen, cx);
            return;
        }
        let rekordbox = match &self.sources.rekordbox {
            RekordboxSource::Ready(library) => Some(Arc::clone(library)),
            RekordboxSource::Off | RekordboxSource::Reading | RekordboxSource::Failed(_) => None,
        };
        let scans = self.sources.chosen_scans();
        let relinks = relink_map(&self.settings.relinks);
        cx.spawn(async move |this, cx| {
            let (collection, catalogue, missing) = cx
                .background_executor()
                .spawn(async move {
                    let collection =
                        Collection::build_relinked(rekordbox.as_deref(), &scans, &relinks);
                    let catalogue = prep::Catalogue::build(&collection);
                    let missing = missing_tracks(&collection, &relinks);
                    (collection, catalogue, missing)
                })
                .await;
            this.update(cx, |this, cx| {
                if this.library_generation == generation {
                    this.catalogue = Arc::new(catalogue);
                    this.restore_energy();
                    this.show_library(LibraryStatus::Ready(Arc::new(collection)), cx);
                    this.show_missing(missing, cx);
                    this.forget_track_checks();
                    this.set_changed(cx);
                    this.request_library_energy(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn show_library(&mut self, status: LibraryStatus, cx: &mut Context<'_, Self>) {
        self.library = status.clone();
        self.browser
            .update(cx, |browser, cx| browser.set_library(status, cx));
        cx.notify();
    }
}
