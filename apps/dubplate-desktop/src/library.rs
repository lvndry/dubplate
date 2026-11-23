//! Where the library comes from: the music folders the DJ chose, scanned off the interface and
//! audio threads, and rekordbox's collection when the DJ chose to import it. Nothing is read
//! until the DJ chooses.

use std::collections::{HashMap, VecDeque};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use dubplate_library::FolderScan;

use crate::collection::RekordboxLibrary;

/// Where rekordbox's collection stands.
#[derive(Debug, Clone)]
pub enum RekordboxSource {
    /// The DJ has not chosen to import it.
    Off,
    /// Being copied and read.
    Reading,
    /// Read.
    Ready(Arc<RekordboxLibrary>),
    /// Could not be read, and why.
    Failed(String),
}

/// A scan running in the background.
#[derive(Debug)]
pub struct ScanJob {
    targets: Vec<ScanTarget>,
    /// The audio files it has looked at so far.
    pub files_seen: Arc<AtomicUsize>,
}

/// A directory to read and the chosen roots whose caches it refreshes.
#[derive(Debug, Clone)]
struct ScanTarget {
    folder: PathBuf,
    roots: Vec<PathBuf>,
}

/// The music folders, their last scans and rekordbox, and the scans waiting to run.
#[derive(Debug)]
pub struct LibrarySources {
    folders: Vec<PathBuf>,
    scans: HashMap<PathBuf, FolderScan>,
    failures: HashMap<PathBuf, String>,
    /// rekordbox's collection.
    pub rekordbox: RekordboxSource,
    scanning: Option<ScanJob>,
    queued: VecDeque<ScanTarget>,
}

impl LibrarySources {
    /// The sources the DJ chose: `folders` in the order they were added, and rekordbox when
    /// `import_rekordbox` is on.
    pub fn new(folders: Vec<PathBuf>, import_rekordbox: bool) -> Self {
        Self {
            folders,
            scans: HashMap::new(),
            failures: HashMap::new(),
            rekordbox: if import_rekordbox {
                RekordboxSource::Reading
            } else {
                RekordboxSource::Off
            },
            scanning: None,
            queued: VecDeque::new(),
        }
    }

    /// The music folders, in the order they were added.
    pub fn folders(&self) -> &[PathBuf] {
        &self.folders
    }

    /// Whether the DJ has chosen nothing for the library yet.
    pub fn is_unchosen(&self) -> bool {
        self.folders.is_empty() && matches!(self.rekordbox, RekordboxSource::Off)
    }

    /// The last scan of `folder`, if it was scanned.
    pub fn scan(&self, folder: &Path) -> Option<&FolderScan> {
        self.scans.get(folder)
    }

    /// Why `folder` could not be scanned the last time, if it could not.
    pub fn failure(&self, folder: &Path) -> Option<&str> {
        self.failures.get(folder).map(String::as_str)
    }

    /// The scans of the chosen folders, in their order, for building the collection.
    pub fn chosen_scans(&self) -> Vec<FolderScan> {
        self.folders
            .iter()
            .filter_map(|folder| self.scans.get(folder).cloned())
            .collect()
    }

    /// Takes the scans read from the cache, for the folders still chosen.
    pub fn restore(&mut self, scans: Vec<FolderScan>) {
        for scan in scans {
            if self.folders.contains(&scan.folder) {
                self.scans.insert(scan.folder.clone(), scan);
            }
        }
    }

    /// Adds the folders not already chosen and returns them, to be scanned. A folder already
    /// inside a chosen one is added too: its crate shows that part of the music.
    pub fn add(&mut self, folders: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
        let mut added = Vec::new();
        for folder in folders {
            if !self.folders.contains(&folder) {
                self.folders.push(folder.clone());
                added.push(folder);
            }
        }
        added
    }

    /// Drops `folder` and its tracks from the library. The files stay where they are.
    pub fn remove(&mut self, folder: &Path) {
        self.folders.retain(|chosen| chosen != folder);
        self.scans.remove(folder);
        self.failures.remove(folder);
        self.queued.retain_mut(|target| {
            target.roots.retain(|root| root != folder);
            !target.roots.is_empty()
        });
        if let Some(job) = &mut self.scanning {
            for target in &mut job.targets {
                target.roots.retain(|root| root != folder);
            }
        }
    }

    /// Asks for `folders` to be scanned: at once when no scan runs, else after it. Returns the
    /// scan to start, if one should start now.
    pub fn request_scan(&mut self, folders: Vec<PathBuf>) -> Option<ScanRequest> {
        for folder in folders {
            if self.folders.contains(&folder) {
                self.queue_scan(folder.clone(), vec![folder]);
            }
        }
        self.next_scan()
    }

    /// Rescans one sidebar folder, updating its subtree in each covering root. Chosen roots
    /// inside the subtree receive their own full scan, just as they do on startup.
    pub fn request_subfolder_scan(&mut self, folder: PathBuf) -> Option<ScanRequest> {
        let roots: Vec<_> = self
            .folders
            .iter()
            .filter(|root| is_within(&folder, root))
            .cloned()
            .collect();
        if !roots.is_empty() {
            let descendants: Vec<_> = self
                .folders
                .iter()
                .filter(|root| **root != folder && is_within(root, &folder))
                .cloned()
                .collect();
            self.queue_scan(folder, roots);
            return self.request_scan(descendants);
        }
        self.next_scan()
    }

    fn queue_scan(&mut self, folder: PathBuf, roots: Vec<PathBuf>) {
        if let Some(target) = self
            .queued
            .iter_mut()
            .find(|target| target.folder == folder)
        {
            for root in roots {
                if !target.roots.contains(&root) {
                    target.roots.push(root);
                }
            }
        } else {
            self.queued.push_back(ScanTarget { folder, roots });
        }
    }

    /// The scan to start now, if none runs and some folders wait.
    fn next_scan(&mut self) -> Option<ScanRequest> {
        if self.scanning.is_some() || self.queued.is_empty() {
            return None;
        }
        let targets: Vec<_> = self.queued.drain(..).collect();
        let previous = self
            .scans
            .iter()
            .filter(|(root, _)| targets.iter().any(|target| target.roots.contains(root)))
            .map(|(_, scan)| scan.clone())
            .collect();
        let files_seen = Arc::new(AtomicUsize::new(0));
        self.scanning = Some(ScanJob {
            targets: targets.clone(),
            files_seen: Arc::clone(&files_seen),
        });
        Some(ScanRequest {
            targets,
            previous,
            files_seen,
        })
    }

    /// Takes what a scan found, keeping only the folders still chosen, and returns the next
    /// scan to start, if folders were asked for meanwhile.
    pub fn finish_scan(&mut self, results: Vec<ScanResult>) -> Option<ScanRequest> {
        if let Some(job) = self.scanning.take() {
            for ScanResult {
                folder,
                result,
                scope,
            } in results
            {
                let Some(target) = job.targets.iter().find(|target| target.folder == folder) else {
                    continue;
                };
                match result {
                    Ok(scan) => {
                        for root in target.roots.iter().filter(|root| **root != folder) {
                            if let Some(previous) = self.scans.get_mut(root) {
                                replace_subtree(previous, &scan, &scope);
                            }
                        }
                        if target.roots.contains(&folder) {
                            self.failures.remove(&folder);
                            self.scans.insert(folder, scan);
                        }
                    }
                    Err(reason) if target.roots.contains(&folder) => {
                        self.failures.insert(folder, reason);
                    }
                    Err(_) => {}
                }
            }
        }
        self.next_scan()
    }

    /// Whether the result still belongs to a root that has stayed chosen during the scan.
    pub fn is_current_scan_result(&self, folder: &Path) -> bool {
        self.scanning.as_ref().is_some_and(|job| {
            job.targets
                .iter()
                .any(|target| target.folder == folder && !target.roots.is_empty())
        })
    }

    /// The scan running now, if any.
    pub fn scanning(&self) -> Option<&ScanJob> {
        self.scanning.as_ref()
    }

    /// Whether `folder` is being scanned or waits to be.
    pub fn is_scanning(&self, folder: &Path) -> bool {
        self.queued
            .iter()
            .chain(self.scanning.iter().flat_map(|job| &job.targets))
            .any(|target| {
                !target.roots.is_empty()
                    && (is_within(folder, &target.folder)
                        || target.roots.iter().any(|root| root == folder))
            })
    }

    /// The audio files the running scan has looked at.
    pub fn files_seen(&self) -> Option<usize> {
        self.scanning
            .as_ref()
            .map(|job| job.files_seen.load(Ordering::Relaxed))
    }
}

/// What a background scan needs: the folders, what was found in them before, and the counter
/// it raises as it goes.
#[derive(Debug)]
pub struct ScanRequest {
    targets: Vec<ScanTarget>,
    /// Their previous scans, so files that did not change are not read again.
    previous: Vec<FolderScan>,
    /// The files looked at so far.
    files_seen: Arc<AtomicUsize>,
}

/// What one requested scan found, including the resolved path used for cached tracks.
#[derive(Debug)]
pub struct ScanResult {
    /// The requested path, as shown in the sidebar.
    pub folder: PathBuf,
    /// The scan, or the reason the folder could not be read.
    pub result: Result<FolderScan, String>,
    scope: PathBuf,
}

impl ScanRequest {
    /// Scans every folder. Disk work: run it off the interface and audio threads.
    pub fn run(self) -> Vec<ScanResult> {
        self.targets
            .into_iter()
            .map(|ScanTarget { folder, roots }| {
                let previous = self
                    .previous
                    .iter()
                    .filter(|scan| roots.contains(&scan.folder))
                    .max_by_key(|scan| scan.folder.components().count());
                let scope = resolved_path(&folder);
                let result = dubplate_library::scan_folder(&folder, previous, &self.files_seen)
                    .or_else(|error| {
                        if matches!(error, dubplate_library::ScanError::NotAFolder(_))
                            && deleted_subfolder(&folder, &roots)
                        {
                            Ok(FolderScan {
                                folder: folder.clone(),
                                tracks: Vec::new(),
                                problems: Vec::new(),
                                scanned_at_ms: SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_millis()
                                    as u64,
                            })
                        } else {
                            Err(error)
                        }
                    })
                    .map_err(|error| error.to_string());
                ScanResult {
                    folder,
                    result,
                    scope,
                }
            })
            .collect()
    }
}

fn is_within(path: &Path, root: &Path) -> bool {
    path.strip_prefix(root).is_ok_and(|relative| {
        relative
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    })
}

/// Scanned tracks use canonical paths. A removed directory still needs a canonical prefix
/// when, for example, the configured root was chosen through a symbolic link.
fn resolved_path(path: &Path) -> PathBuf {
    path.ancestors()
        .find_map(|ancestor| {
            let canonical = ancestor.canonicalize().ok()?;
            Some(canonical.join(path.strip_prefix(ancestor).ok()?))
        })
        .unwrap_or_else(|| path.to_path_buf())
}

fn deleted_subfolder(folder: &Path, roots: &[PathBuf]) -> bool {
    !roots.iter().any(|root| root == folder)
        && std::fs::symlink_metadata(folder)
            .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
        && roots.iter().any(|root| std::fs::read_dir(root).is_ok())
}

fn replace_subtree(previous: &mut FolderScan, scan: &FolderScan, scope: &Path) {
    previous
        .tracks
        .retain(|track| !track.path.starts_with(scope));
    previous.tracks.extend(
        scan.tracks
            .iter()
            .filter(|track| track.path.starts_with(scope))
            .cloned(),
    );
    previous
        .tracks
        .sort_by(|first, second| first.path.cmp(&second.path));
    previous
        .problems
        .retain(|problem| !problem.path.starts_with(&scan.folder));
    previous.problems.extend(scan.problems.iter().cloned());
    previous
        .problems
        .sort_by(|first, second| first.path.cmp(&second.path));
    previous.scanned_at_ms = scan.scanned_at_ms;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_track(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut wave = hound::WavWriter::create(
            path,
            hound::WavSpec {
                channels: 1,
                sample_rate: 44_100,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        wave.write_sample(0_i16).unwrap();
        wave.finalize().unwrap();
    }

    fn track_names(scan: &FolderScan) -> Vec<&str> {
        scan.tracks
            .iter()
            .map(|track| track.path.file_name().unwrap().to_str().unwrap())
            .collect()
    }

    fn finish(sources: &mut LibrarySources, request: ScanRequest) {
        let results = request.run();
        assert!(
            results.iter().all(|result| result.result.is_ok()),
            "{results:?}"
        );
        assert!(sources.finish_scan(results).is_none());
    }

    fn scan_with_result(folder: &str, result: Result<(), &str>) -> ScanResult {
        ScanResult {
            folder: PathBuf::from(folder),
            result: result
                .map(|()| FolderScan {
                    folder: PathBuf::from(folder),
                    tracks: Vec::new(),
                    problems: Vec::new(),
                    scanned_at_ms: 1,
                })
                .map_err(str::to_owned),
            scope: PathBuf::from(folder),
        }
    }

    #[test]
    fn nothing_is_chosen_until_the_dj_chooses() {
        let sources = LibrarySources::new(Vec::new(), false);
        assert!(sources.is_unchosen());
        assert!(!LibrarySources::new(Vec::new(), true).is_unchosen());
        assert!(!LibrarySources::new(vec![PathBuf::from("/Music")], false).is_unchosen());
    }

    #[test]
    fn scans_run_one_at_a_time_and_later_requests_wait() {
        let mut sources = LibrarySources::new(Vec::new(), false);
        let added = sources.add([
            PathBuf::from("/A"),
            PathBuf::from("/B"),
            PathBuf::from("/A"),
        ]);
        assert_eq!(added, [PathBuf::from("/A"), PathBuf::from("/B")]);
        let first = sources.request_scan(added).unwrap();
        assert_eq!(first.targets.len(), 2);
        assert!(sources.is_scanning(Path::new("/A")));
        assert!(sources.request_scan(vec![PathBuf::from("/A")]).is_none());
        let next = sources
            .finish_scan(vec![
                scan_with_result("/A", Ok(())),
                scan_with_result("/B", Err("unplugged")),
            ])
            .unwrap();
        assert_eq!(next.targets.len(), 1);
        assert_eq!(next.targets[0].folder, Path::new("/A"));
        assert_eq!(next.previous.len(), 1, "a rescan starts from the last scan");
        assert_eq!(sources.failure(Path::new("/B")), Some("unplugged"));
        assert_eq!(sources.chosen_scans().len(), 1);
    }

    #[test]
    fn a_removed_folder_drops_its_tracks_even_when_its_scan_comes_back_later() {
        let mut sources = LibrarySources::new(vec![PathBuf::from("/A")], false);
        sources.restore(vec![
            scan_with_result("/A", Ok(())).result.unwrap(),
            scan_with_result("/Forgotten", Ok(())).result.unwrap(),
        ]);
        assert_eq!(sources.chosen_scans().len(), 1);
        sources.request_scan(vec![PathBuf::from("/A")]).unwrap();
        sources.remove(Path::new("/A"));
        assert!(
            sources
                .finish_scan(vec![scan_with_result("/A", Ok(()))])
                .is_none()
        );
        assert!(sources.chosen_scans().is_empty());
        assert!(sources.is_unchosen());
    }

    #[test]
    fn subfolder_scan_replaces_only_its_tracks_and_problems_in_every_covering_root() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().to_path_buf();
        let parent = root.join("Parent");
        let target = parent.join("Deep");
        let sibling = parent.join("Deeper");
        for track in [target.join("Old.wav"), sibling.join("Sibling.wav")] {
            write_track(&track);
        }
        for problem in [target.join("Old.mp3"), sibling.join("Sibling.mp3")] {
            std::fs::write(problem, b"not audio").unwrap();
        }
        let roots = vec![root.clone(), parent.clone()];
        let mut sources = LibrarySources::new(roots.clone(), false);
        let initial = sources.request_scan(roots.clone()).unwrap();
        finish(&mut sources, initial);
        std::fs::remove_file(target.join("Old.wav")).unwrap();
        std::fs::remove_file(target.join("Old.mp3")).unwrap();
        std::fs::remove_file(sibling.join("Sibling.wav")).unwrap();
        std::fs::remove_file(sibling.join("Sibling.mp3")).unwrap();
        write_track(&target.join("New.wav"));
        std::fs::write(target.join("Broken.mp3"), b"still not audio").unwrap();

        let request = sources.request_subfolder_scan(target.clone()).unwrap();
        assert!(sources.is_scanning(&target));
        assert!(!sources.is_scanning(&sibling));
        assert_eq!(request.targets.len(), 1);
        finish(&mut sources, request);

        assert_eq!(sources.folders(), roots);
        assert!(sources.scan(&target).is_none());
        for root in roots {
            let scan = sources.scan(&root).unwrap();
            assert_eq!(track_names(scan), ["New.wav", "Sibling.wav"]);
            let problems: Vec<_> = scan.problems.iter().map(|problem| &problem.path).collect();
            assert_eq!(
                problems,
                [&target.join("Broken.mp3"), &sibling.join("Sibling.mp3")]
            );
        }
    }

    #[test]
    fn a_deleted_subfolder_is_pruned_but_a_missing_root_keeps_its_cache() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("Library");
        let target = root.join("Deleted");
        write_track(&target.join("Old.wav"));
        write_track(&root.join("Kept.wav"));
        let mut sources = LibrarySources::new(vec![root.clone()], false);
        let initial = sources.request_scan(vec![root.clone()]).unwrap();
        finish(&mut sources, initial);
        std::fs::remove_dir_all(&target).unwrap();

        let request = sources.request_subfolder_scan(target.clone()).unwrap();
        finish(&mut sources, request);
        assert_eq!(track_names(sources.scan(&root).unwrap()), ["Kept.wav"]);

        std::fs::remove_dir_all(&root).unwrap();
        let results = sources.request_subfolder_scan(target).unwrap().run();
        assert!(results[0].result.is_err());
        assert!(sources.finish_scan(results).is_none());
        assert_eq!(track_names(sources.scan(&root).unwrap()), ["Kept.wav"]);

        let results = sources.request_subfolder_scan(root.clone()).unwrap().run();
        assert!(results[0].result.is_err());
        assert!(sources.finish_scan(results).is_none());
        assert!(sources.failure(&root).is_some());
    }

    #[test]
    fn a_subtree_rescan_also_refreshes_a_separately_chosen_root_within_it() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().to_path_buf();
        let target = root.join("Selected");
        let chosen = target.join("Other Root");
        write_track(&chosen.join("Old.wav"));
        write_track(&root.join("Sibling.wav"));
        let roots = vec![root.clone(), chosen.clone()];
        let mut sources = LibrarySources::new(roots.clone(), false);
        let initial = sources.request_scan(roots).unwrap();
        finish(&mut sources, initial);
        std::fs::remove_file(chosen.join("Old.wav")).unwrap();
        std::fs::remove_file(root.join("Sibling.wav")).unwrap();
        write_track(&chosen.join("New.wav"));

        let request = sources.request_subfolder_scan(target).unwrap();
        finish(&mut sources, request);

        assert_eq!(
            track_names(sources.scan(&root).unwrap()),
            ["New.wav", "Sibling.wav"]
        );
        assert_eq!(track_names(sources.scan(&chosen).unwrap()), ["New.wav"]);
    }

    #[test]
    fn queued_subfolder_scan_uses_the_latest_cache_and_removed_targets_are_discarded() {
        let root = PathBuf::from("/A");
        let target = root.join("Nested");
        let mut sources = LibrarySources::new(vec![root.clone()], false);
        sources.request_scan(vec![root.clone()]).unwrap();
        assert!(sources.request_subfolder_scan(target.clone()).is_none());
        assert!(sources.request_subfolder_scan(target.clone()).is_none());
        let request = sources
            .finish_scan(vec![scan_with_result("/A", Ok(()))])
            .unwrap();
        assert_eq!(request.targets.len(), 1);
        assert_eq!(request.previous.len(), 1);
        assert!(sources.is_current_scan_result(&target));
        sources.remove(&root);
        sources.add([root.clone()]);
        assert!(!sources.is_current_scan_result(&target));
        assert!(!sources.is_scanning(&target));
        assert!(sources.finish_scan(request.run()).is_none());
        assert!(sources.chosen_scans().is_empty());
    }
}
