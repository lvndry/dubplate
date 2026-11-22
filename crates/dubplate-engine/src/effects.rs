use std::fmt;

use dubplate_fx::{Effect, EffectConfig, EffectKind, ParameterValue, create};

use crate::deck::DeckId;
use crate::error::EngineError;
use crate::processor::{ProcessContext, StereoBlock};
use crate::{DECK_COUNT, EFFECT_UNIT_COUNT};

const KIND_COUNT: usize = EffectKind::ALL.len();
const TARGET_COUNT: usize = EffectTarget::ALL.len();
const DEFAULT_KINDS: [EffectKind; EFFECT_UNIT_COUNT] =
    [EffectKind::Echo, EffectKind::Reverb, EffectKind::Flanger];
const DEFAULT_DEPTH: f32 = 0.5;

/// One of the effect units, like the beat effects section of a DJ mixer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EffectUnitId(u8);

impl EffectUnitId {
    /// Every effect unit, in order.
    pub const ALL: [Self; EFFECT_UNIT_COUNT] = [Self(0), Self(1), Self(2)];

    /// The unit at `index`, counted from 0.
    pub fn new(index: usize) -> Result<Self, EngineError> {
        Self::ALL
            .get(index)
            .copied()
            .ok_or(EngineError::InvalidEffectUnit {
                unit: index,
                unit_count: EFFECT_UNIT_COUNT,
            })
    }

    /// Position of the unit in [`EffectUnitId::ALL`].
    pub fn index(self) -> usize {
        usize::from(self.0)
    }
}

impl fmt::Display for EffectUnitId {
    /// The unit's number as printed on a mixer, counted from 1.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "FX {}", self.index() + 1)
    }
}

/// Where an effect unit sits: on one deck's channel or on the master mix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EffectTarget {
    /// A deck's channel, after its channel fader and the crossfader, so echo and reverb trails
    /// keep ringing when the fader closes.
    Deck(DeckId),
    /// The master mix, after the master inserts and before the master gain.
    Master,
}

impl EffectTarget {
    /// Every target, in the order of a mixer's channel select switch.
    pub const ALL: [Self; DECK_COUNT + 1] =
        [Self::Deck(DeckId::A), Self::Deck(DeckId::B), Self::Master];

    /// Position of the target in [`EffectTarget::ALL`].
    pub fn index(self) -> usize {
        match self {
            Self::Deck(deck) => deck.index(),
            Self::Master => DECK_COUNT,
        }
    }
}

/// What an effect unit is set to, as the control side last asked for it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EffectUnitState {
    /// The selected effect.
    pub kind: EffectKind,
    /// Where the unit sits.
    pub target: EffectTarget,
    /// The selected effect's parameter, as its BEAT buttons step through it: the echo time,
    /// sweep period or gate step in beats, a reverb's size, a filter's resonance. `None` for an
    /// effect without one.
    pub parameter: Option<ParameterValue>,
    /// The level and depth knob, from 0 to 1.
    pub depth: f32,
    /// Whether the effect is switched on.
    pub enabled: bool,
    /// Whether the unit still changes the sound: switched on, or ringing out a trail after it
    /// was switched off, moved or changed to another effect.
    pub sounding: bool,
}

/// The settings of an effect unit, without what only the audio thread knows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct EffectSettings {
    kind: EffectKind,
    target: EffectTarget,
    parameters: [Option<ParameterValue>; KIND_COUNT],
    depth: f32,
    enabled: bool,
}

impl EffectSettings {
    pub(crate) fn initial(unit: EffectUnitId) -> Self {
        Self {
            kind: DEFAULT_KINDS[unit.index()],
            target: EffectTarget::Master,
            parameters: EffectKind::ALL.map(|kind| kind.parameter().map(|spec| spec.default)),
            depth: DEFAULT_DEPTH,
            enabled: false,
        }
    }

    pub(crate) fn is_enabled(self) -> bool {
        self.enabled
    }

    pub(crate) fn apply(&mut self, change: EffectChange) {
        match change {
            EffectChange::Kind(kind) => self.kind = kind,
            EffectChange::Target(target) => self.target = target,
            EffectChange::Parameter(value) => {
                if self
                    .kind
                    .parameter()
                    .is_some_and(|spec| spec.accepts(value))
                {
                    self.parameters[self.kind.index()] = Some(value);
                }
            }
            EffectChange::Depth(depth) => self.depth = depth,
            EffectChange::Enabled(enabled) => self.enabled = enabled,
        }
    }

    pub(crate) fn with_sounding(self, sounding: bool) -> EffectUnitState {
        EffectUnitState {
            kind: self.kind,
            target: self.target,
            parameter: self.parameters[self.kind.index()],
            depth: self.depth,
            enabled: self.enabled,
            sounding,
        }
    }
}

/// A change to one setting of an effect unit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum EffectChange {
    Kind(EffectKind),
    Target(EffectTarget),
    Parameter(ParameterValue),
    Depth(f32),
    Enabled(bool),
}

/// Every effect of one unit for one target, built up front so switching never allocates.
type Bank = [Box<dyn Effect>; KIND_COUNT];

/// The audio-thread side of one effect unit.
///
/// It holds a bank of every effect for every target. Moving the unit or choosing another
/// effect switches the old instance off and the new one on: the old one fades out or rings out
/// its trail on its own target while the new one fades in, so nothing clicks.
struct EffectUnit {
    settings: EffectSettings,
    banks: [Bank; TARGET_COUNT],
}

impl EffectUnit {
    fn new(settings: EffectSettings, config: &EffectConfig) -> Self {
        let mut unit = Self {
            settings,
            banks: std::array::from_fn(|_| {
                std::array::from_fn(|index| create(EffectKind::ALL[index], config))
            }),
        };
        unit.for_each_effect(|effect| {
            effect.set_depth(settings.depth);
            if let Some(value) = settings.parameters[effect.kind().index()] {
                effect.set_parameter(value);
            }
        });
        unit
    }

    fn for_each_effect(&mut self, mut action: impl FnMut(&mut dyn Effect)) {
        for effect in self.banks.iter_mut().flatten() {
            action(effect.as_mut());
        }
    }

    fn set_selected_enabled(&mut self, kind: EffectKind, target: EffectTarget, enabled: bool) {
        for effect in &mut self.banks[target.index()] {
            if effect.kind() == kind {
                effect.set_enabled(enabled);
            }
        }
    }

    fn change(&mut self, change: EffectChange) {
        let previous = self.settings;
        self.settings.apply(change);
        match change {
            EffectChange::Parameter(_) => {
                let kind = self.settings.kind;
                if let Some(value) = self.settings.parameters[kind.index()] {
                    self.for_each_effect(|effect| {
                        if effect.kind() == kind {
                            effect.set_parameter(value);
                        }
                    });
                }
            }
            EffectChange::Depth(depth) => self.for_each_effect(|effect| effect.set_depth(depth)),
            EffectChange::Kind(_) | EffectChange::Target(_) | EffectChange::Enabled(_) => {
                let moved =
                    (previous.kind, previous.target) != (self.settings.kind, self.settings.target);
                if moved {
                    self.set_selected_enabled(previous.kind, previous.target, false);
                }
                self.set_selected_enabled(
                    self.settings.kind,
                    self.settings.target,
                    self.settings.enabled,
                );
            }
        }
    }

    /// Runs the selected effect, when the unit sits on `target`, and every effect still fading
    /// out or ringing out there. An effect that is off, faded out and not ringing is skipped
    /// unless it needs to hear the music while off; brake and backspin do, so they can stop the
    /// turntable the moment they are switched on.
    fn process(
        &mut self,
        target: EffectTarget,
        left: &mut [f32],
        right: &mut [f32],
        context: &dubplate_fx::ProcessContext,
    ) {
        let selected = (self.settings.target == target).then_some(self.settings.kind);
        for effect in &mut self.banks[target.index()] {
            let selected = selected == Some(effect.kind());
            if !selected && !effect.is_active() {
                continue;
            }
            if !effect.is_enabled() && !effect.is_active() && !effect.needs_input_when_off() {
                continue;
            }
            effect.process(left, right, context);
        }
    }

    fn is_sounding(&self) -> bool {
        self.banks.iter().flatten().any(|effect| effect.is_active())
    }
}

/// The limits every effect the engine builds allocates for: the engine's own sample rate.
pub(crate) fn effect_config(sample_rate: u32) -> EffectConfig {
    EffectConfig {
        max_sample_rate: sample_rate as f32,
        ..EffectConfig::default()
    }
}

/// The tempo and beat position effects follow for a block, from the engine's context. Without
/// a beat grid the effects fall back to their own tempo.
pub(crate) fn effect_context(context: &ProcessContext) -> dubplate_fx::ProcessContext {
    let (tempo_bpm, beat_position) = context
        .beat_clock
        .map_or((0.0, 0.0), |clock| (clock.bpm, clock.beat_position));
    dubplate_fx::ProcessContext::new(context.sample_rate as f32, tempo_bpm, beat_position)
}

/// The audio-thread side of every effect unit.
pub(crate) struct EffectRack {
    units: [EffectUnit; EFFECT_UNIT_COUNT],
}

impl EffectRack {
    /// Builds every effect of every unit for `sample_rate`. This allocates, so it runs before
    /// the renderer reaches the audio thread.
    pub(crate) fn new(sample_rate: u32) -> Self {
        let config = effect_config(sample_rate);
        Self {
            units: EffectUnitId::ALL
                .map(|unit| EffectUnit::new(EffectSettings::initial(unit), &config)),
        }
    }

    pub(crate) fn change(&mut self, unit: EffectUnitId, change: EffectChange) {
        self.units[unit.index()].change(change);
    }

    /// Runs every unit on `target` over the block, in unit order.
    pub(crate) fn process(
        &mut self,
        target: EffectTarget,
        block: &mut StereoBlock,
        context: &ProcessContext,
    ) {
        let effect_context = effect_context(context);
        let (left, right) = block.channels_mut();
        for unit in &mut self.units {
            unit.process(target, left, right, &effect_context);
        }
    }

    pub(crate) fn sounding(&self) -> [bool; EFFECT_UNIT_COUNT] {
        std::array::from_fn(|index| self.units[index].is_sounding())
    }
}
