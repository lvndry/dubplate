//! The types a prepared set is made of, as the store stores and returns them.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Dubplate's identifier for a prepared set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SetId(pub i64);

impl SetId {
    /// The raw database row id.
    pub fn as_i64(self) -> i64 {
        self.0
    }
}

impl std::fmt::Display for SetId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0.to_string())
    }
}

/// How a seam will happen: planned in advance, or left free to be felt live.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeamStatus {
    /// The seam has a plan the DJ will follow.
    Planned,
    /// The DJ will feel the transition live; no plan, no warnings.
    Free,
}

/// Where a track's cues came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CueSource {
    /// rekordbox's own cues.
    Rekordbox,
    /// Cues Euphoria proposed.
    Euphoria,
    /// The DJ set them by hand.
    Manual,
}

/// What the DJ wants the equalizer to do across a seam.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EqIntent {
    /// Move the bass to the incoming track.
    BassSwap,
    /// High-pass the incoming track while it rides in.
    HighPassIncoming,
    /// A long filter blend across the overlap.
    LongFilterBlend,
    /// No equalizer plan for this seam.
    None,
}

/// Which track list an entry operation applies to.
///
/// Main and backups keep separate positions: editing one never shifts the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetList {
    /// The tracks in play order.
    Main,
    /// The backup tracks, kept off the main order.
    Backups,
}

impl SetList {
    /// Whether an entry in this list is stored as a backup.
    pub fn is_backup(self) -> bool {
        matches!(self, Self::Backups)
    }
}

/// A prepared set: its metadata, ordered entries and seam plans.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Set {
    /// The set's identifier.
    pub id: SetId,
    /// The set's name.
    pub name: String,
    /// How long the set should run.
    pub target_minutes: u32,
    /// When the set was created, Unix milliseconds.
    pub created_at: i64,
    /// When the set was last edited, Unix milliseconds.
    pub updated_at: i64,
    /// The tracks in play order, backup tracks last.
    pub entries: Vec<SetEntry>,
    /// The planned transitions between named pairs of tracks.
    pub seams: Vec<Seam>,
}

impl Set {
    /// The non-backup tracks in play order.
    pub fn main_entries(&self) -> impl Iterator<Item = &SetEntry> {
        self.entries.iter().filter(|candidate| !candidate.is_backup)
    }

    /// The seam plan for the main pair at `position` and `position + 1`, when stored.
    ///
    /// Seams are named by the two tracks they join, not by a position, so a
    /// reorder never moves a plan onto another pair.
    pub fn seam_after(&self, position: usize) -> Option<&Seam> {
        let from_path = self.main_entries().nth(position)?.track_path.clone();
        let to_path = self.main_entries().nth(position + 1)?.track_path.clone();
        self.seams.iter().find(|candidate| {
            candidate.from_track_path == from_path && candidate.to_track_path == to_path
        })
    }
}

/// One track in a set and what mixing it needs to know.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SetEntry {
    /// The audio file of the track; its identity in a set.
    pub track_path: PathBuf,
    /// rekordbox's identifier for the track, when it has one, so a moved file
    /// can be found again.
    pub rekordbox_id: Option<String>,
    /// Where the incoming track starts, in milliseconds.
    pub mix_in_ms: Option<u32>,
    /// Where the outgoing track ends, in milliseconds.
    pub mix_out_ms: Option<u32>,
    /// Where the cues came from, once they have a source.
    pub cue_source: Option<CueSource>,
    /// Whether the DJ has confirmed the phrase grid.
    pub grid_confirmed: bool,
    /// How far the DJ moved rekordbox's grid, in milliseconds, later when positive.
    pub grid_offset_ms: i32,
    /// How many beats the DJ moved the downbeats, later when positive, for grids whose bars
    /// start on the wrong beat.
    pub downbeat_shift: i32,
    /// The DJ's own energy rating, kept apart from the analysis.
    pub energy_override: Option<f32>,
    /// Where the vocals start, in milliseconds, when tagged.
    pub vocal_in_ms: Option<u32>,
    /// Where the vocals end, in milliseconds, when tagged.
    pub vocal_out_ms: Option<u32>,
    /// The DJ's note on the track.
    pub note: String,
    /// Whether the track is a backup, kept off the main order. The store sets
    /// this from the list an entry is written to.
    pub is_backup: bool,
}

/// The plan for the transition between two named tracks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Seam {
    /// The audio file of the outgoing track.
    pub from_track_path: PathBuf,
    /// The audio file of the incoming track.
    pub to_track_path: PathBuf,
    /// How many bars the two tracks overlap.
    pub overlap_bars: u32,
    /// What the equalizer should do across the overlap.
    pub eq_intent: EqIntent,
    /// Planned, or free to be felt live.
    pub status: SeamStatus,
    /// The DJ's note on the transition.
    pub note: String,
}
