//! Set preparation for Dubplate.
//!
//! Builds on Euphoria's analysis and harmony rules (key relations, tempo fit and energy
//! direction) so a DJ can pick and order the tracks of a set. Dubplate never builds a set on
//! its own: it suggests and checks, the DJ decides. A prepared set goes to rekordbox as a
//! rekordbox XML file, never by writing rekordbox's own files.

mod cue_proposal;
mod energy;
mod energy_cache;
mod energy_queue;
mod export;
mod grid;
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
pub use export::{
    ExportError, ExportLibrary, ExportTrack, MIX_IN_CUE_NAME, MIX_OUT_CUE_NAME,
    REKORDBOX_IMPORT_STEPS, RekordboxCollection, SetExport, default_export_folder,
    export_file_name, file_track, folder_track, rekordbox_xml, tempo_markers,
};
pub use grid::{BEATS_PER_BAR, GridCorrection};
pub use ready::{ReadyProblem, ReadyReport, Severity, TrackReadiness, ready_report};
pub use store::{
    CueSource, EqIntent, Seam, SeamStatus, Set, SetEntry, SetId, SetList, SetStore, StoreError,
};
pub use suggestion::{KeyFit, Suggestion, suggest_next};
pub use track::PrepTrack;
pub use warnings::{SetTrack, SetWarning, WarningKind, WarningPosition, set_warnings};
