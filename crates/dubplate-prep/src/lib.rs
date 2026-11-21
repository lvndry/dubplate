//! Set preparation for Dubplate.
//!
//! Builds on Euphoria's analysis and harmony rules (key relations, tempo fit and energy
//! direction) so a DJ can pick and order the tracks of a set. Dubplate never builds a set on
//! its own: it suggests and checks, the DJ decides.

mod suggestion;
mod track;

pub use euphoria_core::harmony::EnergyDirection;
pub use euphoria_core::key::Key;
pub use suggestion::{Suggestion, suggest_next};
pub use track::PrepTrack;
