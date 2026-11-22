//! Every effect, grouped by family. Effects of one family share their building blocks: the
//! echoes one delay loop, the rolls one recorder, the stops one turntable.

pub(crate) mod delays;
pub(crate) mod filters;
pub(crate) mod lofi;
pub(crate) mod modulation;
pub(crate) mod pitch;
pub(crate) mod release;
pub(crate) mod reverb;
pub(crate) mod rolls;
