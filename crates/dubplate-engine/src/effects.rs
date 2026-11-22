use std::fmt;

use dubplate_fx::{BeatLength, Effect, EffectConfig, EffectKind, ParameterValue, create};

use crate::deck::DeckId;
use crate::error::EngineError;
use crate::processor::{ProcessContext, StereoBlock};
use crate::{DECK_COUNT, EFFECT_UNIT_COUNT};

const KIND_COUNT: usize = EffectKind::ALL.len();
const BUS_COUNT: usize = DECK_COUNT + 1;
const DEFAULT_KINDS: [EffectKind; EFFECT_UNIT_COUNT] =
    [EffectKind::Echo, EffectKind::Reverb, EffectKind::Flanger];
const DEFAULT_DEPTH: f32 = 0.5;
const RELEASE_COUNT: usize = ReleaseFx::ALL.len();
/// The depth every Release FX plays at: the stops go all the way and the echo rings for a while.
const RELEASE_DEPTH: f32 = 0.5;

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

/// Where an effect unit sits: on one deck's channel, on both decks' channels, or on the master
/// mix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EffectTarget {
    /// A deck's channel, after its channel fader and the crossfader, so echo and reverb trails
    /// keep ringing when the fader closes.
    Deck(DeckId),
    /// Both decks' channels, each with its own instance of the effect following its own beat
    /// grid: the 1&2 position of a controller's CH SELECT lever. The sampler is left dry.
    BothDecks,
    /// The master mix, after the master inserts and before the master gain.
    Master,
}

impl EffectTarget {
    /// Every target, in the order of the strip's target buttons.
    pub const ALL: [Self; 4] = [
        Self::Deck(DeckId::A),
        Self::Deck(DeckId::B),
        Self::BothDecks,
        Self::Master,
    ];

    /// Position of the target in [`EffectTarget::ALL`].
    pub fn index(self) -> usize {
        match self {
            Self::Deck(deck) => deck.index(),
            Self::BothDecks => DECK_COUNT,
            Self::Master => DECK_COUNT + 1,
        }
    }

    /// Whether a unit on this target processes `bus`.
    pub(crate) fn covers(self, bus: Bus) -> bool {
        match (self, bus) {
            (Self::Deck(target), Bus::Deck(deck)) => target == deck,
            (Self::BothDecks, Bus::Deck(_)) | (Self::Master, Bus::Master) => true,
            (Self::Deck(_) | Self::BothDecks, Bus::Master) | (Self::Master, Bus::Deck(_)) => false,
        }
    }
}

/// A point in the mix an effect runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Bus {
    /// A deck's channel, after its fader.
    Deck(DeckId),
    /// The master mix.
    Master,
}

impl Bus {
    const ALL: [Self; BUS_COUNT] = [Self::Deck(DeckId::A), Self::Deck(DeckId::B), Self::Master];

    fn index(self) -> usize {
        match self {
            Self::Deck(deck) => deck.index(),
            Self::Master => DECK_COUNT,
        }
    }
}

/// A Release FX: what an effect unit plays while SHIFT + BEAT FX ON is held, cutting its beat
/// effect, as on rekordbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReleaseFx {
    /// The last beat echoed while the music cuts out.
    Echo,
    /// The record spun backwards to a stop.
    BackSpin,
    /// The record slowed to a stop.
    VinylBrake,
}

impl ReleaseFx {
    /// Every Release FX, in the order the strip's button steps through them.
    pub const ALL: [Self; 3] = [Self::Echo, Self::BackSpin, Self::VinylBrake];

    /// The effect that plays it.
    fn kind(self) -> EffectKind {
        match self {
            Self::Echo => EffectKind::EchoOut,
            Self::BackSpin => EffectKind::Backspin,
            Self::VinylBrake => EffectKind::Brake,
        }
    }

    /// How long it takes, in beats.
    fn beat(self) -> BeatLength {
        match self {
            Self::Echo | Self::VinylBrake => BeatLength::ONE,
            Self::BackSpin => BeatLength::TWO,
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Echo => 0,
            Self::BackSpin => 1,
            Self::VinylBrake => 2,
        }
    }

    /// The next Release FX, wrapping round.
    pub fn next(self) -> Self {
        Self::ALL[(self.index() + 1) % Self::ALL.len()]
    }
}

impl fmt::Display for ReleaseFx {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Echo => "echo",
            Self::BackSpin => "back spin",
            Self::VinylBrake => "vinyl brake",
        })
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
    /// The Release FX SHIFT + BEAT FX ON plays.
    pub release: ReleaseFx,
    /// Whether the Release FX is held.
    pub releasing: bool,
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
    release: ReleaseFx,
    releasing: bool,
}

impl EffectSettings {
    pub(crate) fn initial(unit: EffectUnitId) -> Self {
        Self {
            kind: DEFAULT_KINDS[unit.index()],
            target: EffectTarget::Master,
            parameters: EffectKind::ALL.map(|kind| kind.parameter().map(|spec| spec.default)),
            depth: DEFAULT_DEPTH,
            enabled: false,
            release: ReleaseFx::Echo,
            releasing: false,
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
            EffectChange::ChooseRelease(release) => {
                if !self.releasing {
                    self.release = release;
                }
            }
            EffectChange::Release(held) => {
                self.releasing = held;
                if held {
                    self.enabled = false;
                }
            }
        }
    }

    pub(crate) fn with_sounding(self, sounding: bool) -> EffectUnitState {
        EffectUnitState {
            kind: self.kind,
            target: self.target,
            parameter: self.parameters[self.kind.index()],
            depth: self.depth,
            enabled: self.enabled,
            release: self.release,
            releasing: self.releasing,
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
    ChooseRelease(ReleaseFx),
    Release(bool),
}

/// Every effect of one unit for one bus, built up front so switching never allocates.
type Bank = [Box<dyn Effect>; KIND_COUNT];

/// Every Release FX of one unit for one bus, built up front, apart from the unit's beat
/// effects so a release always plays at full strength whatever the unit's depth.
type Releases = [Box<dyn Effect>; RELEASE_COUNT];

/// The audio-thread side of one effect unit.
///
/// It holds a bank of every effect for every bus. Moving the unit or choosing another
/// effect switches the old instance off and the new one on: the old one fades out or rings out
/// its trail on its own target while the new one fades in, so nothing clicks.
struct EffectUnit {
    settings: EffectSettings,
    banks: [Bank; BUS_COUNT],
    releases: [Releases; BUS_COUNT],
}

impl EffectUnit {
    fn new(settings: EffectSettings, config: &EffectConfig) -> Self {
        let mut unit = Self {
            settings,
            banks: std::array::from_fn(|_| {
                std::array::from_fn(|index| create(EffectKind::ALL[index], config))
            }),
            releases: std::array::from_fn(|_| {
                ReleaseFx::ALL.map(|release| {
                    let mut effect = create(release.kind(), config);
                    effect.set_depth(RELEASE_DEPTH);
                    effect.set_beat(release.beat());
                    effect
                })
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

    fn set_enabled_on(&mut self, bus: Bus, kind: EffectKind, enabled: bool) {
        for effect in &mut self.banks[bus.index()] {
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
            EffectChange::ChooseRelease(_) => {}
            EffectChange::Release(held) => self.release(previous, held),
            EffectChange::Kind(_) | EffectChange::Target(_) | EffectChange::Enabled(_) => {
                for bus in Bus::ALL {
                    let before = previous.target.covers(bus).then_some(previous.kind);
                    let after = self
                        .settings
                        .target
                        .covers(bus)
                        .then_some(self.settings.kind);
                    if let Some(kind) = before.filter(|kind| after != Some(*kind)) {
                        self.set_enabled_on(bus, kind, false);
                    }
                    if let Some(kind) = after {
                        self.set_enabled_on(bus, kind, self.settings.enabled);
                    }
                }
            }
        }
    }

    /// Holds or lets go of the Release FX: holding it cuts the beat effect, which fades out or
    /// rings out, and plays the release on every bus the unit covers; letting go stops the
    /// release, which brings the live music back or lets the echo ring out, and leaves the beat
    /// effect off, as on rekordbox.
    fn release(&mut self, previous: EffectSettings, held: bool) {
        let kind = self.settings.kind;
        let release = self.settings.release.index();
        for bus in Bus::ALL {
            let covered = self.settings.target.covers(bus);
            if held && covered && previous.enabled {
                self.set_enabled_on(bus, kind, false);
            }
            self.releases[bus.index()][release].set_enabled(held && covered);
        }
    }

    /// Runs the selected effect, when the unit covers `bus`, and every effect still fading
    /// out or ringing out there. An effect that is off, faded out and not ringing is skipped
    /// unless it needs to hear the music while off; brake and backspin do, so they can stop the
    /// turntable the moment they are switched on.
    fn process(
        &mut self,
        bus: Bus,
        left: &mut [f32],
        right: &mut [f32],
        context: &dubplate_fx::ProcessContext,
    ) {
        let selected = self
            .settings
            .target
            .covers(bus)
            .then_some(self.settings.kind);
        for effect in &mut self.banks[bus.index()] {
            let selected = selected == Some(effect.kind());
            if !selected && !effect.is_active() {
                continue;
            }
            if !effect.is_enabled() && !effect.is_active() && !effect.needs_input_when_off() {
                continue;
            }
            effect.process(left, right, context);
        }
        let chosen = self.settings.release.index();
        let covered = self.settings.target.covers(bus);
        for (index, effect) in self.releases[bus.index()].iter_mut().enumerate() {
            let listening = covered && index == chosen && effect.needs_input_when_off();
            if effect.is_enabled() || effect.is_active() || listening {
                effect.process(left, right, context);
            }
        }
    }

    fn is_sounding(&self) -> bool {
        self.banks
            .iter()
            .flatten()
            .chain(self.releases.iter().flatten())
            .any(|effect| effect.is_active())
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

    /// Runs every unit on `bus` over the block, in unit order.
    pub(crate) fn process(&mut self, bus: Bus, block: &mut StereoBlock, context: &ProcessContext) {
        let effect_context = effect_context(context);
        let (left, right) = block.channels_mut();
        for unit in &mut self.units {
            unit.process(bus, left, right, &effect_context);
        }
    }

    pub(crate) fn sounding(&self) -> [bool; EFFECT_UNIT_COUNT] {
        std::array::from_fn(|index| self.units[index].is_sounding())
    }
}
