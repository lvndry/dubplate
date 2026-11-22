//! The control side of the engine: [`Controller`] sends commands to the audio thread and reads
//! back what it publishes. Each submodule holds the commands of one part of a DJ setup.

use std::sync::Arc;

use rtrb::{Consumer, Producer, RingBuffer};

use crate::channel::ChannelSettings;
use crate::command::{Command, DeckCommand};
use crate::deck::DeckId;
use crate::effects::{EffectSettings, EffectUnitId};
use crate::error::EngineError;
use crate::grid::BeatGrid;
use crate::headphones::HeadphoneSettings;
use crate::pad_effects::PadEffect;
use crate::renderer::Renderer;
use crate::retire::{RetireQueue, Retired};
use crate::sampler::{DEFAULT_SAMPLER_VOLUME, SAMPLER_SLOTS, SampleSettings};
use crate::smart_cfx::SmartCfx;
use crate::state::{EngineStats, Meters, SharedState, take_peak};
use crate::track::Track;
use crate::{DECK_COUNT, EFFECT_UNIT_COUNT};

mod effects;
mod jog;
mod mixer;
mod performance;
mod preview;
mod recording;
mod sampler;
mod tempo;
mod transport;

const COMMAND_CAPACITY: usize = 1024;
const RETIRE_CAPACITY: usize = COMMAND_CAPACITY * 4;
/// The fastest a deck or a preview track plays, as a share of its own tempo.
const MAX_TEMPO: f64 = 4.0;
/// The largest linear gain a trim, the master or a sample can be set to.
const MAX_GAIN: f32 = 16.0;

/// Creates an engine whose audio side is driven by hand instead of a sound card.
///
/// Call [`Renderer::render`] to produce audio. [`crate::AudioOutput::start`] does the same with
/// a real device.
pub fn offline_engine(sample_rate: u32) -> (Controller, Renderer) {
    let (command_producer, command_consumer) = RingBuffer::new(COMMAND_CAPACITY);
    let (retire_producer, retire_consumer) = RingBuffer::new(RETIRE_CAPACITY);
    let shared = Arc::new(SharedState::new(sample_rate));
    let controller = Controller {
        commands: command_producer,
        retired: retire_consumer,
        shared: Arc::clone(&shared),
        decks: std::array::from_fn(|_| DeckSettings::default()),
        effect_units: EffectUnitId::ALL.map(EffectSettings::initial),
        quantize: true,
        headphones: HeadphoneSettings::default(),
        samples: std::array::from_fn(|_| None),
        sampler_volume: DEFAULT_SAMPLER_VOLUME,
        sampler_cue: false,
        smart_fader: false,
    };
    let renderer = Renderer::new(
        sample_rate,
        command_consumer,
        RetireQueue::new(retire_producer),
        shared,
    );
    (controller, renderer)
}

/// The control side of the engine, for the UI, a MIDI controller or a test.
///
/// Every method returns at once: commands go to the audio thread through a lock-free queue and
/// take effect at the start of its next callback, and state comes back through atomics.
pub struct Controller {
    commands: Producer<Command>,
    retired: Consumer<Retired>,
    shared: Arc<SharedState>,
    decks: [DeckSettings; DECK_COUNT],
    effect_units: [EffectSettings; EFFECT_UNIT_COUNT],
    quantize: bool,
    headphones: HeadphoneSettings,
    samples: [Option<(Track, SampleSettings)>; SAMPLER_SLOTS],
    sampler_volume: f32,
    sampler_cue: bool,
    smart_fader: bool,
}

/// What the control side last asked of one deck, kept so it reads back at once without a
/// round trip to the audio thread.
#[derive(Default)]
struct DeckSettings {
    track: Option<Track>,
    beat_grid: Option<BeatGrid>,
    channel: ChannelSettings,
    key_lock: bool,
    key_shift: f32,
    pad_effect: Option<PadEffect>,
    smart_cfx: Option<SmartCfx>,
}

impl Controller {
    /// Peak levels since the previous call.
    pub fn take_meters(&self) -> Meters {
        Meters {
            deck_peaks: std::array::from_fn(|index| take_peak(&self.shared.decks[index].peak)),
            channel_peaks: std::array::from_fn(|index| {
                take_peak(&self.shared.decks[index].pre_fader_peak)
            }),
            master_peak: take_peak(&self.shared.master_peak),
            headphone_peak: take_peak(&self.shared.headphone_peak),
            master_limiter_gain: 1.0 - take_peak(&self.shared.master_limiting),
            sampler_peak: take_peak(&self.shared.sampler_peak),
        }
    }

    /// Buffer size, latency and dropout counters.
    pub fn stats(&self) -> EngineStats {
        self.shared.stats.snapshot()
    }

    pub(crate) fn shared(&self) -> &Arc<SharedState> {
        &self.shared
    }

    /// Drops what the audio thread handed back. Every command does this first, so calling it
    /// is only needed to free memory sooner on an idle controller.
    pub fn collect_garbage(&mut self) {
        while let Ok(retired) = self.retired.pop() {
            drop(retired);
        }
    }

    fn send_deck(&mut self, deck: DeckId, command: DeckCommand) -> Result<(), EngineError> {
        self.send(Command::Deck { deck, command })
    }

    fn send(&mut self, command: Command) -> Result<(), EngineError> {
        self.collect_garbage();
        self.commands
            .push(command)
            .map_err(|_| EngineError::CommandQueueFull)
    }
}
