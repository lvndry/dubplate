use std::fmt;

use dubplate_fx::{BeatLength, Effect, EffectKind, ParameterValue, create};

use crate::effects::{effect_config, effect_context};
use crate::processor::{ProcessContext, StereoBlock};

/// How far the filter knob must leave its centre before a preset's effect comes in, the same
/// dead zone as the filter's own.
const DEAD_ZONE: f32 = 0.02;
/// How far the Pitch preset bends the music at the end of the knob: an octave down to the
/// left, an octave up to the right.
const PITCH_RANGE_SEMITONES: i8 = 12;

/// A Smart CFX preset: the channel's filter knob drives the filter and a second effect at
/// once, the effect growing with the square of the knob's travel from the centre so the first
/// moves stay subtle and the last sweep of the knob goes all in. At the centre the music passes
/// untouched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SmartCfx {
    /// A reverb that grows into a wash, darkened by the low-pass or thinned by the high-pass.
    Space,
    /// A 3/4-beat echo feeding the filter, the classic dub echo.
    DubEcho,
    /// A bit crusher after the filter.
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
    pub const ALL: [SmartCfx; 7] = [
        SmartCfx::Space,
        SmartCfx::DubEcho,
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

    /// The effect, its parameter and the most depth the knob gives it.
    fn recipe(self) -> (EffectKind, Option<ParameterValue>, f32) {
        let beats = |length| Some(ParameterValue::Beats(length));
        match self {
            SmartCfx::Space => (EffectKind::Reverb, Some(ParameterValue::Percent(60)), 0.6),
            SmartCfx::DubEcho => (EffectKind::Echo, beats(BeatLength::THREE_QUARTERS), 0.6),
            SmartCfx::Crush => (EffectKind::Crusher, None, 0.7),
            SmartCfx::Chop => (EffectKind::Gate, beats(BeatLength::EIGHTH), 1.0),
            SmartCfx::Jet => (EffectKind::Flanger, beats(BeatLength::FOUR), 1.0),
            SmartCfx::Phase => (EffectKind::Phaser, beats(BeatLength::TWO), 1.0),
            SmartCfx::Pitch => (
                EffectKind::Pitch,
                Some(ParameterValue::Semitones(PITCH_RANGE_SEMITONES)),
                1.0,
            ),
        }
    }

    /// Whether the effect comes before the filter, so the filter shapes its tails.
    fn before_filter(self) -> bool {
        !matches!(self, SmartCfx::Crush | SmartCfx::Chop)
    }
}

impl fmt::Display for SmartCfx {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            SmartCfx::Space => "space",
            SmartCfx::DubEcho => "dub echo",
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
    selected: Option<SmartCfx>,
    travel: f32,
    lowering: bool,
}

impl SmartCfxRack {
    /// Builds every preset's effect for `sample_rate`. This allocates, so it runs before the
    /// renderer reaches the audio thread.
    pub(crate) fn new(sample_rate: u32) -> Self {
        let config = effect_config(sample_rate);
        Self {
            effects: SmartCfx::ALL.map(|preset| {
                let (kind, parameter, _) = preset.recipe();
                let mut effect = create(kind, &config);
                if let Some(value) = parameter {
                    effect.set_parameter(value);
                }
                effect
            }),
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

    /// Follows the filter knob, from 0 to 1 with its centre at 0.5.
    pub(crate) fn set_knob(&mut self, position: f32) {
        self.travel = ((position - 0.5).abs() * 2.0).clamp(0.0, 1.0);
        self.lowering = position < 0.5;
        self.update();
    }

    fn update(&mut self) {
        let amount = ((self.travel - DEAD_ZONE) / (1.0 - DEAD_ZONE)).max(0.0);
        for (preset, effect) in SmartCfx::ALL.into_iter().zip(&mut self.effects) {
            let engaged = self.selected == Some(preset) && amount > 0.0;
            if engaged {
                let (_, _, max_depth) = preset.recipe();
                effect.set_depth(amount * amount * max_depth);
                if preset == SmartCfx::Pitch {
                    let range = if self.lowering {
                        -PITCH_RANGE_SEMITONES
                    } else {
                        PITCH_RANGE_SEMITONES
                    };
                    effect.set_parameter(ParameterValue::Semitones(range));
                }
            }
            effect.set_enabled(engaged);
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
        for (preset, effect) in SmartCfx::ALL.into_iter().zip(&mut self.effects) {
            if preset.before_filter() == before_filter
                && (effect.is_enabled() || effect.is_active())
            {
                effect.process(left, right, &effect_context);
            }
        }
    }
}
