//! Which sound card and controllers this Dubplate may open.
//!
//! A developer testing beside the DJ's own Dubplate starts a second one with
//! `DUBPLATE_OFFLINE_DEVICES=1` and its own `HOME`. That copy renders the decks to a silent
//! output, never opens a MIDI port, and keeps its settings and single-instance lock under that
//! `HOME`, so it cannot play through the DJ's speakers or answer the DJ's controller. It can be
//! driven by a script instead: see [`crate::script`].

use std::ffi::OsStr;
use std::path::PathBuf;

const OFFLINE_VARIABLE: &str = "DUBPLATE_OFFLINE_DEVICES";
const SCRIPT_VARIABLE: &str = "DUBPLATE_SCRIPT";

/// Whether Dubplate opens the sound cards and controllers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceAccess {
    /// The sound card the DJ chose and every controller plugged in.
    Real,
    /// A silent output and no controller, with an optional script standing in for one.
    Offline {
        /// The file or named pipe the script is read from.
        script: Option<PathBuf>,
    },
}

impl DeviceAccess {
    /// What the environment asks for: offline when `DUBPLATE_OFFLINE_DEVICES` is set to
    /// anything but `0` or nothing, with the script at `DUBPLATE_SCRIPT`.
    pub fn from_environment() -> Self {
        Self::from_values(
            std::env::var_os(OFFLINE_VARIABLE).as_deref(),
            std::env::var_os(SCRIPT_VARIABLE),
        )
    }

    fn from_values(offline: Option<&OsStr>, script: Option<std::ffi::OsString>) -> Self {
        match offline {
            Some(value) if !value.is_empty() && value != "0" => Self::Offline {
                script: script.filter(|path| !path.is_empty()).map(PathBuf::from),
            },
            _ => Self::Real,
        }
    }

    /// Whether the sound cards and controllers stay closed.
    pub fn is_offline(&self) -> bool {
        matches!(self, Self::Offline { .. })
    }
}

/// The buffer the sound card is fed in: a smaller one answers the controller sooner, a larger
/// one gives the Mac more time and so drops out less, as rekordbox's Preferences > Audio >
/// Buffer Size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferSize {
    /// 128 frames, about 2.7 ms at 48 kHz.
    Frames128,
    /// 256 frames, about 5.3 ms at 48 kHz.
    Frames256,
    /// 512 frames, about 10.7 ms at 48 kHz.
    Frames512,
    /// 1024 frames, about 21.3 ms at 48 kHz.
    Frames1024,
}

impl BufferSize {
    /// Every size, smallest first.
    pub const ALL: [Self; 4] = [
        Self::Frames128,
        Self::Frames256,
        Self::Frames512,
        Self::Frames1024,
    ];

    /// The size Dubplate starts with.
    pub const DEFAULT: Self = Self::Frames256;

    /// The frames in one buffer.
    pub fn frames(self) -> u32 {
        match self {
            Self::Frames128 => 128,
            Self::Frames256 => 256,
            Self::Frames512 => 512,
            Self::Frames1024 => 1024,
        }
    }

    /// How long one buffer lasts at `sample_rate`, in milliseconds.
    pub fn milliseconds(self, sample_rate: u32) -> f64 {
        f64::from(self.frames()) * 1000.0 / f64::from(sample_rate.max(1))
    }

    /// The size kept in the settings as a number of frames, or the default.
    pub fn from_setting(frames: Option<u32>) -> Self {
        frames
            .and_then(|frames| Self::ALL.into_iter().find(|size| size.frames() == frames))
            .unwrap_or(Self::DEFAULT)
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::*;

    #[test]
    fn a_buffer_size_comes_back_from_its_frames() {
        assert_eq!(BufferSize::from_setting(Some(512)), BufferSize::Frames512);
        assert_eq!(BufferSize::from_setting(Some(300)), BufferSize::DEFAULT);
        assert_eq!(BufferSize::from_setting(None), BufferSize::DEFAULT);
        assert!((BufferSize::Frames256.milliseconds(48_000) - 5.333).abs() < 1e-3);
    }

    #[test]
    fn devices_are_real_unless_offline_is_asked_for() {
        assert_eq!(DeviceAccess::from_values(None, None), DeviceAccess::Real);
        assert_eq!(
            DeviceAccess::from_values(Some(OsStr::new("0")), None),
            DeviceAccess::Real
        );
        assert_eq!(
            DeviceAccess::from_values(Some(OsStr::new("")), Some(OsString::from("/tmp/script"))),
            DeviceAccess::Real
        );
    }

    #[test]
    fn offline_devices_take_the_script_with_them() {
        assert_eq!(
            DeviceAccess::from_values(Some(OsStr::new("1")), None),
            DeviceAccess::Offline { script: None }
        );
        assert_eq!(
            DeviceAccess::from_values(Some(OsStr::new("1")), Some(OsString::from("/tmp/script"))),
            DeviceAccess::Offline {
                script: Some(PathBuf::from("/tmp/script"))
            }
        );
    }
}
