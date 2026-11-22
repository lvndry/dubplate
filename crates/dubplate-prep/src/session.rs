//! Serialized set edits off the interface thread, with immutable committed snapshots for views.

use std::path::Path;
use std::sync::{Arc, mpsc};
use std::time::Duration;

use crate::{SeamStatus, Set, SetEditor, SetEntry, StoreError};

type EditJob = Box<dyn FnOnce(&mut SetEditor, u64) + Send>;

/// A background set worker and the newest committed snapshot accepted by its view.
///
/// Submitting never waits for database work. Dropping this session disconnects the command
/// channel; its worker finishes already submitted edits before exiting, without a UI join.
/// A dropped result does not cancel its edit. This does not keep the process alive on exit.
pub struct SetSession {
    sender: mpsc::Sender<EditJob>,
    snapshot: Arc<Set>,
    revision: u64,
    identity: Arc<()>,
}

/// Why submitting or receiving an edit failed, separately from a database operation's error.
#[derive(Debug, thiserror::Error)]
pub enum SetSessionError {
    /// The set worker stopped before it could accept or return the edit.
    #[error("the set editing worker stopped unexpectedly")]
    WorkerStopped,
}

/// One submitted edit's result, received on a background executor rather than the UI thread.
#[must_use = "receive this result off the UI thread and apply its committed snapshot"]
pub struct SetJob<ResultValue> {
    receiver: mpsc::Receiver<SetCompletion<ResultValue>>,
}

impl<ResultValue> SetJob<ResultValue> {
    /// Waits for the worker's answer. This blocks; use a background thread or executor.
    pub fn wait(self) -> Result<SetCompletion<ResultValue>, SetSessionError> {
        self.receiver
            .recv()
            .map_err(|_| SetSessionError::WorkerStopped)
    }
}

/// An edit's outcome and the committed set after that edit, including when it failed.
///
/// Applying a completion updates only the read-only view; it never writes the database.
/// A multi-step edit may have committed earlier steps before returning an error, so the
/// snapshot always reflects the actual editor state, not an assumed rollback.
pub struct SetCompletion<ResultValue> {
    /// Monotonic worker order, independent of when its awaiting UI tasks resume.
    pub revision: u64,
    /// The set as committed after the edit.
    pub set: Arc<Set>,
    /// The operation's return value or storage failure.
    pub outcome: Result<ResultValue, StoreError>,
    identity: Arc<()>,
}

impl SetSession {
    /// Moves an open editor to its own worker and retains its initial immutable view.
    /// Opening/migrating the editor itself should also happen off the UI thread.
    pub fn new(mut editor: SetEditor) -> std::io::Result<Self> {
        let snapshot = Arc::new(editor.set().clone());
        let (sender, receiver) = mpsc::channel::<EditJob>();
        std::thread::Builder::new()
            .name("dubplate-set-editor".to_owned())
            .spawn(move || {
                let mut revision = 0_u64;
                for edit in receiver {
                    revision = revision.saturating_add(1);
                    edit(&mut editor, revision);
                }
            })?;
        Ok(Self {
            sender,
            snapshot,
            revision: 0,
            identity: Arc::new(()),
        })
    }

    /// Queues an edit in submission order, without waiting for prior edits or database I/O.
    /// The closure owns its inputs and runs only on the worker, even if its result is dropped.
    pub fn submit<ResultValue: Send + 'static>(
        &self,
        edit: impl FnOnce(&mut SetEditor) -> Result<ResultValue, StoreError> + Send + 'static,
    ) -> Result<SetJob<ResultValue>, SetSessionError> {
        let (sender, receiver) = mpsc::channel();
        let identity = Arc::clone(&self.identity);
        self.sender
            .send(Box::new(move |editor, revision| {
                let outcome = edit(editor);
                let completion = SetCompletion {
                    revision,
                    set: Arc::new(editor.set().clone()),
                    outcome,
                    identity,
                };
                // The edit remains committed if the window/result task has gone away.
                let _ = sender.send(completion);
            }))
            .map_err(|_| SetSessionError::WorkerStopped)?;
        Ok(SetJob { receiver })
    }

    /// Accepts a newer snapshot from this session, rejecting stale or foreign completions.
    /// Returns whether the view advanced; older completions must not move UI selection back.
    pub fn apply<ResultValue>(&mut self, completion: &SetCompletion<ResultValue>) -> bool {
        if !Arc::ptr_eq(&self.identity, &completion.identity)
            || completion.revision <= self.revision
        {
            return false;
        }
        self.snapshot = Arc::clone(&completion.set);
        self.revision = completion.revision;
        true
    }

    /// The newest committed set accepted by the view; no worker lock is needed to read it.
    pub fn set(&self) -> &Set {
        &self.snapshot
    }

    /// Shares the current immutable set without cloning its entries on the UI thread.
    pub fn snapshot(&self) -> Arc<Set> {
        Arc::clone(&self.snapshot)
    }

    /// How long this set aims to run.
    pub fn target(&self) -> Duration {
        Duration::from_secs(u64::from(self.snapshot.target_minutes) * 60)
    }

    /// The set's tracks, in playing order.
    pub fn tracks(&self) -> Vec<&SetEntry> {
        self.snapshot.main_entries().collect()
    }

    /// The backup tracks, in their saved order.
    pub fn backups(&self) -> Vec<&SetEntry> {
        self.snapshot
            .entries
            .iter()
            .filter(|entry| entry.is_backup)
            .collect()
    }

    /// Where this file appears in the main set.
    pub fn position(&self, path: &Path) -> Option<usize> {
        self.snapshot
            .main_entries()
            .position(|entry| entry.track_path == path)
    }

    /// Whether the seam after this position is left to the moment.
    pub fn is_free_after(&self, position: usize) -> bool {
        self.snapshot
            .seam_after(position)
            .is_some_and(|seam| seam.status == SeamStatus::Free)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::SetTrackRef;

    use super::*;

    fn editor(directory: &tempfile::TempDir) -> SetEditor {
        SetEditor::open(&directory.path().join("sets.sqlite")).unwrap()
    }

    fn track(name: &str) -> SetTrackRef {
        SetTrackRef {
            path: PathBuf::from(format!("/Music/{name}.mp3")),
            rekordbox_id: None,
        }
    }

    #[test]
    fn queued_edits_are_serial_and_old_completions_cannot_rewind_the_view() {
        let directory = tempfile::tempdir().unwrap();
        let mut session = SetSession::new(editor(&directory)).unwrap();
        let (started, started_rx) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        let first = session
            .submit(move |editor| {
                started.send(()).unwrap();
                release_rx.recv().unwrap();
                editor.add(track("a"))
            })
            .unwrap();
        started_rx.recv().unwrap();
        let second = session.submit(|editor| editor.add(track("b"))).unwrap();
        assert!(
            session.tracks().is_empty(),
            "submission and reads do not wait for the blocked worker"
        );
        release.send(()).unwrap();
        let second = second.wait().unwrap();
        assert_eq!(second.outcome.as_ref().unwrap(), &1);
        assert!(session.apply(&second));
        let first = first.wait().unwrap();
        assert_eq!(first.outcome.as_ref().unwrap(), &0);
        assert!(!session.apply(&first));
        assert_eq!(session.tracks().len(), 2);
    }

    #[test]
    fn dropping_results_or_the_session_does_not_cancel_queued_writes() {
        let directory = tempfile::tempdir().unwrap();
        let session = SetSession::new(editor(&directory)).unwrap();
        drop(session.submit(|editor| editor.add(track("a"))).unwrap());
        let last = session.submit(|editor| editor.add(track("b"))).unwrap();
        drop(session);
        let completion = last.wait().unwrap();
        assert_eq!(completion.outcome.unwrap(), 1);
        assert_eq!(editor(&directory).tracks().len(), 2);
    }

    #[test]
    fn storage_errors_return_the_committed_snapshot_and_leave_the_worker_usable() {
        let directory = tempfile::tempdir().unwrap();
        let mut session = SetSession::new(editor(&directory)).unwrap();
        let connection = rusqlite::Connection::open(directory.path().join("sets.sqlite")).unwrap();
        connection
            .execute_batch(
                "CREATE TRIGGER fail_entry BEFORE INSERT ON set_entry BEGIN
             SELECT RAISE(ABORT, 'injected write failure'); END;",
            )
            .unwrap();
        let failed = session
            .submit(|editor| editor.add(track("a")))
            .unwrap()
            .wait()
            .unwrap();
        assert!(failed.outcome.is_err());
        assert!(session.apply(&failed));
        assert!(session.tracks().is_empty());
        connection
            .execute_batch("DROP TRIGGER fail_entry;")
            .unwrap();
        let next = session
            .submit(|editor| editor.add(track("b")))
            .unwrap()
            .wait()
            .unwrap();
        assert!(next.outcome.is_ok());
        assert!(session.apply(&next));
        assert_eq!(session.tracks()[0].track_path, track("b").path);
    }

    #[test]
    fn completions_from_another_session_are_not_accepted() {
        let first_directory = tempfile::tempdir().unwrap();
        let second_directory = tempfile::tempdir().unwrap();
        let mut first = SetSession::new(editor(&first_directory)).unwrap();
        let second = SetSession::new(editor(&second_directory)).unwrap();
        let foreign = second
            .submit(|editor| editor.add(track("a")))
            .unwrap()
            .wait()
            .unwrap();
        assert!(!first.apply(&foreign));
        assert!(first.tracks().is_empty());
    }
}
