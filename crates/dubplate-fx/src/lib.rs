//! Real-time DJ effects for Dubplate.
//!
//! Every effect implements [`Effect`]: it processes a block of planar stereo `f32` audio in
//! place, given a [`ProcessContext`] with the sample rate, the tempo and the musical position of
//! the block. [`Effect::process`] never allocates, locks or blocks. Each effect allocates its
//! buffers once, in its constructor, sized by an [`EffectConfig`] for the highest sample rate and
//! the longest delay it will ever be asked for.
//!
//! The behaviour a DJ expects is shared by every effect rather than written into each one:
//!
//! - Every knob is smoothed, so moving it never clicks or zippers.
//! - Switching an effect on or off fades instead of cutting. Echo, ping-pong and reverb keep
//!   their trails: when switched off they stop taking in new audio and let what they hold ring
//!   out, while the dry signal comes back to full level.
//! - Beat lengths follow the tempo of every block, so a tempo change moves echoes and sweeps
//!   with the music. Small tempo moves glide, a new beat length crossfades to the new time.
//! - Feedback paths saturate gently and flush denormals, and the output is checked for NaN and
//!   infinity, so nothing a DJ does can blow up the output or the CPU.
//!
//! ```
//! use dubplate_fx::{BeatLength, Echo, Effect, EffectConfig, ProcessContext};
//!
//! let mut echo = Echo::new(&EffectConfig::default());
//! echo.set_beat(BeatLength::HALF);
//! echo.set_feedback(0.6);
//! echo.set_enabled(true);
//!
//! let mut left = vec![0.0_f32; 512];
//! let mut right = vec![0.0_f32; 512];
//! echo.process(&mut left, &mut right, &ProcessContext::new(48_000.0, 124.0, 0.0));
//! ```

mod beat;
mod config;
mod context;
mod dsp;
mod effect;
mod effects;
mod unit;

pub use beat::{BeatLength, ParseBeatLengthError};
pub use config::EffectConfig;
pub use context::{MAX_TEMPO_BPM, MIN_TEMPO_BPM, ProcessContext};
pub use effect::{Effect, EffectKind, ParseEffectKindError, create};
pub use effects::backspin::Backspin;
pub use effects::brake::Brake;
pub use effects::crusher::Crusher;
pub use effects::echo::Echo;
pub use effects::filter::{Filter, FilterMode, FilterSlope};
pub use effects::flanger::Flanger;
pub use effects::gate::Gate;
pub use effects::phaser::Phaser;
pub use effects::ping_pong::PingPong;
pub use effects::reverb::Reverb;
pub use effects::sweep::SweepFilter;
