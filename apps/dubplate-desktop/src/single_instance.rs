//! One Dubplate at a time. Each copy opens the sound card and the controller on its own, so with
//! two running, one press of LOAD or PLAY on the controller plays the track twice, a little
//! apart, on the same speakers.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Why this copy of Dubplate did not become the running one.
#[derive(Debug, thiserror::Error)]
pub enum InstanceError {
    /// Another Dubplate holds the lock.
    #[error("Dubplate is already running{}", running_process(*.process_id))]
    AlreadyRunning {
        /// The other Dubplate's process, when it wrote it down.
        process_id: Option<u32>,
    },
    /// The lock file could not be opened, locked or written.
    #[error("{path}: {source}")]
    Io {
        /// The lock file.
        path: PathBuf,
        /// What went wrong.
        source: std::io::Error,
    },
}

fn running_process(process_id: Option<u32>) -> String {
    process_id.map_or_else(String::new, |process_id| format!(" (process {process_id})"))
}

/// The lock that makes this copy the running Dubplate. The system releases it when the process
/// ends, however it ends, so a crash never leaves Dubplate unable to start.
#[derive(Debug)]
pub struct InstanceLock {
    _file: File,
}

impl InstanceLock {
    /// Takes the lock at `path` and writes this process's identifier in it, or reports the
    /// Dubplate that holds it.
    pub fn acquire(path: &Path) -> Result<Self, InstanceError> {
        let io_error = |source| InstanceError::Io {
            path: path.to_path_buf(),
            source,
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(io_error)?;
        }
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(io_error)?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                return Err(InstanceError::AlreadyRunning {
                    process_id: read_process_id(&mut file),
                });
            }
            Err(TryLockError::Error(source)) => return Err(io_error(source)),
        }
        file.set_len(0).map_err(io_error)?;
        file.seek(SeekFrom::Start(0)).map_err(io_error)?;
        write!(file, "{}", std::process::id()).map_err(io_error)?;
        file.flush().map_err(io_error)?;
        Ok(Self { _file: file })
    }
}

fn read_process_id(file: &mut File) -> Option<u32> {
    let mut text = String::new();
    file.read_to_string(&mut text).ok()?;
    text.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_dubplate_finds_the_first_one_running() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("nested").join("dubplate.lock");
        let first = InstanceLock::acquire(&path).unwrap();
        let second = InstanceLock::acquire(&path).unwrap_err();
        assert!(matches!(
            second,
            InstanceError::AlreadyRunning {
                process_id: Some(process_id)
            } if process_id == std::process::id()
        ));
        assert_eq!(
            second.to_string(),
            format!(
                "Dubplate is already running (process {})",
                std::process::id()
            )
        );
        drop(first);
        InstanceLock::acquire(&path).unwrap();
    }

    #[test]
    fn a_lock_file_left_behind_does_not_stop_dubplate() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("dubplate.lock");
        std::fs::write(&path, "4294967295 left over").unwrap();
        let _lock = InstanceLock::acquire(&path).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            std::process::id().to_string()
        );
    }
}
