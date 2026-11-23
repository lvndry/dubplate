//! The sampler sounds shipped with Dubplate and the keys used to restore them.

use dubplate_engine::{
    EngineError, SampleMode, SampleSettings, SynthSound, Track, decode_bytes_to_end,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltInSample {
    Recorded(RecordedSample),
    Synth(SynthSound),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordedSample {
    TumbaLow,
    BongoMuted,
    AgogoLow,
    RoomClap,
    ShakerGroove,
    ChordCm7,
}

struct Recording {
    key: &'static str,
    name: &'static str,
    file: &'static str,
    bytes: &'static [u8],
    loop_bpm: Option<f64>,
}

macro_rules! recording {
    ($key:literal, $name:literal, $bpm:expr) => {
        Recording {
            key: $key,
            name: $name,
            file: concat!($key, ".flac"),
            bytes: include_bytes!(concat!("../assets/sampler/", $key, ".flac")),
            loop_bpm: $bpm,
        }
    };
}

impl RecordedSample {
    fn recording(self) -> Recording {
        match self {
            Self::TumbaLow => recording!("tumba-low", "Low tumba", None),
            Self::BongoMuted => recording!("bongo-muted", "Muted bongo", None),
            Self::AgogoLow => recording!("agogo-low", "Low agogo", None),
            Self::RoomClap => recording!("room-clap", "Room clap", None),
            Self::ShakerGroove => recording!("shaker-groove", "Shaker groove", Some(120.0)),
            Self::ChordCm7 => recording!("chord-cm7", "Cm7 chord stab", None),
        }
    }
}

impl BuiltInSample {
    pub const HOUSE_SOUNDS: [Self; 6] = [
        Self::Recorded(RecordedSample::TumbaLow),
        Self::Recorded(RecordedSample::BongoMuted),
        Self::Recorded(RecordedSample::AgogoLow),
        Self::Recorded(RecordedSample::RoomClap),
        Self::Recorded(RecordedSample::ShakerGroove),
        Self::Recorded(RecordedSample::ChordCm7),
    ];

    pub fn dj_effects() -> [Self; SynthSound::ALL.len()] {
        SynthSound::ALL.map(Self::Synth)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Synth(sound) => sound.name(),
            Self::Recorded(sound) => sound.recording().name,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Synth(sound) => sound.key(),
            Self::Recorded(sound) => sound.recording().key,
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::HOUSE_SOUNDS
            .into_iter()
            .find(|sound| sound.key() == key)
            .or_else(|| SynthSound::from_key(key).map(Self::Synth))
    }

    pub fn settings(self) -> SampleSettings {
        match self {
            Self::Synth(sound) => sound.settings(),
            Self::Recorded(sound) => {
                if let Some(bpm) = sound.recording().loop_bpm {
                    SampleSettings {
                        mode: SampleMode::Loop,
                        sync: true,
                        bpm: Some(bpm),
                        ..SampleSettings::default()
                    }
                } else {
                    SampleSettings::default()
                }
            }
        }
    }

    pub fn track(self) -> Result<Track, EngineError> {
        match self {
            Self::Synth(sound) => sound.track(48_000),
            Self::Recorded(sound) => {
                let recording = sound.recording();
                decode_bytes_to_end(recording.bytes, recording.file)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dubplate_engine::DecodeStatus;

    #[test]
    fn the_embedded_house_recordings_decode_completely_offline() {
        for sound in BuiltInSample::HOUSE_SOUNDS {
            let track = sound
                .track()
                .unwrap_or_else(|error| panic!("{}: {error}", sound.key()));
            assert_eq!(track.status(), DecodeStatus::Complete, "{}", sound.key());
            assert_eq!(track.sample_rate(), 48_000, "{}", sound.key());
            let frames = track.decoded_frames();
            let peak = (0..frames)
                .flat_map(|frame| track.frame(frame).unwrap())
                .map(f32::abs)
                .fold(0.0, f32::max);
            assert!((0.5..0.85).contains(&peak), "{}: peak {peak}", sound.key());

            let settings = sound.settings();
            if sound == BuiltInSample::Recorded(RecordedSample::ShakerGroove) {
                assert_eq!(settings.mode, SampleMode::Loop);
                assert!(settings.sync);
                assert_eq!(settings.bpm, Some(120.0));
                assert_eq!(frames, 96_000, "four beats at 120 BPM");
            } else {
                assert_eq!(settings, SampleSettings::default(), "{}", sound.key());
                assert!(
                    (1..48_000).contains(&frames),
                    "{}: {frames} frames",
                    sound.key()
                );
            }
        }
    }
}
