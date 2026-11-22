//! How the app and a separation worker talk: the worker's command line, and the lines it prints
//! on its standard output, one message per line.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::location::{CacheLimit, StemsLocation};
use crate::status::SeparationStage;

/// The first argument that makes a Dubplate executable run as a separation worker.
pub(crate) const WORKER_FLAG: &str = "--dubplate-separate-stems";

/// What a worker separates, and where it keeps the stems.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkerArguments {
    pub(crate) location: StemsLocation,
    pub(crate) track: PathBuf,
}

impl WorkerArguments {
    /// The arguments after the executable: the flag, the cache folder, the weights file, the
    /// cache limit in bytes and the track.
    pub(crate) fn to_args(&self) -> [OsString; 5] {
        [
            WORKER_FLAG.into(),
            self.location.cache_directory().into(),
            self.location.weights_path().into(),
            self.location.limit().bytes().to_string().into(),
            self.track.as_os_str().to_owned(),
        ]
    }

    /// Reads the arguments after the executable. `None` when they do not start with the
    /// worker flag, so the process is not a worker; an error when they start with it but the
    /// rest is not what [`WorkerArguments::to_args`] writes.
    pub(crate) fn parse(
        mut arguments: impl Iterator<Item = OsString>,
    ) -> Option<Result<Self, String>> {
        if arguments.next()? != WORKER_FLAG {
            return None;
        }
        let mut next = |name: &str| {
            arguments
                .next()
                .ok_or_else(|| format!("the separation worker needs {name}"))
        };
        let parsed = (|| {
            let cache = PathBuf::from(next("the stems folder")?);
            let weights = PathBuf::from(next("the weights file")?);
            let limit = next("the cache limit")?
                .to_str()
                .and_then(|bytes| bytes.parse::<u64>().ok())
                .ok_or_else(|| "the cache limit is not a number of bytes".to_owned())?;
            let track = PathBuf::from(next("the track")?);
            Ok(Self {
                location: StemsLocation::new(cache, weights, CacheLimit::from_bytes(limit)),
                track,
            })
        })();
        if parsed.is_ok() && arguments.next().is_some() {
            return Some(Err(
                "the separation worker takes one track at a time".to_owned()
            ));
        }
        Some(parsed)
    }

    /// The track to separate.
    pub(crate) fn track(&self) -> &Path {
        &self.track
    }
}

/// One line a worker prints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WorkerMessage {
    /// A new stage started.
    Stage(SeparationStage),
    /// `done` of the `total` steps of the stage are behind: bytes while downloading, segments
    /// of the track while separating.
    Progress { done: u64, total: u64 },
    /// The stems are in the cache folder named `entry`.
    Ready { entry: String },
    /// The separation failed for `reason`.
    Failed { reason: String },
}

impl WorkerMessage {
    /// The message as one line, without its line break.
    pub(crate) fn to_line(&self) -> String {
        match self {
            WorkerMessage::Stage(stage) => format!("stage {}", stage.code()),
            WorkerMessage::Progress { done, total } => format!("progress {done} {total}"),
            WorkerMessage::Ready { entry } => format!("ready {}", one_line(entry)),
            WorkerMessage::Failed { reason } => format!("failed {}", one_line(reason)),
        }
    }

    /// Reads a line a worker printed. `None` for a line that is not a message.
    pub(crate) fn parse(line: &str) -> Option<Self> {
        let (kind, rest) = line.trim_end().split_once(' ')?;
        match kind {
            "stage" => SeparationStage::from_code(rest).map(WorkerMessage::Stage),
            "progress" => {
                let (done, total) = rest.split_once(' ')?;
                Some(WorkerMessage::Progress {
                    done: done.parse().ok()?,
                    total: total.parse().ok()?,
                })
            }
            "ready" if is_entry_name(rest) => Some(WorkerMessage::Ready {
                entry: rest.to_owned(),
            }),
            "failed" => Some(WorkerMessage::Failed {
                reason: rest.to_owned(),
            }),
            _ => None,
        }
    }
}

/// Whether `name` can only name a folder inside the cache, never one outside it.
fn is_entry_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\'])
        && !name.starts_with('.')
}

fn one_line(text: &str) -> String {
    text.split(['\n', '\r'])
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_message_reads_back_from_its_line() {
        let messages = SeparationStage::ALL
            .into_iter()
            .map(WorkerMessage::Stage)
            .chain([
                WorkerMessage::Progress { done: 3, total: 40 },
                WorkerMessage::Ready {
                    entry: "htdemucs-955717e8-00ff".to_owned(),
                },
                WorkerMessage::Failed {
                    reason: "cannot open the track".to_owned(),
                },
            ]);
        for message in messages {
            assert_eq!(WorkerMessage::parse(&message.to_line()), Some(message));
        }
    }

    #[test]
    fn a_reason_over_several_lines_is_sent_as_one() {
        let message = WorkerMessage::Failed {
            reason: "cannot load the network\ncaused by: out of memory".to_owned(),
        };
        assert_eq!(
            message.to_line(),
            "failed cannot load the network caused by: out of memory"
        );
    }

    #[test]
    fn a_ready_message_never_names_a_folder_outside_the_cache() {
        for line in [
            "ready ../../Music",
            "ready /Users/dj/Music",
            "ready ..",
            "ready .incoming-x",
            "ready ",
        ] {
            assert_eq!(WorkerMessage::parse(line), None, "{line}");
        }
    }

    #[test]
    fn lines_that_are_not_messages_are_ignored() {
        for line in [
            "",
            "hello",
            "progress 1",
            "progress one two",
            "stage dancing",
        ] {
            assert_eq!(WorkerMessage::parse(line), None, "{line}");
        }
    }

    #[test]
    fn worker_arguments_read_back_from_the_command_line() {
        let arguments = WorkerArguments {
            location: StemsLocation::new(
                PathBuf::from("/support/stems"),
                PathBuf::from("/support/models/weights.safetensors"),
                CacheLimit::from_bytes(1_000),
            ),
            track: PathBuf::from("/Music/afro house/Track (Extended Mix).m4a"),
        };
        assert_eq!(
            WorkerArguments::parse(arguments.to_args().into_iter()),
            Some(Ok(arguments.clone()))
        );
        assert_eq!(WorkerArguments::parse(["--other".into()].into_iter()), None);
        assert_eq!(WorkerArguments::parse(std::iter::empty()), None);
        assert!(matches!(
            WorkerArguments::parse([WORKER_FLAG.into()].into_iter()),
            Some(Err(_))
        ));
        let mut too_many = arguments.to_args().to_vec();
        too_many.push("/Music/second.m4a".into());
        assert!(matches!(
            WorkerArguments::parse(too_many.into_iter()),
            Some(Err(_))
        ));
    }
}
