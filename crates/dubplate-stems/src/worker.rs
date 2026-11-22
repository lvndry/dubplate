//! The separation worker: a Dubplate process that separates one track into the stem cache and
//! reports how it goes on its standard output.

use std::cell::Cell;
use std::error::Error;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use euphoria_core::stems as separation;

use crate::files::StemFiles;
use crate::location::StemsLocation;
use crate::protocol::{WorkerArguments, WorkerMessage};
use crate::status::{SeparationProgress, SeparationStage};

/// The exit code of a worker whose app went away before the track was separated.
const EXIT_APP_GONE: i32 = 3;
/// How far a worker lowers its scheduling priority, as `nice -n 10` would: the app, the music
/// and the DJ's other apps go first whenever they want the processor.
const WORKER_NICENESS: libc::c_int = 10;

/// Why a track could not be separated.
#[derive(Debug, thiserror::Error)]
pub enum SeparationError {
    /// Decoding, the network or the cache failed.
    #[error("{}", describe(.0))]
    Separation(#[from] euphoria_core::error::EuphoriaError),
    /// The separation finished but the four stem files are not all in the cache.
    #[error("the stems of the track are not all in {}", .0.display())]
    Incomplete(PathBuf),
}

/// Runs this process as a separation worker when the app started it as one, and returns the
/// code to exit with; `None` when the process is not a worker. Call it first thing in `main`,
/// before anything else starts.
///
/// A worker separates one track into the cache, printing its progress for the app. It stops at
/// once when the app goes away, so a separation never outlives Dubplate.
pub fn run_worker_if_requested() -> Option<ExitCode> {
    let arguments = WorkerArguments::parse(std::env::args_os().skip(1))?;
    Some(match arguments {
        Ok(arguments) => {
            lower_priority();
            exit_when_the_app_goes_away();
            run(&arguments)
        }
        Err(reason) => {
            print_message(&WorkerMessage::Failed { reason });
            ExitCode::FAILURE
        }
    })
}

/// Separates `track` into the cache at `location` in this process, reporting its progress to
/// `progress`, and returns the stems. Stems already in the cache are returned at once.
///
/// The app runs this in a worker process; it is public for tools that measure it.
pub fn separate_in_process(
    location: &StemsLocation,
    track: &Path,
    progress: &dyn Fn(SeparationProgress),
) -> Result<StemFiles, SeparationError> {
    let stage = Cell::new(SeparationStage::Starting);
    separate_reporting(location, track, &|message| match message {
        WorkerMessage::Stage(new_stage) => {
            stage.set(new_stage);
            progress(SeparationProgress::new(new_stage, 0, 1));
        }
        WorkerMessage::Progress { done, total } => {
            progress(SeparationProgress::new(stage.get(), done, total));
        }
        WorkerMessage::Ready { .. } | WorkerMessage::Failed { .. } => {}
    })
}

fn run(arguments: &WorkerArguments) -> ExitCode {
    match separate_reporting(&arguments.location, arguments.track(), &|message| {
        print_message(&message);
    }) {
        Ok(files) => match files.entry_name() {
            Some(entry) => {
                print_message(&WorkerMessage::Ready {
                    entry: entry.to_owned(),
                });
                ExitCode::SUCCESS
            }
            None => {
                print_message(&WorkerMessage::Failed {
                    reason: SeparationError::Incomplete(files.directory().to_path_buf())
                        .to_string(),
                });
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            print_message(&WorkerMessage::Failed {
                reason: error.to_string(),
            });
            ExitCode::FAILURE
        }
    }
}

fn separate_reporting(
    location: &StemsLocation,
    track: &Path,
    report: &dyn Fn(WorkerMessage),
) -> Result<StemFiles, SeparationError> {
    seed_weights_from_euphoria(location.weights_path());
    let cache = separation::StemCache::new(
        location.cache_directory().to_path_buf(),
        location.limit().bytes(),
    );
    let mut separator =
        separation::CachedSeparator::new(cache, location.weights_path().to_path_buf());
    let observer = Reporter {
        report,
        downloaded_percent: Cell::new(None),
    };
    let files = separator.stems(track, &observer)?;
    let directory = files.directory().to_path_buf();
    StemFiles::in_folder(directory.clone()).ok_or(SeparationError::Incomplete(directory))
}

/// Turns the separator's progress into worker messages, a download's at most once a percent.
struct Reporter<'report> {
    report: &'report dyn Fn(WorkerMessage),
    downloaded_percent: Cell<Option<u64>>,
}

impl separation::SeparationObserver for Reporter<'_> {
    fn stage(&self, stage: separation::SeparationStage) {
        (self.report)(WorkerMessage::Stage(stage.into()));
    }

    fn downloading(&self, received: u64, total: u64) {
        let percent = received.saturating_mul(100) / total.max(1);
        if self.downloaded_percent.get() == Some(percent) {
            return;
        }
        if self.downloaded_percent.get().is_none() {
            (self.report)(WorkerMessage::Stage(SeparationStage::DownloadingModel));
        }
        self.downloaded_percent.set(Some(percent));
        (self.report)(WorkerMessage::Progress {
            done: received,
            total,
        });
    }

    fn separated(&self, done: usize, total: usize) {
        (self.report)(WorkerMessage::Progress {
            done: done as u64,
            total: total as u64,
        });
    }
}

fn print_message(message: &WorkerMessage) {
    let mut output = std::io::stdout().lock();
    let _ = writeln!(output, "{}", message.to_line());
    let _ = output.flush();
}

/// Lowers this process's scheduling priority before the network starts its threads, which
/// inherit it.
fn lower_priority() {
    // SAFETY: setpriority takes plain integers and only changes this process's priority.
    let result = unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, WORKER_NICENESS) };
    if result != 0 {
        eprintln!(
            "warning: the separation could not lower its priority: {}",
            std::io::Error::last_os_error()
        );
    }
}

/// Ends the process once its standard input closes, which happens when the app that started it
/// quits or crashes.
fn exit_when_the_app_goes_away() {
    let watcher = std::thread::Builder::new()
        .name("dubplate-stems-app-watch".to_owned())
        .spawn(|| {
            let _ = std::io::copy(&mut std::io::stdin().lock(), &mut std::io::sink());
            std::process::exit(EXIT_APP_GONE);
        });
    if let Err(error) = watcher {
        eprintln!("warning: the separation will not stop if Dubplate quits: {error}");
    }
}

/// Copies Euphoria's copy of the weights when Dubplate has none yet, so a DJ who uses both apps
/// downloads them once. The copy is checked against the weights' SHA-256 when it is loaded,
/// like a download; when there is nothing to copy, or copying fails, the weights are downloaded.
fn seed_weights_from_euphoria(weights: &Path) {
    if weights.exists() {
        return;
    }
    let Ok(euphoria_weights) = separation::default_weights_path() else {
        return;
    };
    if !euphoria_weights.is_file() {
        return;
    }
    let Some(folder) = weights.parent() else {
        return;
    };
    let partial = weights.with_extension("partial");
    let copied = std::fs::create_dir_all(folder)
        .and_then(|()| std::fs::copy(&euphoria_weights, &partial))
        .and_then(|_| std::fs::rename(&partial, weights));
    if let Err(error) = copied {
        let _ = std::fs::remove_file(&partial);
        eprintln!(
            "warning: could not copy the weights from {}, downloading them instead: {error}",
            euphoria_weights.display()
        );
    }
}

/// An error and its causes on one line.
fn describe(error: &dyn Error) -> String {
    let mut description = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        description.push_str(": ");
        description.push_str(&cause.to_string());
        source = cause.source();
    }
    description
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use euphoria_core::stems::SeparationObserver;

    use super::*;

    #[test]
    fn a_download_is_reported_once_a_percent() {
        let messages = RefCell::new(Vec::new());
        let report = |message| messages.borrow_mut().push(message);
        let observer = Reporter {
            report: &report,
            downloaded_percent: Cell::new(None),
        };
        for received in (0..=1_000).step_by(5) {
            observer.downloading(received, 1_000);
        }
        let messages = messages.into_inner();
        assert_eq!(
            messages[0],
            WorkerMessage::Stage(SeparationStage::DownloadingModel)
        );
        assert_eq!(messages.len(), 1 + 101);
        assert_eq!(
            messages.last(),
            Some(&WorkerMessage::Progress {
                done: 1_000,
                total: 1_000
            })
        );
    }

    #[test]
    fn an_error_is_described_with_its_causes() {
        #[derive(Debug, thiserror::Error)]
        #[error("cannot load the network")]
        struct Outer(#[source] std::io::Error);
        let error = Outer(std::io::Error::other("out of memory"));
        assert_eq!(describe(&error), "cannot load the network: out of memory");
    }
}
