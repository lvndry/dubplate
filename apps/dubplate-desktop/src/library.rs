//! Where the library comes from: the music folders the DJ chose, scanned off the interface and
//! audio threads, and rekordbox's collection when the DJ chose to import it. Nothing is read
//! until the DJ chooses.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

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
    /// The folders it scans.
    pub folders: Vec<PathBuf>,
    /// The audio files it has looked at so far.
    pub files_seen: Arc<AtomicUsize>,
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
    queued: VecDeque<PathBuf>,
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
        self.queued.retain(|queued| queued != folder);
    }

    /// Asks for `folders` to be scanned: at once when no scan runs, else after it. Returns the
    /// scan to start, if one should start now.
    pub fn request_scan(&mut self, folders: Vec<PathBuf>) -> Option<ScanRequest> {
        for folder in folders {
            if !self.queued.contains(&folder) {
                self.queued.push_back(folder);
            }
        }
        self.next_scan()
    }

    /// The scan to start now, if none runs and some folders wait.
    fn next_scan(&mut self) -> Option<ScanRequest> {
        if self.scanning.is_some() || self.queued.is_empty() {
            return None;
        }
        let folders: Vec<PathBuf> = self.queued.drain(..).collect();
        let previous = folders
            .iter()
            .filter_map(|folder| self.scans.get(folder).cloned())
            .collect();
        let files_seen = Arc::new(AtomicUsize::new(0));
        self.scanning = Some(ScanJob {
            folders: folders.clone(),
            files_seen: Arc::clone(&files_seen),
        });
        Some(ScanRequest {
            folders,
            previous,
            files_seen,
        })
    }

    /// Takes what a scan found, keeping only the folders still chosen, and returns the next
    /// scan to start, if folders were asked for meanwhile.
    pub fn finish_scan(
        &mut self,
        results: Vec<(PathBuf, Result<FolderScan, String>)>,
    ) -> Option<ScanRequest> {
        self.scanning = None;
        for (folder, result) in results {
            if !self.folders.contains(&folder) {
                continue;
            }
            match result {
                Ok(scan) => {
                    self.failures.remove(&folder);
                    self.scans.insert(folder, scan);
                }
                Err(reason) => {
                    self.failures.insert(folder, reason);
                }
            }
        }
        self.next_scan()
    }

    /// The scan running now, if any.
    pub fn scanning(&self) -> Option<&ScanJob> {
        self.scanning.as_ref()
    }

    /// Whether `folder` is being scanned or waits to be.
    pub fn is_scanning(&self, folder: &Path) -> bool {
        self.queued.iter().any(|queued| queued == folder)
            || self
                .scanning
                .as_ref()
                .is_some_and(|job| job.folders.iter().any(|scanned| scanned == folder))
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
    /// The folders to scan.
    pub folders: Vec<PathBuf>,
    /// Their previous scans, so files that did not change are not read again.
    pub previous: Vec<FolderScan>,
    /// The files looked at so far.
    pub files_seen: Arc<AtomicUsize>,
}

impl ScanRequest {
    /// Scans every folder. Disk work: run it off the interface and audio threads.
    pub fn run(self) -> Vec<(PathBuf, Result<FolderScan, String>)> {
        self.folders
            .into_iter()
            .map(|folder| {
                let previous = self.previous.iter().find(|scan| scan.folder == folder);
                let result = dubplate_library::scan_folder(&folder, previous, &self.files_seen)
                    .map_err(|error| error.to_string());
                (folder, result)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(folder: &str) -> FolderScan {
        FolderScan {
            folder: PathBuf::from(folder),
            tracks: Vec::new(),
            problems: Vec::new(),
            scanned_at_ms: 1,
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
        assert_eq!(first.folders.len(), 2);
        assert!(sources.is_scanning(Path::new("/A")));
        assert!(sources.request_scan(vec![PathBuf::from("/A")]).is_none());
        let next = sources
            .finish_scan(vec![
                (PathBuf::from("/A"), Ok(scan("/A"))),
                (PathBuf::from("/B"), Err("unplugged".to_owned())),
            ])
            .unwrap();
        assert_eq!(next.folders, [PathBuf::from("/A")]);
        assert_eq!(next.previous.len(), 1, "a rescan starts from the last scan");
        assert_eq!(sources.failure(Path::new("/B")), Some("unplugged"));
        assert_eq!(sources.chosen_scans().len(), 1);
    }

    #[test]
    fn a_removed_folder_drops_its_tracks_even_when_its_scan_comes_back_later() {
        let mut sources = LibrarySources::new(vec![PathBuf::from("/A")], false);
        sources.restore(vec![scan("/A"), scan("/Forgotten")]);
        assert_eq!(sources.chosen_scans().len(), 1);
        let request = sources.request_scan(vec![PathBuf::from("/A")]).unwrap();
        sources.remove(Path::new("/A"));
        assert!(
            sources
                .finish_scan(vec![(request.folders[0].clone(), Ok(scan("/A")))])
                .is_none()
        );
        assert!(sources.chosen_scans().is_empty());
        assert!(sources.is_unchosen());
    }
}
