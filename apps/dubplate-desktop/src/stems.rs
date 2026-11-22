//! The window's view of every track's stems: asking for separations, keeping the status of
//! each track as the separation queue reports it, and the words the library and the set show.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dubplate_stems::{
    CurrentExecutable, SeparationPriority, SeparationQueue, StemsLocation, StemsStatus,
    VocalPresence,
};
use gpui::SharedString;

/// Every track's stems status, as the library and the set read it.
pub type StemStatuses = Arc<HashMap<PathBuf, StemsStatus>>;

/// The separation queue and what it reported, by track.
pub struct TrackStems {
    queue: Option<SeparationQueue>,
    statuses: StemStatuses,
    vocals: HashMap<PathBuf, VocalPresence>,
    set_tracks: HashSet<PathBuf>,
}

impl TrackStems {
    /// Starts the separation queue on Dubplate's stems folder, with workers that are this
    /// executable. Returns why it could not start, when it cannot: the decks then play the
    /// tracks as they are.
    pub fn start(location: StemsLocation) -> (Self, Option<String>) {
        let queue = CurrentExecutable::new(location.clone())
            .and_then(|launcher| SeparationQueue::start(location, launcher));
        match queue {
            Ok(queue) => (Self::with_queue(Some(queue)), None),
            Err(error) => (Self::with_queue(None), Some(error.to_string())),
        }
    }

    fn with_queue(queue: Option<SeparationQueue>) -> Self {
        Self {
            queue,
            statuses: Arc::default(),
            vocals: HashMap::new(),
            set_tracks: HashSet::new(),
        }
    }

    /// A view without a queue, for when separation cannot start: every track plays as it is.
    pub fn start_unavailable() -> Self {
        Self::with_queue(None)
    }

    /// Asks for the stems of these tracks, each at its priority.
    pub fn request(&mut self, requests: impl IntoIterator<Item = (PathBuf, SeparationPriority)>) {
        if let Some(queue) = &self.queue {
            queue.request_all(requests);
        }
    }

    /// Asks for the stems of the set's and the shortlist's tracks, and stops waiting for those
    /// asked for earlier that left both: a track the DJ asked for by hand is not touched.
    pub fn follow_set(&mut self, requests: Vec<(PathBuf, SeparationPriority)>) {
        let tracks: HashSet<PathBuf> = requests.iter().map(|(track, _)| track.clone()).collect();
        let left: Vec<PathBuf> = self
            .set_tracks
            .difference(&tracks)
            .filter(|track| self.status(track).is_pending())
            .cloned()
            .collect();
        for track in left {
            self.cancel(track);
        }
        self.set_tracks = tracks;
        self.request(requests);
    }

    /// Asks which of these tracks have stems already, without separating any.
    pub fn probe(&mut self, tracks: impl IntoIterator<Item = PathBuf>) {
        if let Some(queue) = &self.queue {
            queue.probe(tracks);
        }
    }

    /// Stops separating `track` and forgets it if it waits.
    pub fn cancel(&mut self, track: PathBuf) {
        if let Some(queue) = &self.queue {
            queue.cancel(track);
        }
    }

    /// Takes every update the queue sent since the last call, and returns the tracks whose
    /// stems became ready.
    pub fn collect(&mut self) -> Vec<PathBuf> {
        let Some(queue) = &self.queue else {
            return Vec::new();
        };
        let updates: Vec<_> = queue.updates().try_iter().collect();
        if updates.is_empty() {
            return Vec::new();
        }
        let statuses = Arc::make_mut(&mut self.statuses);
        let mut ready = Vec::new();
        for update in updates {
            if matches!(update.status, StemsStatus::Ready(_)) {
                ready.push(update.track.clone());
            }
            match update.status {
                StemsStatus::None => {
                    statuses.remove(&update.track);
                }
                status => {
                    statuses.insert(update.track, status);
                }
            }
        }
        ready
    }

    /// Whether a track waits for or is in a separation, so updates are still to come.
    pub fn is_busy(&self) -> bool {
        self.statuses.values().any(StemsStatus::is_pending)
    }

    /// The status of `track`'s stems.
    pub fn status(&self, track: &Path) -> &StemsStatus {
        self.statuses.get(track).unwrap_or(&StemsStatus::None)
    }

    /// Where the vocals sound near the start and end of `track`, once its stems are measured.
    pub fn vocals(&self, track: &Path) -> Option<VocalPresence> {
        self.vocals.get(track).copied()
    }

    /// Remembers where the vocals of `track` sound.
    pub fn set_vocals(&mut self, track: PathBuf, vocals: VocalPresence) {
        self.vocals.insert(track, vocals);
    }

    /// Every status, to hand to the library.
    pub fn statuses(&self) -> StemStatuses {
        Arc::clone(&self.statuses)
    }
}

/// The few characters the library's Stems column and the set rows show for a status, in the
/// quiet style of the Energy column: blank without stems, the share separated while
/// separating, and a mark once they are ready.
pub fn stems_cell(status: &StemsStatus) -> SharedString {
    match status {
        StemsStatus::None => SharedString::default(),
        StemsStatus::Queued => "…".into(),
        StemsStatus::Separating(progress) => format!("{:.0}%", progress.overall() * 100.0).into(),
        StemsStatus::Ready(_) => "ready".into(),
        StemsStatus::Failed(_) => "–".into(),
    }
}

#[cfg(test)]
mod tests {
    use dubplate_stems::{SeparationProgress, SeparationStage};

    use super::*;

    #[test]
    fn the_cell_says_little_and_never_a_word_too_long_for_the_column() {
        assert_eq!(stems_cell(&StemsStatus::None), "");
        assert_eq!(stems_cell(&StemsStatus::Queued), "…");
        assert_eq!(
            stems_cell(&StemsStatus::Separating(SeparationProgress::new(
                SeparationStage::Separating,
                37,
                100
            ))),
            "37%"
        );
        assert_eq!(stems_cell(&StemsStatus::Failed("broken".into())), "–");
    }

    #[test]
    fn without_a_queue_every_track_has_no_stems() {
        let mut stems = TrackStems::start_unavailable();
        stems.request([(PathBuf::from("/a.m4a"), SeparationPriority::Now)]);
        assert!(stems.collect().is_empty());
        assert_eq!(stems.status(Path::new("/a.m4a")), &StemsStatus::None);
        assert!(!stems.is_busy());
    }
}
