//! One worker thread that analyses the tracks Dubplate is preparing: tempo, beat grid, key and
//! energy, from one decode per track.
//!
//! Set tracks are analysed before the shortlist and the shortlist before the library. Every
//! request is answered over a channel: from the cache when the cached row is still fresh, from
//! a fresh decode otherwise. The worker is the only thing that touches the cache or decodes,
//! and it is plain CPU work on its own thread; the audio thread is never touched.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;

use crate::analysis::{AudioAnalysis, analyse_track};
use crate::analysis_cache::{AnalysisCache, AnalysisCacheError};

/// The order in which tracks are analysed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Priority {
    /// A track in the set being mixed, or on a deck; analysed first.
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

/// The answer to one analysis request.
#[derive(Debug)]
pub struct AnalysisResult {
    /// The track the request was for.
    pub path: PathBuf,
    /// Caller revision, echoed so obsolete answers can be discarded.
    pub revision: u64,
    /// What the analysis found, or why the track could not be analysed.
    pub outcome: Result<AudioAnalysis, String>,
}

/// A track waiting to be analysed.
#[derive(Clone)]
struct QueuedTrack {
    path: PathBuf,
    priority: Priority,
    sequence: u64,
    revision: u64,
}

/// The tracks waiting to be analysed.
#[derive(Default)]
struct QueueState {
    pending: Vec<QueuedTrack>,
    next_sequence: u64,
}

impl QueueState {
    /// The next track to analyse: the earliest priority first, the oldest request first inside
    /// a priority. The waiting list is empty otherwise.
    fn next_track(&mut self) -> Option<QueuedTrack> {
        let (index, _) = self
            .pending
            .iter()
            .enumerate()
            .min_by_key(|(_, queued)| (queued.priority.rank(), queued.sequence))?;
        Some(self.pending.swap_remove(index))
    }

    /// Puts a request in the waiting list, or raises the priority of a track already waiting.
    /// A track the worker is on goes back in the waiting list, so a request made while it was
    /// being analysed is answered too, from the cache.
    fn accept(&mut self, request: QueuedTrack) {
        if let Some(queued) = self
            .pending
            .iter_mut()
            .find(|queued| queued.path == request.path)
        {
            queued.revision = request.revision;
            if !queued.priority.at_least(request.priority) {
                queued.priority = request.priority;
            }
            return;
        }
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        self.pending.push(QueuedTrack {
            sequence,
            ..request
        });
    }
}

/// A queue of tracks for the analysis worker, one worker per `AnalysisQueue`.
///
/// Dropping the queue stops the worker after the track it is analysing: the rest of the queue
/// is dropped with it and the worker is joined before the queue is gone, so no detached thread
/// keeps running.
pub struct AnalysisQueue {
    request_sender: Option<Sender<Vec<QueuedTrack>>>,
    results: Receiver<AnalysisResult>,
    decoded: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl AnalysisQueue {
    /// Starts the worker thread named `dubplate-analysis` and returns the queue. The worker
    /// opens the cache at `cache_path` on its own thread.
    pub fn start(cache_path: PathBuf) -> Result<Self, AnalysisCacheError> {
        let (request_sender, request_receiver) = channel();
        let (result_sender, results) = channel();
        let decoded = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let worker = std::thread::Builder::new()
            .name("dubplate-analysis".to_owned())
            .spawn({
                let worker = Worker {
                    request_receiver,
                    result_sender,
                    decoded: Arc::clone(&decoded),
                    stop: Arc::clone(&stop),
                };
                move || worker.run(cache_path)
            })?;
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
    /// A path already queued is not queued twice; if the new priority is earlier, the track is
    /// analysed earlier instead.
    pub fn request(&self, path: PathBuf, priority: Priority) {
        self.request_all([(path, priority)]);
    }

    /// Queues several tracks at once. The worker takes the whole batch before it picks the next
    /// track, so a batch is always analysed in priority order.
    pub fn request_all(&self, requests: impl IntoIterator<Item = (PathBuf, Priority)>) {
        self.request_all_versioned(
            requests
                .into_iter()
                .map(|(path, priority)| (path, priority, 0)),
        );
    }

    /// Queues requests with a caller revision, echoed in each result. A newer request for
    /// a queued path supersedes its revision while preserving its earliest priority.
    pub fn request_all_versioned(
        &self,
        requests: impl IntoIterator<Item = (PathBuf, Priority, u64)>,
    ) {
        let batch: Vec<QueuedTrack> = requests
            .into_iter()
            .map(|(path, priority, revision)| QueuedTrack {
                revision,
                path,
                priority,
                sequence: 0,
            })
            .collect();
        if let Some(sender) = &self.request_sender {
            let _ = sender.send(batch);
        }
    }

    /// The end results arrive on this channel.
    pub fn results(&self) -> &Receiver<AnalysisResult> {
        &self.results
    }

    /// How many tracks the worker decoded, so a test can tell a cached answer from a fresh
    /// decode without timing.
    pub fn decoded_count(&self) -> usize {
        self.decoded.load(Ordering::SeqCst)
    }
}

impl Drop for AnalysisQueue {
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
    result_sender: Sender<AnalysisResult>,
    decoded: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
}

impl Worker {
    /// Takes the earliest-priority track, answers it from the cache when fresh, otherwise
    /// decodes and analyses it and stores the result. New requests are read between tracks, so
    /// a set track asked for while the library is being analysed comes next. Stops after the
    /// track in progress once the queue is dropped.
    fn run(self, cache_path: PathBuf) {
        let mut cache = match AnalysisCache::open(&cache_path) {
            Ok(cache) => cache,
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
                let outcome = self.answer(&track, &mut cache);
                let _ = self.result_sender.send(AnalysisResult {
                    path: track.path,
                    revision: track.revision,
                    outcome,
                });
            }
        }
    }

    /// The analysis of one track: the cache row when it is fresh, otherwise a fresh decode and
    /// analysis, stored for the next launch.
    fn answer(
        &self,
        track: &QueuedTrack,
        cache: &mut AnalysisCache,
    ) -> Result<AudioAnalysis, String> {
        if let Some(cached) = cache.get(&track.path).map_err(|error| error.to_string())? {
            return Ok(cached);
        }
        if std::fs::metadata(&track.path).is_err() {
            return Err(format!(
                "the file at {} is not readable",
                track.path.display()
            ));
        }
        self.decoded.fetch_add(1, Ordering::SeqCst);
        let analysis = analyse_track(&track.path).map_err(|error| error.to_string())?;
        cache
            .put(&track.path, &analysis)
            .map_err(|error| error.to_string())?;
        Ok(analysis)
    }

    /// The cache did not open: answers every request with `message` until the queue is dropped.
    fn answer_all_with(&self, message: &str) {
        for batch in self.request_receiver.iter() {
            for track in batch {
                let _ = self.result_sender.send(AnalysisResult {
                    path: track.path,
                    revision: track.revision,
                    outcome: Err(message.to_owned()),
                });
            }
        }
    }
}
