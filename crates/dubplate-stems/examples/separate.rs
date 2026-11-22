//! Separates tracks the way the app does, through the queue and a worker process at low
//! priority, and prints how long each one took.
//!
//! ```text
//! cargo run --profile fast -p dubplate-stems --example separate -- <stems-folder> [--threads 3] <track>...
//! ```
//!
//! The weights are kept in `<stems-folder>/models` and the stems in `<stems-folder>/stems`.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use dubplate_stems::{
    CurrentExecutable, DEFAULT_CACHE_LIMIT, SeparationPriority, SeparationQueue, StemsLocation,
    StemsStatus, WorkerThreads, run_worker_if_requested,
};

fn main() -> ExitCode {
    if let Some(exit) = run_worker_if_requested() {
        return exit;
    }
    let mut arguments = std::env::args_os().skip(1);
    let Some(folder) = arguments.next().map(PathBuf::from) else {
        eprintln!("usage: separate <stems-folder> [--threads N] <track>...");
        return ExitCode::FAILURE;
    };
    let mut threads = WorkerThreads::default();
    let mut tracks = Vec::new();
    while let Some(argument) = arguments.next() {
        if argument == "--threads" {
            let count = arguments
                .next()
                .and_then(|count| count.to_str()?.parse().ok());
            let Some(count) = count else {
                eprintln!("--threads takes a number");
                return ExitCode::FAILURE;
            };
            threads = WorkerThreads::new(count);
        } else {
            tracks.push(PathBuf::from(argument));
        }
    }
    let location = StemsLocation::new(
        folder.join("stems"),
        folder.join("models/htdemucs-955717e8.safetensors"),
        DEFAULT_CACHE_LIMIT,
    );
    let launcher = match CurrentExecutable::new(location.clone()) {
        Ok(launcher) => launcher.with_threads(threads),
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    let queue = match SeparationQueue::start(location, launcher) {
        Ok(queue) => queue,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    println!("threads: {}", threads.count());
    let mut started = Instant::now();
    queue.request_all(
        tracks
            .iter()
            .map(|track| (track.clone(), SeparationPriority::Set)),
    );
    let mut finished = 0;
    let mut failed = false;
    while finished < tracks.len() {
        let Ok(update) = queue.updates().recv() else {
            break;
        };
        match update.status {
            StemsStatus::Separating(progress) if progress.fraction() == 0.0 => {
                if progress.stage() == dubplate_stems::SeparationStage::Starting {
                    started = Instant::now();
                }
                println!("  {}: {}", update.track.display(), progress.stage().label());
            }
            StemsStatus::Ready(files) => {
                finished += 1;
                println!(
                    "{:.1} s  {}  ->  {}",
                    started.elapsed().as_secs_f64(),
                    update.track.display(),
                    files.directory().display()
                );
            }
            StemsStatus::Failed(reason) => {
                finished += 1;
                failed = true;
                println!("failed  {}: {reason}", update.track.display());
            }
            StemsStatus::None | StemsStatus::Queued | StemsStatus::Separating(_) => {}
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
