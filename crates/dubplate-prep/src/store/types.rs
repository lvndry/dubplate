//! The types a prepared set is made of, as the store stores and returns them.

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
    /// The planned transitions between consecutive tracks.
    pub seams: Vec<Seam>,
}

/// One track in a set and what mixing it needs to know.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SetEntry {
    /// rekordbox's identifier for the track.
    pub rekordbox_id: String,
    /// Where the incoming track starts, in milliseconds.
    pub mix_in_ms: Option<u32>,
    /// Where the outgoing track ends, in milliseconds.
    pub mix_out_ms: Option<u32>,
    /// Where the cues came from, once they have a source.
    pub cue_source: Option<CueSource>,
    /// Whether the DJ has confirmed the phrase grid.
    pub grid_confirmed: bool,
    /// The DJ's own energy rating, kept apart from the analysis.
    pub energy_override: Option<f32>,
    /// Where the vocals start, in milliseconds, when tagged.
    pub vocal_in_ms: Option<u32>,
    /// Where the vocals end, in milliseconds, when tagged.
    pub vocal_out_ms: Option<u32>,
    /// The DJ's note on the track.
    pub note: String,
    /// Whether the track is a backup, kept off the main order.
    pub is_backup: bool,
}

/// The plan for the transition after the track at `from_position`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Seam {
    /// The position of the outgoing track.
    pub from_position: usize,
    /// How many bars the two tracks overlap.
    pub overlap_bars: u32,
    /// What the equalizer should do across the overlap.
    pub eq_intent: EqIntent,
    /// Planned, or free to be felt live.
    pub status: SeamStatus,
    /// The DJ's note on the transition.
    pub note: String,
}
