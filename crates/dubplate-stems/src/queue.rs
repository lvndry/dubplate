//! The queue of tracks to separate, run by one thread that starts one worker process at a time.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, ExitStatus, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;

use crate::files::StemFiles;
use crate::index::StemsIndex;
use crate::launcher::WorkerLauncher;
use crate::location::StemsLocation;
use crate::protocol::WorkerMessage;
use crate::status::{SeparationProgress, SeparationStage, StemsStatus, StemsUpdate};

/// Prefixes of the folders the cache writes into before a separation is complete. A worker
/// stopped in the middle leaves one behind.
const UNFINISHED_PREFIXES: [&str; 2] = [".incoming-", ".replaced-"];

/// The order in which tracks are separated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SeparationPriority {
    /// The DJ asked for this track's stems: separated next.
    Now,
    /// A track of the set being prepared.
    Set,
    /// A track of the shortlist, once the set is done.
    Shortlist,
}

/// Separates tracks one at a time, the earliest priority first, each in a worker process, and
/// reports every change of their [`StemsStatus`] on [`SeparationQueue::updates`].
///
/// [`SeparationQueue::cancel`] stops a track at once, even halfway through. Dropping the queue
/// stops the worker in progress, forgets the waiting tracks and joins every thread before it
/// returns, so no separation keeps running after it.
pub struct SeparationQueue {
    events: Sender<Event>,
    updates: Receiver<StemsUpdate>,
    thread: Option<JoinHandle<()>>,
}

impl SeparationQueue {
    /// Starts the queue's thread, named `dubplate-stems`, which separates into `location` with
    /// workers from `launcher`.
    pub fn start(location: StemsLocation, launcher: impl WorkerLauncher) -> std::io::Result<Self> {
        let (events, event_receiver) = channel();
        let (update_sender, updates) = channel();
        let scheduler = Scheduler {
            index: StemsIndex::open(location.cache_directory()),
            location,
            launcher: Box::new(launcher),
            events: event_receiver,
            event_sender: events.clone(),
            updates: update_sender,
            waiting: Vec::new(),
            next_sequence: 0,
            running: None,
            next_job: 0,
        };
        let thread = std::thread::Builder::new()
            .name("dubplate-stems".to_owned())
            .spawn(move || scheduler.run())?;
        Ok(Self {
            events,
            updates,
            thread: Some(thread),
        })
    }

    /// Asks for the stems of `track` at `priority`. A track already waiting moves up when the
    /// new priority is earlier; a track with stems is answered at once.
    pub fn request(&self, track: PathBuf, priority: SeparationPriority) {
        self.request_all([(track, priority)]);
    }

    /// Asks for the stems of several tracks; they are all queued before the next one starts.
    pub fn request_all(&self, requests: impl IntoIterator<Item = (PathBuf, SeparationPriority)>) {
        self.send(Event::Request(requests.into_iter().collect()));
    }

    /// Asks which of `tracks` have stems already, without separating any: each gets a
    /// [`StemsStatus::Ready`] or [`StemsStatus::None`] update, unless it is queued. Reads no
    /// track, so it is quick for a whole library.
    pub fn probe(&self, tracks: impl IntoIterator<Item = PathBuf>) {
        self.send(Event::Probe(tracks.into_iter().collect()));
    }

    /// Stops separating `track`, at once when it is in progress, and forgets it if it waits.
    pub fn cancel(&self, track: PathBuf) {
        self.send(Event::Cancel(track));
    }

    /// Stops the separation in progress and forgets every waiting track.
    pub fn cancel_all(&self) {
        self.send(Event::CancelAll);
    }

    /// Every change of a track's stems status, in order.
    pub fn updates(&self) -> &Receiver<StemsUpdate> {
        &self.updates
    }

    fn send(&self, event: Event) {
        let _ = self.events.send(event);
    }
}

impl Drop for SeparationQueue {
    fn drop(&mut self) {
        self.send(Event::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl std::fmt::Debug for SeparationQueue {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SeparationQueue")
            .finish_non_exhaustive()
    }
}

/// What the queue's thread reacts to: the app's requests and the running worker's messages.
enum Event {
    Request(Vec<(PathBuf, SeparationPriority)>),
    Probe(Vec<PathBuf>),
    Cancel(PathBuf),
    CancelAll,
    Worker { job: u64, message: WorkerMessage },
    WorkerDone { job: u64 },
    Stop,
}

struct Waiting {
    track: PathBuf,
    priority: SeparationPriority,
    sequence: u64,
}

/// The worker separating a track, and what it said so far.
struct Running {
    job: u64,
    track: PathBuf,
    child: Child,
    reader: Option<JoinHandle<()>>,
    stage: SeparationStage,
    ready: Option<String>,
    failure: Option<String>,
}

/// How a worker ended.
enum Outcome {
    Ready(StemFiles),
    Failed(String),
    Cancelled,
}

struct Scheduler {
    location: StemsLocation,
    launcher: Box<dyn WorkerLauncher>,
    index: StemsIndex,
    events: Receiver<Event>,
    event_sender: Sender<Event>,
    updates: Sender<StemsUpdate>,
    waiting: Vec<Waiting>,
    next_sequence: u64,
    running: Option<Running>,
    next_job: u64,
}

impl Scheduler {
    fn run(mut self) {
        remove_unfinished(self.location.cache_directory());
        loop {
            if self.running.is_none() {
                self.start_next();
            }
            let Ok(event) = self.events.recv() else {
                self.stop_running(Outcome::Cancelled);
                return;
            };
            match event {
                Event::Request(requests) => {
                    for (track, priority) in requests {
                        self.accept(track, priority);
                    }
                }
                Event::Probe(tracks) => {
                    for track in tracks {
                        self.probe(track);
                    }
                }
                Event::Cancel(track) => self.cancel(&track),
                Event::CancelAll => {
                    for waiting in std::mem::take(&mut self.waiting) {
                        self.report(waiting.track, StemsStatus::None);
                    }
                    self.stop_running(Outcome::Cancelled);
                }
                Event::Worker { job, message } => self.heard(job, message),
                Event::WorkerDone { job } => {
                    if self
                        .running
                        .as_ref()
                        .is_some_and(|running| running.job == job)
                    {
                        self.finish_running();
                    }
                }
                Event::Stop => {
                    self.stop_running(Outcome::Cancelled);
                    return;
                }
            }
        }
    }

    fn accept(&mut self, track: PathBuf, priority: SeparationPriority) {
        if self
            .running
            .as_ref()
            .is_some_and(|running| running.track == track)
        {
            return;
        }
        if let Some(waiting) = self
            .waiting
            .iter_mut()
            .find(|waiting| waiting.track == track)
        {
            waiting.priority = waiting.priority.min(priority);
            return;
        }
        if let Some(files) = self.index.lookup(&track) {
            self.report(track, StemsStatus::Ready(files));
            return;
        }
        self.waiting.push(Waiting {
            track: track.clone(),
            priority,
            sequence: self.next_sequence,
        });
        self.next_sequence += 1;
        self.report(track, StemsStatus::Queued);
    }

    fn probe(&mut self, track: PathBuf) {
        let busy = self
            .running
            .as_ref()
            .is_some_and(|running| running.track == track)
            || self.waiting.iter().any(|waiting| waiting.track == track);
        if busy {
            return;
        }
        let status = match self.index.lookup(&track) {
            Some(files) => StemsStatus::Ready(files),
            None => StemsStatus::None,
        };
        self.report(track, status);
    }

    fn cancel(&mut self, track: &Path) {
        if let Some(position) = self
            .waiting
            .iter()
            .position(|waiting| waiting.track == track)
        {
            let waiting = self.waiting.swap_remove(position);
            self.report(waiting.track, StemsStatus::None);
        }
        if self
            .running
            .as_ref()
            .is_some_and(|running| running.track == track)
        {
            self.stop_running(Outcome::Cancelled);
        }
    }

    /// Starts a worker on the earliest waiting track, the oldest request first within a
    /// priority. A worker that cannot start fails its track and the next one is tried.
    fn start_next(&mut self) {
        while self.running.is_none() {
            let Some(position) = self
                .waiting
                .iter()
                .enumerate()
                .min_by_key(|(_, waiting)| (waiting.priority, waiting.sequence))
                .map(|(position, _)| position)
            else {
                return;
            };
            let waiting = self.waiting.swap_remove(position);
            match self.spawn(&waiting.track) {
                Ok(running) => {
                    self.running = Some(running);
                    self.report(
                        waiting.track,
                        StemsStatus::Separating(SeparationProgress::new(
                            SeparationStage::Starting,
                            0,
                            1,
                        )),
                    );
                }
                Err(error) => self.report(
                    waiting.track,
                    StemsStatus::Failed(format!("the separation could not start: {error}")),
                ),
            }
        }
    }

    fn spawn(&mut self, track: &Path) -> std::io::Result<Running> {
        let job = self.next_job;
        self.next_job += 1;
        let mut child = self
            .launcher
            .command(track)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let Some(stdout) = child.stdout.take() else {
            stop_child(&mut child);
            return Err(std::io::Error::other("the worker has no output to read"));
        };
        let events = self.event_sender.clone();
        let reader = std::thread::Builder::new()
            .name("dubplate-stems-worker".to_owned())
            .spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    let Ok(line) = line else {
                        break;
                    };
                    if let Some(message) = WorkerMessage::parse(&line)
                        && events.send(Event::Worker { job, message }).is_err()
                    {
                        return;
                    }
                }
                let _ = events.send(Event::WorkerDone { job });
            });
        let reader = match reader {
            Ok(reader) => reader,
            Err(error) => {
                stop_child(&mut child);
                return Err(error);
            }
        };
        Ok(Running {
            job,
            track: track.to_path_buf(),
            child,
            reader: Some(reader),
            stage: SeparationStage::Starting,
            ready: None,
            failure: None,
        })
    }

    fn heard(&mut self, job: u64, message: WorkerMessage) {
        let Some(running) = self.running.as_mut().filter(|running| running.job == job) else {
            return;
        };
        let progress = match message {
            WorkerMessage::Stage(stage) => {
                running.stage = stage;
                SeparationProgress::new(stage, 0, 1)
            }
            WorkerMessage::Progress { done, total } => {
                SeparationProgress::new(running.stage, done, total)
            }
            WorkerMessage::Ready { entry } => {
                running.ready = Some(entry);
                return;
            }
            WorkerMessage::Failed { reason } => {
                running.failure = Some(reason);
                return;
            }
        };
        let track = running.track.clone();
        self.report(track, StemsStatus::Separating(progress));
    }

    /// The worker closed its output: waits for it to exit and reports how it ended.
    fn finish_running(&mut self) {
        let Some(mut running) = self.running.take() else {
            return;
        };
        let exit = running.child.wait();
        join_reader(&mut running);
        let outcome = match (exit, running.ready.take()) {
            (Ok(status), Some(entry)) if status.success() => {
                let directory = self.location.cache_directory().join(&entry);
                match StemFiles::in_folder(directory) {
                    Some(files) => Outcome::Ready(files),
                    None => Outcome::Failed(format!(
                        "the separation finished but the stems are not all in {entry}"
                    )),
                }
            }
            (Ok(status), _) => Outcome::Failed(
                running
                    .failure
                    .take()
                    .unwrap_or_else(|| stopped_without_reason(status)),
            ),
            (Err(error), _) => Outcome::Failed(format!("the separation was lost: {error}")),
        };
        self.conclude(running.track, outcome);
    }

    /// Stops the worker in progress at once, if any, and reports `outcome` for its track.
    fn stop_running(&mut self, outcome: Outcome) {
        let Some(mut running) = self.running.take() else {
            return;
        };
        stop_child(&mut running.child);
        join_reader(&mut running);
        self.conclude(running.track, outcome);
    }

    fn conclude(&mut self, track: PathBuf, outcome: Outcome) {
        remove_unfinished(self.location.cache_directory());
        let status = match outcome {
            Outcome::Ready(files) => {
                if let Err(error) = self.index.record(&track, &files) {
                    eprintln!(
                        "warning: the stems of {} are ready but could not be remembered for the next launch: {error}",
                        track.display()
                    );
                }
                StemsStatus::Ready(files)
            }
            Outcome::Failed(reason) => StemsStatus::Failed(reason),
            Outcome::Cancelled => StemsStatus::None,
        };
        self.report(track, status);
    }

    fn report(&self, track: PathBuf, status: StemsStatus) {
        let _ = self.updates.send(StemsUpdate { track, status });
    }
}

/// Kills a worker and waits for it, so it leaves no zombie behind.
fn stop_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn join_reader(running: &mut Running) {
    if let Some(reader) = running.reader.take() {
        let _ = reader.join();
    }
}

fn stopped_without_reason(status: ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("the separation stopped with exit code {code}"),
        None => "the separation stopped unexpectedly".to_owned(),
    }
}

/// Removes what a worker stopped halfway left in the cache. Only called while no worker runs.
fn remove_unfinished(cache: &Path) {
    let Ok(entries) = std::fs::read_dir(cache) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let unfinished = name.to_str().is_some_and(|name| {
            UNFINISHED_PREFIXES
                .iter()
                .any(|prefix| name.starts_with(prefix))
        });
        if unfinished && entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}
