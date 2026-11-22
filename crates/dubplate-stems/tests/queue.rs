//! The separation queue against stand-in workers: shell scripts that speak the worker's
//! protocol, so ordering, cancelling and stopping are checked on real processes without the
//! network.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use dubplate_stems::{
    CacheLimit, SeparationPriority, SeparationQueue, SeparationStage, StemsLocation, StemsStatus,
    StemsUpdate,
};

const PATIENCE: Duration = Duration::from_secs(10);
/// How quickly a cancelled or dropped separation must be gone.
const PROMPT: Duration = Duration::from_millis(500);

struct Fixture {
    folder: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        Self {
            folder: tempfile::tempdir().unwrap(),
        }
    }

    fn cache(&self) -> PathBuf {
        self.folder.path().join("stems")
    }

    fn location(&self) -> StemsLocation {
        StemsLocation::new(
            self.cache(),
            self.folder.path().join("models/weights.safetensors"),
            CacheLimit::from_bytes(1_000_000),
        )
    }

    fn track(&self, name: &str) -> PathBuf {
        let path = self.folder.path().join(format!("{name}.m4a"));
        std::fs::write(&path, name.as_bytes()).unwrap();
        path
    }

    fn pid_file(&self) -> PathBuf {
        self.folder.path().join("worker.pid")
    }

    /// A queue whose workers run `script` in `sh`, with the track as `$1`, the cache folder as
    /// `$2` and the file to write the worker's process id to as `$3`.
    fn queue(&self, script: &'static str) -> SeparationQueue {
        let cache = self.cache();
        let pid_file = self.pid_file();
        SeparationQueue::start(self.location(), move |track: &Path| {
            let mut command = Command::new("/bin/sh");
            command
                .arg("-c")
                .arg(script)
                .arg("worker")
                .arg(track)
                .arg(&cache)
                .arg(&pid_file);
            command
        })
        .unwrap()
    }

    /// Waits until the stand-in worker wrote its process id.
    fn worker_pid(&self) -> u32 {
        let deadline = Instant::now() + PATIENCE;
        loop {
            if let Ok(text) = std::fs::read_to_string(self.pid_file())
                && let Ok(pid) = text.trim().parse()
            {
                return pid;
            }
            assert!(Instant::now() < deadline, "the worker never started");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

/// Makes the four stem files of the track named in `$1` and reports them ready.
const SEPARATES: &str = r#"
entry="htdemucs-955717e8-$(basename "$1" .m4a)"
mkdir -p "$2/$entry"
for stem in drums bass other vocals; do : > "$2/$entry/$stem.wav"; done
echo "stage separating"
echo "progress 1 2"
echo "progress 2 2"
echo "ready $entry"
"#;

/// Reports some progress, leaves an unfinished folder in the cache and then works for a long
/// time, as a real separation does.
const SEPARATES_SLOWLY: &str = r#"
echo $$ > "$3"
mkdir -p "$2/.incoming-unfinished"
echo "stage separating"
echo "progress 1 40"
exec sleep 30
"#;

fn is_running(pid: u32) -> bool {
    Command::new("/bin/kill")
        .arg("-0")
        .arg(pid.to_string())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn next_update(queue: &SeparationQueue) -> StemsUpdate {
    queue
        .updates()
        .recv_timeout(PATIENCE)
        .expect("an update in time")
}

/// Takes updates until `track` reaches a status `accepts` takes, and returns it.
fn wait_for(
    queue: &SeparationQueue,
    track: &Path,
    accepts: impl Fn(&StemsStatus) -> bool,
) -> StemsStatus {
    loop {
        let update = next_update(queue);
        if update.track == track && accepts(&update.status) {
            return update.status;
        }
    }
}

#[test]
fn tracks_are_separated_now_first_then_the_set_then_the_shortlist() {
    let fixture = Fixture::new();
    let queue = fixture.queue(SEPARATES);
    let shortlisted = fixture.track("shortlisted");
    let in_set = fixture.track("in-set");
    let asked = fixture.track("asked");
    queue.request_all([
        (shortlisted.clone(), SeparationPriority::Shortlist),
        (in_set.clone(), SeparationPriority::Set),
        (asked.clone(), SeparationPriority::Now),
    ]);
    let mut ready = Vec::new();
    while ready.len() < 3 {
        let update = next_update(&queue);
        if let StemsStatus::Ready(files) = &update.status {
            assert!(files.directory().starts_with(fixture.cache()));
            ready.push(update.track);
        }
    }
    assert_eq!(ready, [asked, in_set, shortlisted]);
}

#[test]
fn a_separation_reports_its_progress_on_the_way_to_ready() {
    let fixture = Fixture::new();
    let queue = fixture.queue(SEPARATES);
    let track = fixture.track("progress");
    queue.request(track.clone(), SeparationPriority::Set);
    let mut statuses = Vec::new();
    loop {
        let update = next_update(&queue);
        let done = matches!(update.status, StemsStatus::Ready(_));
        statuses.push(update.status);
        if done {
            break;
        }
    }
    assert_eq!(statuses[0], StemsStatus::Queued);
    let halfway = statuses.iter().any(|status| {
        matches!(status, StemsStatus::Separating(progress)
            if progress.stage() == SeparationStage::Separating && progress.overall() == 0.5)
    });
    assert!(halfway, "{statuses:?}");
}

#[test]
fn a_track_with_stems_is_found_after_a_relaunch_without_a_worker() {
    let fixture = Fixture::new();
    let track = fixture.track("kept");
    {
        let queue = fixture.queue(SEPARATES);
        queue.request(track.clone(), SeparationPriority::Set);
        wait_for(&queue, &track, |status| matches!(status, StemsStatus::Ready(_)));
    }
    let relaunched = fixture.queue("echo 'failed no worker should start'; exit 1");
    let other = fixture.track("never-separated");
    relaunched.probe([track.clone(), other.clone()]);
    let first = next_update(&relaunched);
    assert_eq!(first.track, track);
    assert!(matches!(first.status, StemsStatus::Ready(_)));
    assert_eq!(
        next_update(&relaunched),
        StemsUpdate {
            track: other,
            status: StemsStatus::None
        }
    );
    relaunched.request(track.clone(), SeparationPriority::Now);
    assert!(matches!(
        next_update(&relaunched).status,
        StemsStatus::Ready(_)
    ));
}

#[test]
fn cancelling_stops_the_worker_at_once_and_cleans_up_after_it() {
    let fixture = Fixture::new();
    let queue = fixture.queue(SEPARATES_SLOWLY);
    let track = fixture.track("cancelled");
    queue.request(track.clone(), SeparationPriority::Now);
    wait_for(&queue, &track, |status| {
        matches!(status, StemsStatus::Separating(progress) if progress.fraction() > 0.0)
    });
    let pid = fixture.worker_pid();
    assert!(is_running(pid));

    let asked = Instant::now();
    queue.cancel(track.clone());
    assert_eq!(
        wait_for(&queue, &track, |status| !status.is_pending()),
        StemsStatus::None
    );
    assert!(asked.elapsed() < PROMPT, "{:?}", asked.elapsed());
    assert!(!is_running(pid));
    assert!(!fixture.cache().join(".incoming-unfinished").exists());
}

#[test]
fn cancelling_everything_forgets_the_waiting_tracks_too() {
    let fixture = Fixture::new();
    let queue = fixture.queue(SEPARATES_SLOWLY);
    let first = fixture.track("first");
    let second = fixture.track("second");
    queue.request_all([
        (first.clone(), SeparationPriority::Set),
        (second.clone(), SeparationPriority::Shortlist),
    ]);
    wait_for(&queue, &first, |status| {
        matches!(status, StemsStatus::Separating(progress) if progress.fraction() > 0.0)
    });
    let pid = fixture.worker_pid();
    queue.cancel_all();
    let mut stopped = Vec::new();
    while stopped.len() < 2 {
        let update = next_update(&queue);
        if update.status == StemsStatus::None {
            stopped.push(update.track);
        }
    }
    stopped.sort();
    assert_eq!(stopped, [first, second]);
    assert!(!is_running(pid));
}

#[test]
fn dropping_the_queue_stops_the_worker_before_it_returns() {
    let fixture = Fixture::new();
    let queue = fixture.queue(SEPARATES_SLOWLY);
    let track = fixture.track("dropped");
    queue.request(track.clone(), SeparationPriority::Now);
    wait_for(&queue, &track, |status| {
        matches!(status, StemsStatus::Separating(progress) if progress.fraction() > 0.0)
    });
    let pid = fixture.worker_pid();
    let asked = Instant::now();
    drop(queue);
    assert!(asked.elapsed() < PROMPT, "{:?}", asked.elapsed());
    assert!(!is_running(pid));
}

#[test]
fn a_worker_that_fails_or_crashes_fails_its_track_and_the_queue_goes_on() {
    let fixture = Fixture::new();
    let queue = fixture.queue(
        r#"
case "$1" in
  *broken*) echo "failed cannot decode the track"; exit 1 ;;
  *crashing*) kill -9 $$ ;;
  *) entry="htdemucs-955717e8-fine"; mkdir -p "$2/$entry"
     for stem in drums bass other vocals; do : > "$2/$entry/$stem.wav"; done
     echo "ready $entry" ;;
esac
"#,
    );
    let broken = fixture.track("broken");
    let crashing = fixture.track("crashing");
    let fine = fixture.track("fine");
    queue.request_all([
        (broken.clone(), SeparationPriority::Now),
        (crashing.clone(), SeparationPriority::Set),
        (fine.clone(), SeparationPriority::Shortlist),
    ]);
    assert_eq!(
        wait_for(&queue, &broken, |status| !status.is_pending()),
        StemsStatus::Failed("cannot decode the track".to_owned())
    );
    assert_eq!(
        wait_for(&queue, &crashing, |status| !status.is_pending()),
        StemsStatus::Failed("the separation stopped unexpectedly".to_owned())
    );
    assert!(matches!(
        wait_for(&queue, &fine, |status| !status.is_pending()),
        StemsStatus::Ready(_)
    ));
}

#[test]
fn a_track_asked_for_twice_waits_once_at_its_earliest_priority() {
    let fixture = Fixture::new();
    let queue = fixture.queue(SEPARATES);
    let blocker = fixture.track("blocker");
    let later = fixture.track("later");
    let raised = fixture.track("raised");
    queue.request_all([
        (blocker.clone(), SeparationPriority::Now),
        (later.clone(), SeparationPriority::Set),
        (raised.clone(), SeparationPriority::Shortlist),
        (raised.clone(), SeparationPriority::Now),
    ]);
    let mut queued = 0;
    let mut ready = Vec::new();
    while ready.len() < 3 {
        let update = next_update(&queue);
        match update.status {
            StemsStatus::Queued => queued += 1,
            StemsStatus::Ready(_) => ready.push(update.track),
            _ => {}
        }
    }
    assert_eq!(queued, 3);
    assert_eq!(ready, [blocker, raised, later]);
}

#[test]
fn a_worker_that_cannot_start_fails_its_track() {
    let fixture = Fixture::new();
    let queue = SeparationQueue::start(fixture.location(), |_: &Path| {
        Command::new("/nonexistent/dubplate-worker")
    })
    .unwrap();
    let track = fixture.track("unstartable");
    queue.request(track.clone(), SeparationPriority::Now);
    let status = wait_for(&queue, &track, |status| !status.is_pending());
    assert!(
        matches!(&status, StemsStatus::Failed(reason) if reason.starts_with("the separation could not start")),
        "{status:?}"
    );
}
