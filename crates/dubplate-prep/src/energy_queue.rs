//! One worker thread that rates the tracks Dubplate is preparing.
//!
//! Set tracks are analysed before the shortlist and the shortlist before the
//! library. Every request is answered over a channel: from the cache when the
//! cached row is still fresh, from a fresh decode otherwise. The worker is the
//! only thing that touches the cache or decodes, and it is plain CPU work on
//! its own thread; the audio thread is never touched.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;

use crate::energy::TrackEnergy;
use crate::energy_cache::{EnergyCache, EnergyCacheError};

/// The order in which tracks are analysed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Priority {
    /// A track in the set being mixed; analysed first.
    Set,
    /// A track shortlisted for the set; after the set, before the library.
    Shortlist,
    /// A track of the library; analysed as everything else is done.
    #[default]
    Library,
}

impl Priority {
    /// How early this priority is analysed; a lower rank comes first.
    fn rank(self) -> u8 {
        match self {
            Priority::Set => 0,
            Priority::Shortlist => 1,
            Priority::Library => 2,
        }
    }

    /// Whether this priority is analysed no later than `other`.
    fn at_least(self, other: Priority) -> bool {
        self.rank() <= other.rank()
    }
}

/// The answer to one energy request.
#[derive(Debug)]
pub struct EnergyResult {
    /// The track the request was for.
    pub path: PathBuf,
    /// The rated energy, or an error message when the track could not be rated.
    pub outcome: Result<Option<TrackEnergy>, String>,
}

/// A track waiting to be analysed.
#[derive(Clone)]
struct QueuedTrack {
    path: PathBuf,
    bpm: Option<f32>,
    priority: Priority,
    sequence: u64,
}

/// The waiting list, plus the track the worker is currently on.
#[derive(Default)]
struct QueueState {
    pending: Vec<QueuedTrack>,
    claimed_path: Option<PathBuf>,
    claimed_priority: Priority,
    next_sequence: u64,
}

impl QueueState {
    /// The next track to analyse: the earliest priority first, the oldest
    /// request first inside a priority. The waiting list is empty otherwise.
    fn next_track(&mut self) -> Option<QueuedTrack> {
        let (index, _) = self
            .pending
            .iter()
            .enumerate()
            .min_by_key(|(_, queued)| (queued.priority.rank(), queued.sequence))?;
        let track = self.pending.swap_remove(index);
        self.claimed_path = Some(track.path.clone());
        self.claimed_priority = track.priority;
        Some(track)
    }

    /// Puts a request in the waiting list, or raises the priority of a track
    /// already waiting or in the worker's hands.
    fn accept(&mut self, request: QueuedTrack) {
        let claimed = self
            .claimed_path
            .as_ref()
            .is_some_and(|path| *path == request.path);
        let position = self
            .pending
            .iter()
            .position(|queued| queued.path == request.path);
        let push = |state: &mut Self, track: QueuedTrack| {
            let sequence = state.next_sequence;
            state.next_sequence += 1;
            state.pending.push(QueuedTrack { sequence, ..track });
        };
        match position {
            Some(index) => {
                let queued = &mut self.pending[index];
                if !queued.priority.at_least(request.priority) {
                    queued.priority = request.priority;
                }
            }
            None if claimed && !self.claimed_priority.at_least(request.priority) => {
                // The worker is on this track at a later priority already:
                // it goes back in the waiting list as soon as the worker is
                // done, so it is analysed again at the earlier priority.
                push(self, request);
            }
            None => {
                push(self, request);
            }
        }
    }
}

/// A queue of tracks for the energy worker, one worker per `EnergyQueue`.
///
/// Dropping the queue stops the worker after the track it is analysing: the
/// rest of the queue is dropped with it and the worker is joined before the
/// queue is gone, so no detached thread keeps running.
pub struct EnergyQueue {
    request_sender: Option<Sender<Vec<QueuedTrack>>>,
    results: Receiver<EnergyResult>,
    decoded: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl EnergyQueue {
    /// Starts the worker thread named `dubplate-energy` and returns the
    /// queue. The worker opens the cache at `cache_path` on its own thread.
    pub fn start(cache_path: PathBuf) -> Result<Self, EnergyCacheError> {
        let (request_sender, request_receiver) = channel();
        let (result_sender, results) = channel();
        let decoded = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let worker = std::thread::Builder::new()
            .name("dubplate-energy".to_owned())
            .spawn({
                let worker = Worker {
                    request_receiver,
                    result_sender,
                    decoded: Arc::clone(&decoded),
                    stop: Arc::clone(&stop),
                };
                move || worker.run(cache_path)
            })
            .map_err(|error| EnergyCacheError::Io(std::io::Error::other(error.to_string())))?;
        Ok(Self {
            worker: Some(worker),
            request_sender: Some(request_sender),
            results,
            decoded,
            stop,
        })
    }

    /// Queues the track at `path` to be analysed at `priority`.
    ///
    /// A path already queued is not queued twice; if the new priority is
    /// earlier, the track is analysed earlier instead.
    pub fn request(&self, path: PathBuf, bpm: Option<f32>, priority: Priority) {
        self.request_all([(path, bpm, priority)]);
    }

    /// Queues several tracks at once. The worker takes the whole batch before it picks the next
    /// track, so a batch is always analysed in priority order.
    pub fn request_all(
        &self,
        requests: impl IntoIterator<Item = (PathBuf, Option<f32>, Priority)>,
    ) {
        let batch: Vec<QueuedTrack> = requests
            .into_iter()
            .map(|(path, bpm, priority)| QueuedTrack {
                path,
                bpm,
                priority,
                sequence: 0,
            })
            .collect();
        if let Some(sender) = &self.request_sender {
            let _ = sender.send(batch);
        }
    }

    /// The end results arrive on this channel.
    pub fn results(&self) -> &Receiver<EnergyResult> {
        &self.results
    }

    /// How many tracks the worker decoded, so a test can tell a cached
    /// answer from a fresh decode without timing.
    pub fn decoded_count(&self) -> usize {
        self.decoded.load(Ordering::SeqCst)
    }
}

impl Drop for EnergyQueue {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.request_sender.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// The worker thread's ends of the channels.
struct Worker {
    request_receiver: Receiver<Vec<QueuedTrack>>,
    result_sender: Sender<EnergyResult>,
    decoded: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
}

impl Worker {
    /// Takes the earliest-priority track, answers it from the cache when fresh, otherwise decodes
    /// and rates it and stores the result. New requests are read between tracks, so a set track
    /// asked for while the library is being analysed comes next. Stops after the track in
    /// progress once the queue is dropped.
    fn run(self, cache_path: PathBuf) {
        let mut cache = match EnergyCache::open(&cache_path) {
            Ok(cache) => Some(cache),
            Err(error) => {
                self.answer_all_with(&error.to_string());
                return;
            }
        };
        let mut state = QueueState::default();
        loop {
            if self.stop.load(Ordering::SeqCst) {
                return;
            }
            if state.pending.is_empty() {
                match self.request_receiver.recv() {
                    Ok(batch) => batch.into_iter().for_each(|request| state.accept(request)),
                    Err(_) => return,
                }
            }
            for batch in self.request_receiver.try_iter() {
                batch.into_iter().for_each(|request| state.accept(request));
            }
            if let Some(track) = state.next_track() {
                answer_track(&track, &mut cache, &self.result_sender, &self.decoded);
            }
        }
    }

    /// The cache did not open: answers every request with `message` until the queue is dropped.
    fn answer_all_with(&self, message: &str) {
        for batch in self.request_receiver.iter() {
            for track in batch {
                let _ = self.result_sender.send(EnergyResult {
                    path: track.path,
                    outcome: Err(message.to_owned()),
                });
            }
        }
    }
}

/// The answer to one track: the cache row when it is fresh, otherwise a
/// fresh decode and rating, stored for the next launch.
fn answer_track(
    track: &QueuedTrack,
    cache: &mut Option<EnergyCache>,
    result_sender: &Sender<EnergyResult>,
    decoded: &Arc<AtomicUsize>,
) {
    let outcome = match cache.as_mut() {
        None => Ok(None),
        Some(cache) => match cache.get(&track.path) {
            Ok(Some(energy)) => Ok(Some(energy)),
            Ok(None) => decode_and_store(cache, track, decoded),
            Err(error) => Err(error.to_string()),
        },
    };
    let _ = result_sender.send(EnergyResult {
        path: track.path.clone(),
        outcome,
    });
}

/// Decodes and rates the track, then stores the result for the next launch.
fn decode_and_store(
    cache: &mut EnergyCache,
    track: &QueuedTrack,
    decoded: &Arc<AtomicUsize>,
) -> Result<Option<TrackEnergy>, String> {
    if std::fs::metadata(&track.path).is_err() {
        return Err(format!(
            "the file at {} is not readable",
            track.path.display()
        ));
    }
    decoded.fetch_add(1, Ordering::SeqCst);
    let energy = crate::energy::measure_track_energy(&track.path, track.bpm)
        .map_err(|error| error.to_string())?;
    if let Some(energy) = &energy {
        cache
            .put(&track.path, energy)
            .map_err(|error| error.to_string())?;
    }
    Ok(energy)
}
