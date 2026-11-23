//! Separating tracks into stems ahead of the set: drums, bass, melody and vocals, so a deck can
//! play each one at its own level.
//!
//! Separation runs Euphoria's Demucs htdemucs network through ONNX Runtime, about a third of a
//! track's length on an M-series Mac, so it happens while the DJ prepares, never live:
//!
//! - [`SeparationQueue`] separates the set's tracks first, then the shortlist, one track at a
//!   time, and reports a [`StemsStatus`] for each track as it goes. A track can be cancelled at
//!   any moment, and dropping the queue stops everything at once.
//! - Each separation runs in a worker process of its own ([`run_worker_if_requested`]), so the
//!   2.5 GB the network needs goes back to the system after every track, a failure in the
//!   network never touches the music playing, and cancelling stops it at once. The worker runs
//!   at a low scheduling priority on a few threads ([`WorkerThreads`]), so preparing a set
//!   never makes the Mac sluggish.
//! - The stems are kept in Dubplate's own folder ([`StemsLocation`]), four 16-bit WAV files per
//!   track, keyed by the content of the file, never written next to the music or in rekordbox's
//!   folders. The model's weights are downloaded once, from the pinned revision Euphoria uses,
//!   and checked against their SHA-256 on every load.

mod files;
mod index;
mod launcher;
mod location;
mod protocol;
mod queue;
mod status;
mod worker;

pub use dubplate_engine::Stem;
pub use files::{SEAM_EDGE_SECONDS, STEMS_SAMPLE_RATE, StemFiles, StemLoadError, VocalPresence};
pub use index::StemsIndex;
pub use launcher::{CurrentExecutable, DEFAULT_WORKER_THREADS, WorkerLauncher, WorkerThreads};
pub use location::{CacheLimit, DEFAULT_CACHE_LIMIT, StemsLocation, WEIGHTS_URL};
pub use queue::{SeparationPriority, SeparationQueue};
pub use status::{SeparationProgress, SeparationStage, StemsStatus, StemsUpdate};
pub use worker::{SeparationError, run_worker_if_requested, separate_in_process};
