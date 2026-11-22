use std::fmt;

use dubplate_fx::{BeatLength, Effect, EffectKind, ParameterValue, create};

use crate::effects::{effect_config, effect_context};
use crate::processor::{ProcessContext, StereoBlock};

/// How far the filter knob must leave its centre before a preset's effect comes in, the same
/// dead zone as the filter's own.
const DEAD_ZONE: f32 = 0.02;
/// How quickly the effect comes in along the knob's travel from the centre: the amount is the
/// travel raised to this power, so a quarter of the way is already 0.43 of the effect.
const ENTRY_CURVE: f32 = 0.6;
/// The crusher only bites deep into its range, so its amount rises faster: 0.66 a quarter of
/// the way.
const CRUSH_ENTRY_CURVE: f32 = 0.3;
/// The noise is quiet next to the music until it is well in, so it rises faster still: 0.76
/// a quarter of the way.
const NOISE_ENTRY_CURVE: f32 = 0.2;
/// How far the Pitch preset bends the music at the end of the knob: an octave down to the
/// left, an octave up to the right.
const PITCH_RANGE_SEMITONES: i8 = 12;
/// The Sweep's band-pass to the right: its depth at the end of the knob, which centres it on
/// the mids, about 900 Hz, and its resonance.
const SWEEP_BAND_DEPTH: f32 = 0.5;
const SWEEP_RESONANCE_PERCENT: i16 = 70;
/// The size of the Space preset's room.
const SPACE_SIZE_PERCENT: i16 = 60;

/// A Smart CFX preset: the channel's filter knob drives the filter and a second effect at
/// once. The effect comes in quickly as the knob leaves the centre, clearly heard a quarter of
/// the way, and grows to its fullest at the end of the knob. At the centre the music passes
/// untouched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SmartCfx {
    /// A reverb that grows into a wash, darkened by the low-pass or thinned by the high-pass.
    Space,
    /// A 3/4-beat echo feeding the filter, the classic dub echo.
    DubEcho,
    /// A DJ mixer's Sweep colour effect, without the filter: to the left a gate that cuts the
    /// music tighter the further it turns, to the right a band-pass that narrows on the mids.
    Sweep,
    /// White noise laid over the filtered music, louder and brighter the further the knob
    /// turns either way: a DJ mixer's Noise colour effect, for build-ups.
    Noise,
    /// A bit crusher into the filter, as a DJ mixer's Crush colour effect crushes then filters.
    Crush,
    /// A 1/8-beat gate after the filter that chops the music in time.
    Chop,
    /// A slow 4-beat flanger sweep into the filter.
    Jet,
    /// A 2-beat phaser into the filter.
    Phase,
    /// The pitch bent down an octave to the left and up an octave to the right, into the
    /// filter, like a DJ mixer's Pitch colour effect.
    Pitch,
}

impl SmartCfx {
    /// Every preset, in the order the preset button steps through them.
    pub const ALL: [SmartCfx; 9] = [
        SmartCfx::Space,
        SmartCfx::DubEcho,
        SmartCfx::Sweep,
        SmartCfx::Noise,
        SmartCfx::Crush,
        SmartCfx::Chop,
        SmartCfx::Jet,
        SmartCfx::Phase,
        SmartCfx::Pitch,
    ];

    /// The next preset, wrapping round to the first.
    pub fn next(self) -> SmartCfx {
        let index = self.index();
        Self::ALL[(index + 1) % Self::ALL.len()]
    }

    fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|preset| *preset == self)
            .unwrap_or(0)
    }

    /// What the preset plays at the end of the knob, turned to the right when `rising`.
    fn recipe(self, rising: bool) -> Recipe {
        let recipe = |kind, parameter, depth, mix| Recipe {
            kind,
            parameter,
            depth,
            mix,
            entry: ENTRY_CURVE,
        };
        let beats = |length| Some(ParameterValue::Beats(length));
        match self {
            SmartCfx::Space => recipe(
                EffectKind::Reverb,
                Some(ParameterValue::Percent(SPACE_SIZE_PERCENT)),
                1.0,
                PresetMix::OnTop,
            ),
            SmartCfx::DubEcho => recipe(
                EffectKind::Echo,
                beats(BeatLength::THREE_QUARTERS),
                0.8,
                PresetMix::OnTop,
            ),
            SmartCfx::Sweep if rising => recipe(
                EffectKind::BandPass,
                Some(ParameterValue::Percent(SWEEP_RESONANCE_PERCENT)),
                SWEEP_BAND_DEPTH,
                PresetMix::FullyWet,
            ),
            SmartCfx::Sweep => recipe(
                EffectKind::Trans,
                beats(BeatLength::SIXTEENTH),
                1.0,
                PresetMix::FullyWet,
            ),
            SmartCfx::Noise => Recipe {
                entry: NOISE_ENTRY_CURVE,
                ..recipe(
                    EffectKind::Noise,
                    beats(BeatLength::QUARTER),
                    1.0,
                    PresetMix::FullyWet,
                )
            },
            SmartCfx::Crush => Recipe {
                entry: CRUSH_ENTRY_CURVE,
                ..recipe(EffectKind::Crusher, None, 1.0, PresetMix::FullyWet)
            },
            SmartCfx::Chop => recipe(
                EffectKind::Trans,
                beats(BeatLength::EIGHTH),
                1.0,
                PresetMix::FullyWet,
            ),
            SmartCfx::Jet => recipe(
                EffectKind::Flanger,
                beats(BeatLength::FOUR),
                1.0,
                PresetMix::FullyWet,
            ),
            SmartCfx::Phase => recipe(
                EffectKind::Phaser,
                beats(BeatLength::TWO),
                1.0,
                PresetMix::FullyWet,
            ),
            SmartCfx::Pitch => recipe(
                EffectKind::Pitch,
                Some(ParameterValue::Semitones(PITCH_RANGE_SEMITONES)),
                1.0,
                PresetMix::FullyWet,
            ),
        }
    }

    /// Whether the effect comes before the filter, so the filter shapes its tails.
    fn before_filter(self) -> bool {
        !matches!(self, SmartCfx::Chop | SmartCfx::Noise)
    }

    /// Whether the preset plays a different effect to the right of the knob than to the left.
    fn splits(self) -> bool {
        self == SmartCfx::Sweep
    }

    /// Whether the knob still turns the channel's filter along with the preset.
    pub(crate) fn filters(self) -> bool {
        self != SmartCfx::Sweep
    }
}

/// What a preset plays at the end of the knob: the effect, its parameter, its depth and its
/// mix, and how quickly it comes in. The knob scales the depth and the mix together, so a
/// preset sounds the same whatever the effect's own depth knob does to its mix.
#[derive(Debug, Clone, Copy)]
struct Recipe {
    kind: EffectKind,
    parameter: Option<ParameterValue>,
    depth: f32,
    mix: PresetMix,
    entry: f32,
}

/// Where a preset's mix ends at the end of the knob.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PresetMix {
    /// An echo or a reverb at full level on top of the music.
    OnTop,
    /// An effect that reshapes the music, such as a gate, on its own.
    FullyWet,
}

impl PresetMix {
    /// The effect's mix for an `amount` of the knob. An echo or a reverb rises along an audio
    /// taper, the square of its mix, so it takes the square root of the amount and comes in as
    /// quickly as the presets that reshape the music.
    fn at(self, amount: f32) -> f32 {
        match self {
            Self::OnTop => 0.5 * amount.sqrt(),
            Self::FullyWet => amount,
        }
    }
}

impl fmt::Display for SmartCfx {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            SmartCfx::Space => "space",
            SmartCfx::DubEcho => "dub echo",
            SmartCfx::Sweep => "sweep",
            SmartCfx::Noise => "noise",
            SmartCfx::Crush => "crush",
            SmartCfx::Chop => "chop",
            SmartCfx::Jet => "jet",
            SmartCfx::Phase => "phase",
            SmartCfx::Pitch => "pitch",
        })
    }
}

/// The audio-thread side of a channel's Smart CFX: one effect per preset, built up front.
pub(crate) struct SmartCfxRack {
    effects: [Box<dyn Effect>; SmartCfx::ALL.len()],
    rising_effects: [Option<Box<dyn Effect>>; SmartCfx::ALL.len()],
    selected: Option<SmartCfx>,
    travel: f32,
    lowering: bool,
}

impl SmartCfxRack {
    /// Builds every preset's effect for `sample_rate`, and a second one for the presets that
    /// play another to the right. This allocates, so it runs before the renderer reaches the
    /// audio thread.
    pub(crate) fn new(sample_rate: u32) -> Self {
        let config = effect_config(sample_rate);
        let build = |recipe: Recipe| {
            let mut effect = create(recipe.kind, &config);
            if let Some(value) = recipe.parameter {
                effect.set_parameter(value);
            }
            effect
        };
        Self {
            effects: SmartCfx::ALL.map(|preset| build(preset.recipe(false))),
            rising_effects: SmartCfx::ALL
                .map(|preset| preset.splits().then(|| build(preset.recipe(true)))),
            selected: None,
            travel: 0.0,
            lowering: false,
        }
    }

    /// Chooses the preset, or plain filter with `None`. The previous preset's effect rings out.
    pub(crate) fn select(&mut self, preset: Option<SmartCfx>) {
        self.selected = preset;
        self.update();
    }

    /// Whether the channel's filter follows the knob: with no preset, or one that keeps it.
    pub(crate) fn filters(&self) -> bool {
        self.selected.is_none_or(SmartCfx::filters)
    }

    /// Follows the filter knob, from 0 to 1 with its centre at 0.5.
    pub(crate) fn set_knob(&mut self, position: f32) {
        self.travel = ((position - 0.5).abs() * 2.0).clamp(0.0, 1.0);
        self.lowering = position < 0.5;
        self.update();
    }

    fn update(&mut self) {
        let travel = ((self.travel - DEAD_ZONE) / (1.0 - DEAD_ZONE)).max(0.0);
        let lowering = self.lowering;
        for ((preset, effect), rising_effect) in SmartCfx::ALL
            .into_iter()
            .zip(&mut self.effects)
            .zip(&mut self.rising_effects)
        {
            let engaged = self.selected == Some(preset) && travel > 0.0;
            let sides = std::iter::once((effect, false))
                .chain(rising_effect.as_mut().map(|rising| (rising, true)));
            for (effect, rising) in sides {
                let playing = engaged && (!preset.splits() || rising != lowering);
                if playing {
                    let recipe = preset.recipe(rising);
                    let amount = travel.powf(recipe.entry);
                    effect.set_depth(amount * recipe.depth);
                    effect.set_mix(recipe.mix.at(amount));
                    if preset == SmartCfx::Pitch {
                        let range = if lowering {
                            -PITCH_RANGE_SEMITONES
                        } else {
                            PITCH_RANGE_SEMITONES
                        };
                        effect.set_parameter(ParameterValue::Semitones(range));
                    }
                }
                effect.set_enabled(playing);
            }
        }
    }

    /// Runs the effects that sit before (`before_filter`) or after the filter: the selected
    /// one and any still ringing out.
    pub(crate) fn process(
        &mut self,
        block: &mut StereoBlock,
        context: &ProcessContext,
        before_filter: bool,
    ) {
        let effect_context = effect_context(context);
        let (left, right) = block.channels_mut();
        for ((preset, effect), rising_effect) in SmartCfx::ALL
            .into_iter()
            .zip(&mut self.effects)
            .zip(&mut self.rising_effects)
        {
            if preset.before_filter() != before_filter {
                continue;
            }
            for effect in std::iter::once(effect).chain(rising_effect.as_mut()) {
                if effect.is_enabled() || effect.is_active() {
                    effect.process(left, right, &effect_context);
                }
            }
        }
    }
}
