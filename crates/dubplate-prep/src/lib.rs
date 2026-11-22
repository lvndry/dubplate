//! Set preparation for Dubplate.
//!
//! Builds on Euphoria's analysis and harmony rules (key relations, tempo fit and energy
//! direction) so a DJ can pick and order the tracks of a set. Dubplate never builds a set on
//! its own: it suggests and checks, the DJ decides.

mod cue_proposal;
mod energy;
mod energy_cache;
mod energy_queue;
mod ready;
mod store;
mod suggestion;
mod track;
mod warnings;

pub use cue_proposal::{ProposalSource, ProposedCue, ProposedCues, propose_mix_cues};
pub use energy::{EnergyError, EnergyWindow, TrackEnergy, measure_track_energy};
pub use energy_cache::{EnergyCache, EnergyCacheError};
pub use energy_queue::{EnergyQueue, EnergyResult, Priority};
pub use euphoria_core::harmony::EnergyDirection;
pub use euphoria_core::key::Key;
pub use ready::{ReadyProblem, ReadyReport, Severity, TrackReadiness, ready_report};
pub use store::{
    CueSource, EqIntent, Seam, SeamStatus, Set, SetEntry, SetId, SetList, SetStore, StoreError,
};
pub use suggestion::{Suggestion, suggest_next};
pub use track::PrepTrack;
pub use warnings::{SetTrack, SetWarning, WarningKind, WarningPosition, set_warnings};
