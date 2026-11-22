//! How the queue starts a separation worker.

use std::path::{Path, PathBuf};
use std::process::Command;

use euphoria_core::stems::STEMS_THREADS_VARIABLE;

use crate::location::StemsLocation;
use crate::protocol::WorkerArguments;

/// How many threads a worker runs the network on unless told otherwise: few enough that the
/// app, the music and everything else on the Mac stay responsive while a set is prepared.
pub const DEFAULT_WORKER_THREADS: WorkerThreads = WorkerThreads(3);

/// How many threads a worker runs the network on, at least one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerThreads(usize);

impl WorkerThreads {
    /// `count` threads, or one when `count` is zero.
    pub const fn new(count: usize) -> Self {
        Self(if count == 0 { 1 } else { count })
    }

    /// The number of threads.
    pub const fn count(self) -> usize {
        self.0
    }
}

impl Default for WorkerThreads {
    fn default() -> Self {
        DEFAULT_WORKER_THREADS
    }
}

/// Builds the command that separates one track in a worker process.
///
/// The worker prints its progress on its standard output, one message per line, and exits once
/// the track is separated. [`CurrentExecutable`] runs the app's own executable as the worker;
/// any `Fn(&Path) -> Command` launches another program, such as a stand-in in tests.
pub trait WorkerLauncher: Send + 'static {
    /// The command that separates `track`.
    fn command(&self, track: &Path) -> Command;
}

impl<Build> WorkerLauncher for Build
where
    Build: Fn(&Path) -> Command + Send + 'static,
{
    fn command(&self, track: &Path) -> Command {
        self(track)
    }
}

/// Runs the executable of this process as the worker, on [`DEFAULT_WORKER_THREADS`] threads
/// unless told otherwise. Its `main` must call [`crate::run_worker_if_requested`] before
/// anything else. The worker lowers its own scheduling priority, so separating never makes the
/// Mac sluggish.
#[derive(Debug, Clone)]
pub struct CurrentExecutable {
    program: PathBuf,
    location: StemsLocation,
    threads: WorkerThreads,
}

impl CurrentExecutable {
    /// Workers that separate into `location`.
    pub fn new(location: StemsLocation) -> std::io::Result<Self> {
        Ok(Self {
            program: std::env::current_exe()?,
            location,
            threads: DEFAULT_WORKER_THREADS,
        })
    }

    /// Runs the network on `threads` threads instead.
    pub fn with_threads(self, threads: WorkerThreads) -> Self {
        Self { threads, ..self }
    }
}

impl WorkerLauncher for CurrentExecutable {
    fn command(&self, track: &Path) -> Command {
        let mut command = Command::new(&self.program);
        command.args(
            WorkerArguments {
                location: self.location.clone(),
                track: track.to_path_buf(),
            }
            .to_args(),
        );
        command.env(STEMS_THREADS_VARIABLE, self.threads.count().to_string());
        command
    }
}
