//! Set preparation for Dubplate.
//!
//! Builds on Euphoria's analysis and harmony rules (key relations, tempo fit and energy
//! direction) so a DJ can pick and order the tracks of a set. Dubplate never builds a set on
//! its own: it suggests and checks, the DJ decides.

mod cue_proposal;
mod ready;
mod store;
mod suggestion;
mod track;

pub use cue_proposal::{ProposalSource, ProposedCue, ProposedCues, propose_mix_cues};
pub use euphoria_core::harmony::EnergyDirection;
pub use euphoria_core::key::Key;
pub use ready::{ReadyProblem, ReadyReport, Severity, TrackReadiness, ready_report};
pub use store::{
    CueSource, EqIntent, Seam, SeamStatus, Set, SetEntry, SetId, SetStore, StoreError,
};
pub use suggestion::{Suggestion, suggest_next};
pub use track::PrepTrack;
