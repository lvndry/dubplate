//! The changes the control side asks the audio thread to make, grouped by the part of the
//! engine that owns the state they change.

use std::sync::Arc;

use dubplate_fx::{BeatLength, EqBand};

use crate::deck::DeckId;
use crate::effects::{EffectChange, EffectUnitId};
use crate::grid::BeatGrid;
use crate::jog::JogInput;
use crate::pad_effects::PadEffect;
use crate::preview::PreviewPlan;
use crate::processor::AudioProcessor;
use crate::recording::RecordingSink;
use crate::sampler::SampleSettings;
use crate::smart_cfx::SmartCfx;
use crate::sync::SyncMode;
use crate::tempo::{MasterTempo, TempoRange};
use crate::track::TrackBuffer;

/// A change the control side asks the audio thread to make.
pub(crate) enum Command {
    /// A change to one deck.
    Deck {
        deck: DeckId,
        command: DeckCommand,
    },
    Headphones(HeadphoneCommand),
    Sampler(SamplerCommand),
    ChangeEffect {
        unit: EffectUnitId,
        change: EffectChange,
    },
    SetCrossfader(f32),
    SetMasterGain(f32),
    SetMasterInsert {
        slot: usize,
        processor: Option<Box<dyn AudioProcessor>>,
    },
    SetMasterTempo(MasterTempo),
    SetJogTicksPerRevolution(u32),
    SetSyncHalfAndDouble(bool),
    SetQuantize(bool),
    SetSmartFader(bool),
    Preview(Option<PreviewPlan>),
    SetRecording(Option<RecordingSink>),
}

/// A change to one deck: its transport, loops, tempo and mixer channel.
pub(crate) enum DeckCommand {
    Load(Option<Arc<TrackBuffer>>),
    Play,
    Pause,
    Cue,
    SetCuePoint {
        seconds: f64,
    },
    Seek {
        seconds: f64,
    },
    HotCue {
        seconds: f64,
    },
    BeatJump {
        beats: f64,
    },
    LoopIn,
    LoopOut,
    BeatLoop(BeatLength),
    HalveLoop,
    DoubleLoop,
    ReloopOrExit,
    SetSlip(bool),
    BeatRoll(Option<BeatLength>),
    HoldPadEffect(PadEffect),
    ReleasePadEffect,
    SetBeatGrid(Option<BeatGrid>),
    SetTempo(f64),
    SetTempoFader(f64),
    SetTempoRange(TempoRange),
    SetPitchBend(f64),
    SetSync(SyncMode),
    SetKeyLock(bool),
    SetKeyShift {
        semitones: f32,
    },
    Jog(JogInput),
    SetVinylMode(bool),
    SetTrim(f32),
    SetChannelFader(f32),
    SetEq {
        band: EqBand,
        position: f32,
    },
    SetFilter(f32),
    SetFilterResonance(f32),
    SetSmartCfx(Option<SmartCfx>),
    SetInsert {
        slot: usize,
        processor: Option<Box<dyn AudioProcessor>>,
    },
}

/// A change to the headphone section.
pub(crate) enum HeadphoneCommand {
    /// A channel's CUE button.
    Cue { deck: DeckId, enabled: bool },
    /// The MASTER CUE button.
    MasterCue(bool),
    /// The MIXING knob.
    Mix(f32),
    /// The LEVEL knob.
    Level(f32),
}

/// A change to the sampler.
pub(crate) enum SamplerCommand {
    Load {
        slot: usize,
        track: Option<Arc<TrackBuffer>>,
        settings: SampleSettings,
    },
    SetSettings {
        slot: usize,
        settings: SampleSettings,
    },
    Trigger(usize),
    Release(usize),
    Stop(usize),
    SetVolume(f32),
    SetCue(bool),
}
