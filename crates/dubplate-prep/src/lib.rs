//! Set preparation for Dubplate.
//!
//! Builds on Euphoria's analysis and harmony rules (key relations, tempo fit and energy
//! direction) so a DJ can pick and order the tracks of a set. Dubplate never builds a set on
//! its own: it suggests and checks, the DJ decides. A prepared set goes to rekordbox as a
//! rekordbox XML file, never by writing rekordbox's own files.
//!
//! Tracks rekordbox never analysed get Dubplate's own analysis, from Euphoria's detectors: a
//! tempo, a beat grid with its downbeats, a key and an energy, from one decode, in the
//! background and cached.

mod analysis;
mod analysis_cache;
mod analysis_queue;
mod cue_proposal;
mod downbeat;
mod editor;
mod energy;
mod export;
mod grid;
mod identity;
mod ready;
mod session;
mod store;
mod suggestion;
mod track;
mod warnings;

pub use analysis::{
    AnalysedGrid, AnalysisError, AudioAnalysis, analyse_audio, analyse_track, analysis_version,
};
pub use analysis_cache::{AnalysisCache, AnalysisCacheError};
pub use analysis_queue::{AnalysisQueue, AnalysisResult, Priority};
pub use cue_proposal::{ProposalSource, ProposedCue, ProposedCues, propose_mix_cues};
pub use editor::{SetEditor, SetTrackRef};
pub use energy::{EnergyWindow, TrackEnergy};
pub use euphoria_core::harmony::{EnergyDirection, Relation};
pub use euphoria_core::key::Key;
pub use export::{
    ExportError, ExportLibrary, ExportTrack, MIX_IN_CUE_NAME, MIX_OUT_CUE_NAME,
    REKORDBOX_IMPORT_STEPS, RekordboxCollection, SetExport, default_export_folder,
    export_file_name, file_track, folder_track, rekordbox_xml, tempo_markers,
};
pub use grid::{BEATS_PER_BAR, GridCorrection, GridEdit, TrackGrid};
pub use identity::TrackIdentity;
pub use ready::{ReadyProblem, ReadyReport, Severity, TrackReadiness, ready_report};
pub use session::{SetCompletion, SetJob, SetSession, SetSessionError};
pub use store::{
    CueSource, EqIntent, Play, PlayDeck, PlayId, RecordedPlay, Seam, SeamStatus, Set, SetEntry,
    SetId, SetList, SetStore, StoreError,
};
pub use suggestion::{KeyFit, Suggestion, suggest_next};
pub use track::PrepTrack;
pub use warnings::{SetTrack, SetWarning, WarningKind, WarningPosition, set_warnings};
