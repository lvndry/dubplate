//! Where each track's stems stand.

use std::path::PathBuf;

use euphoria_core::stems as separation;

use crate::files::StemFiles;

/// Where a track's stems stand.
#[derive(Debug, Clone, PartialEq)]
pub enum StemsStatus {
    /// The track has no stems and none were asked for.
    None,
    /// The track waits for the tracks ahead of it to be separated.
    Queued,
    /// The track is being separated.
    Separating(SeparationProgress),
    /// The stems are ready to play.
    Ready(StemFiles),
    /// The separation failed, for the reason given.
    Failed(String),
}

impl StemsStatus {
    /// The stems, when they are ready.
    pub fn files(&self) -> Option<&StemFiles> {
        match self {
            StemsStatus::Ready(files) => Some(files),
            StemsStatus::None
            | StemsStatus::Queued
            | StemsStatus::Separating(_)
            | StemsStatus::Failed(_) => None,
        }
    }

    /// Whether the track waits for or is in a separation.
    pub fn is_pending(&self) -> bool {
        matches!(self, StemsStatus::Queued | StemsStatus::Separating(_))
    }
}

/// A change of a track's stems status.
#[derive(Debug, Clone, PartialEq)]
pub struct StemsUpdate {
    /// The track's audio file.
    pub track: PathBuf,
    /// Where its stems stand now.
    pub status: StemsStatus,
}

/// What a separation is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeparationStage {
    /// Starting the worker.
    Starting,
    /// Waiting for another separation into the same folder to finish.
    Waiting,
    /// Downloading the network's weights, the first time only.
    DownloadingModel,
    /// Loading the network.
    LoadingModel,
    /// Decoding the track.
    Decoding,
    /// Running the network over the track.
    Separating,
    /// Writing the stems.
    Saving,
}

impl SeparationStage {
    /// Every stage, in the order a separation goes through them.
    pub const ALL: [SeparationStage; 7] = [
        SeparationStage::Starting,
        SeparationStage::Waiting,
        SeparationStage::DownloadingModel,
        SeparationStage::LoadingModel,
        SeparationStage::Decoding,
        SeparationStage::Separating,
        SeparationStage::Saving,
    ];

    /// A few words for the DJ, such as `separating`.
    pub fn label(self) -> &'static str {
        match self {
            SeparationStage::Starting => "starting",
            SeparationStage::Waiting => "waiting",
            SeparationStage::DownloadingModel => "downloading the model",
            SeparationStage::LoadingModel => "loading the model",
            SeparationStage::Decoding => "decoding",
            SeparationStage::Separating => "separating",
            SeparationStage::Saving => "saving",
        }
    }

    /// The name in the worker's messages.
    pub(crate) fn code(self) -> &'static str {
        match self {
            SeparationStage::Starting => "starting",
            SeparationStage::Waiting => "waiting",
            SeparationStage::DownloadingModel => "downloading-model",
            SeparationStage::LoadingModel => "loading-model",
            SeparationStage::Decoding => "decoding",
            SeparationStage::Separating => "separating",
            SeparationStage::Saving => "saving",
        }
    }

    /// The stage named `code` in a worker's message.
    pub(crate) fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|stage| stage.code() == code)
    }
}

impl From<separation::SeparationStage> for SeparationStage {
    fn from(stage: separation::SeparationStage) -> Self {
        match stage {
            separation::SeparationStage::Waiting => SeparationStage::Waiting,
            separation::SeparationStage::LoadingModel => SeparationStage::LoadingModel,
            separation::SeparationStage::Decoding => SeparationStage::Decoding,
            separation::SeparationStage::Separating => SeparationStage::Separating,
            separation::SeparationStage::Saving => SeparationStage::Saving,
        }
    }
}

/// How far a separation has come.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SeparationProgress {
    stage: SeparationStage,
    fraction: f32,
}

impl SeparationProgress {
    /// At `stage`, with `done` of `total` steps of it behind.
    pub fn new(stage: SeparationStage, done: u64, total: u64) -> Self {
        let fraction = if total == 0 {
            0.0
        } else {
            (done as f64 / total as f64).clamp(0.0, 1.0) as f32
        };
        Self { stage, fraction }
    }

    /// What the separation is doing.
    pub fn stage(self) -> SeparationStage {
        self.stage
    }

    /// How much of the stage is done, from 0 to 1.
    pub fn fraction(self) -> f32 {
        self.fraction
    }

    /// How much of the whole separation is done, from 0 to 1, for one progress bar: running the
    /// network is nearly all of the time, so the other stages count as its start or its end.
    pub fn overall(self) -> f32 {
        match self.stage {
            SeparationStage::Starting
            | SeparationStage::Waiting
            | SeparationStage::DownloadingModel
            | SeparationStage::LoadingModel
            | SeparationStage::Decoding => 0.0,
            SeparationStage::Separating => self.fraction,
            SeparationStage::Saving => 1.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_stage_reads_back_from_its_code() {
        for stage in SeparationStage::ALL {
            assert_eq!(SeparationStage::from_code(stage.code()), Some(stage));
        }
        assert_eq!(SeparationStage::from_code("dancing"), None);
    }

    #[test]
    fn progress_stays_between_nothing_and_all() {
        let halfway = SeparationProgress::new(SeparationStage::Separating, 20, 40);
        assert_eq!(halfway.fraction(), 0.5);
        assert_eq!(halfway.overall(), 0.5);
        assert_eq!(
            SeparationProgress::new(SeparationStage::Separating, 9, 0).fraction(),
            0.0
        );
        assert_eq!(
            SeparationProgress::new(SeparationStage::Separating, 50, 40).fraction(),
            1.0
        );
        assert_eq!(
            SeparationProgress::new(SeparationStage::Decoding, 1, 2).overall(),
            0.0
        );
        assert_eq!(
            SeparationProgress::new(SeparationStage::Saving, 0, 1).overall(),
            1.0
        );
    }
}
