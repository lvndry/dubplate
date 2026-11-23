//! The 32 Pad FX of the FLX4's PAD FX1 and PAD FX2 modes, eight pads each with and without
//! SHIFT, as the DJ sets them in the Pad FX editor: what each pad plays, its beat or parameter,
//! its depth, whether it is held or latched and what letting go does to its trail. Three
//! starting sets are built in, and the DJ's own set is remembered.

use std::fmt;
use std::str::FromStr;

use dubplate_engine::{
    BeatLength, EffectKind, PadEffect, PadRelease, ParameterSpec, ParameterValue,
};
use dubplate_midi::PadMode;
use serde::{Deserialize, Serialize};

use crate::effects::{effect_label, effect_order, effect_pad_label};
use crate::pads::{PAD_COUNT, PadFx};

/// The four layers of eight pads: PAD FX1, PAD FX1 with SHIFT, PAD FX2, PAD FX2 with SHIFT.
pub const LAYER_COUNT: usize = 4;
/// Every Pad FX pad, layer by layer.
pub const SLOT_COUNT: usize = LAYER_COUNT * PAD_COUNT;

/// How much one step of the editor's depth arrows moves the depth.
const DEPTH_STEP: f32 = 0.05;

/// The loop roll lengths a pad can take, shortest first, as rekordbox's rolls go.
const ROLL_SPEC: ParameterSpec = ParameterSpec {
    name: "Beat",
    steps: &[
        ParameterValue::Beats(BeatLength::THIRTY_SECOND),
        ParameterValue::Beats(BeatLength::SIXTEENTH),
        ParameterValue::Beats(BeatLength::EIGHTH),
        ParameterValue::Beats(BeatLength::QUARTER),
        ParameterValue::Beats(BeatLength::HALF),
        ParameterValue::Beats(BeatLength::ONE),
        ParameterValue::Beats(BeatLength::TWO),
        ParameterValue::Beats(BeatLength::FOUR),
    ],
    default: ParameterValue::Beats(BeatLength::QUARTER),
};

/// One layer of eight pads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PadFxLayer {
    /// PAD FX1, pads 1 to 8.
    Fx1,
    /// PAD FX1 with SHIFT held, rekordbox's effects 9 to 16.
    Fx1Shift,
    /// PAD FX2, effects 17 to 24.
    Fx2,
    /// PAD FX2 with SHIFT held, effects 25 to 32.
    Fx2Shift,
}

impl PadFxLayer {
    /// Every layer, in rekordbox's numbering.
    pub const ALL: [Self; LAYER_COUNT] = [Self::Fx1, Self::Fx1Shift, Self::Fx2, Self::Fx2Shift];

    /// The layer the pads play in `mode`, with or without SHIFT, if it is a Pad FX mode.
    pub fn of(mode: PadMode, shift: bool) -> Option<Self> {
        match (mode, shift) {
            (PadMode::PadFx1, false) => Some(Self::Fx1),
            (PadMode::PadFx1, true) => Some(Self::Fx1Shift),
            (PadMode::PadFx2, false) => Some(Self::Fx2),
            (PadMode::PadFx2, true) => Some(Self::Fx2Shift),
            _ => None,
        }
    }

    /// Its place, 0 to 3.
    pub fn index(self) -> usize {
        self as usize
    }

    /// Whether SHIFT plays it.
    pub fn shifted(self) -> bool {
        matches!(self, Self::Fx1Shift | Self::Fx2Shift)
    }

    /// The numbers rekordbox gives its pads, such as `9-16`.
    pub fn numbers(self) -> String {
        let first = self.index() * PAD_COUNT + 1;
        format!("{first}\u{2013}{}", first + PAD_COUNT - 1)
    }

    /// Its name on screen, such as `FX1 + SHIFT`.
    pub fn label(self) -> &'static str {
        match self {
            Self::Fx1 => "FX1",
            Self::Fx1Shift => "FX1 + SHIFT",
            Self::Fx2 => "FX2",
            Self::Fx2Shift => "FX2 + SHIFT",
        }
    }
}

/// What a pad plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PadSound {
    /// The deck's loop roll: the beat just passed repeats and the deck lands where the track
    /// would have been.
    LoopRoll,
    /// A beat effect on the deck.
    Effect(EffectKind),
}

impl PadSound {
    /// Every sound in the editor's order: the loop roll, then the effects in the picker's order.
    pub fn all() -> impl Iterator<Item = Self> {
        std::iter::once(Self::LoopRoll).chain(effect_order().map(Self::Effect))
    }

    /// The sound `steps` places from this one in the editor's order, wrapping around.
    pub fn step(self, steps: i32) -> Self {
        let sounds: Vec<Self> = Self::all().collect();
        let position = sounds.iter().position(|sound| *sound == self).unwrap_or(0);
        let count = i64::try_from(sounds.len()).unwrap_or(1);
        let moved = (i64::try_from(position).unwrap_or(0) + i64::from(steps)).rem_euclid(count);
        sounds[usize::try_from(moved).unwrap_or(0)]
    }

    /// Its parameter, if it has one.
    pub fn parameter(self) -> Option<&'static ParameterSpec> {
        match self {
            Self::LoopRoll => Some(&ROLL_SPEC),
            Self::Effect(kind) => kind.parameter(),
        }
    }

    /// Its full name, as the editor shows it.
    pub fn label(self) -> &'static str {
        match self {
            Self::LoopRoll => "Loop roll",
            Self::Effect(kind) => effect_label(kind),
        }
    }

    /// Its name on a pad, seven characters at most.
    pub fn pad_label(self) -> &'static str {
        match self {
            Self::LoopRoll => "Roll",
            Self::Effect(kind) => effect_pad_label(kind),
        }
    }

    /// Whether its echoes or tail ring out by default when the pad is let go.
    pub fn rings_out(self) -> bool {
        matches!(
            self,
            Self::Effect(
                EffectKind::Delay
                    | EffectKind::Echo
                    | EffectKind::Spiral
                    | EffectKind::Reverb
                    | EffectKind::RevDelay
                    | EffectKind::MtDelay
                    | EffectKind::UpEcho
                    | EffectKind::DownEcho
                    | EffectKind::LowCutEcho
                    | EffectKind::PingPong
                    | EffectKind::Helix
                    | EffectKind::BpfEcho
                    | EffectKind::SpiralUp
                    | EffectKind::ReverbUp
                    | EffectKind::HpfEcho
                    | EffectKind::LpfEcho
                    | EffectKind::Noise
                    | EffectKind::CrushEcho
                    | EffectKind::SpiralDown
                    | EffectKind::ReverbDown
                    | EffectKind::EchoOut
            )
        )
    }

    /// Whether letting go can change anything: a loop roll always lands back in time.
    pub fn has_release(self) -> bool {
        self != Self::LoopRoll
    }

    /// The name settings files use, such as `loop-roll` or `echo`.
    pub fn name(self) -> &'static str {
        match self {
            Self::LoopRoll => "loop-roll",
            Self::Effect(kind) => kind.name(),
        }
    }
}

impl FromStr for PadSound {
    type Err = ();

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text.trim() == Self::LoopRoll.name() {
            return Ok(Self::LoopRoll);
        }
        text.parse().map(Self::Effect).map_err(|_| ())
    }
}

/// How a pad is played.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PadTrigger {
    /// It plays while held, as rekordbox's Pad FX do.
    #[default]
    Hold,
    /// A press switches it on and the next press switches it off.
    Latch,
}

/// One Pad FX pad as the DJ set it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PadFxSlot {
    /// What it plays.
    pub sound: PadSound,
    /// The roll length or the effect's parameter.
    pub parameter: Option<ParameterValue>,
    /// The effect's depth, from 0 to 1. A loop roll ignores it.
    pub depth: f32,
    /// Held or latched.
    pub trigger: PadTrigger,
    /// What letting go does to the effect's trail.
    pub release: PadRelease,
}

/// Depth for the echoes and reverbs: the trail as loud as the track, on top of it.
const WASH_DEPTH: f32 = 0.5;
/// Depth for the other effects, which take the track over.
const FULL_DEPTH: f32 = 0.85;

impl PadFxSlot {
    /// A held loop roll of `length`.
    pub const fn roll(length: BeatLength) -> Self {
        Self {
            sound: PadSound::LoopRoll,
            parameter: Some(ParameterValue::Beats(length)),
            depth: FULL_DEPTH,
            trigger: PadTrigger::Hold,
            release: PadRelease::Dry,
        }
    }

    /// A held `kind` with `parameter`, at the depth that suits it and its usual release.
    pub fn effect(kind: EffectKind, parameter: Option<ParameterValue>) -> Self {
        let sound = PadSound::Effect(kind);
        let rings_out = sound.rings_out();
        Self {
            sound,
            parameter,
            depth: if rings_out { WASH_DEPTH } else { FULL_DEPTH },
            trigger: PadTrigger::Hold,
            release: if rings_out {
                PadRelease::RingOut
            } else {
                PadRelease::Dry
            },
        }
    }

    /// A held `kind` of `length` beats.
    pub fn beats(kind: EffectKind, length: BeatLength) -> Self {
        Self::effect(kind, Some(ParameterValue::Beats(length)))
    }

    /// A held `kind` at `percent`.
    pub fn percent(kind: EffectKind, percent: i16) -> Self {
        Self::effect(kind, Some(ParameterValue::Percent(percent)))
    }

    /// This pad at `depth`.
    pub fn at_depth(self, depth: f32) -> Self {
        Self { depth, ..self }
    }

    /// What pressing the pad plays.
    pub fn play(self) -> PadFx {
        match self.sound {
            PadSound::LoopRoll => PadFx::Roll(match self.parameter {
                Some(ParameterValue::Beats(length)) => length,
                _ => BeatLength::QUARTER,
            }),
            PadSound::Effect(kind) => PadFx::Effect(PadEffect {
                kind,
                parameter: self.parameter,
                depth: self.depth,
                release: self.release,
            }),
        }
    }

    /// Its name and parameter, as its pad shows them.
    pub fn label(self) -> (&'static str, String) {
        (
            self.sound.pad_label(),
            self.parameter
                .map_or_else(String::new, |value| value.to_string()),
        )
    }

    /// The same pad playing the sound `steps` places on, with that sound's default parameter
    /// and depth and the trigger kept.
    pub fn step_sound(self, steps: i32) -> Self {
        let sound = self.sound.step(steps);
        let fresh = match sound {
            PadSound::LoopRoll => Self::roll(BeatLength::QUARTER),
            PadSound::Effect(kind) => Self::effect(kind, kind.parameter().map(|spec| spec.default)),
        };
        Self {
            trigger: self.trigger,
            ..fresh
        }
    }

    /// The same pad with its parameter `steps` places on, stopping at either end.
    pub fn step_parameter(self, steps: i32) -> Self {
        let Some(spec) = self.sound.parameter() else {
            return self;
        };
        let current = self.parameter.unwrap_or(spec.default);
        Self {
            parameter: Some(spec.step(current, steps)),
            ..self
        }
    }

    /// The same pad with its depth `steps` steps of 5% on, between 5% and 100%.
    pub fn step_depth(self, steps: i32) -> Self {
        let stepped = (self.depth / DEPTH_STEP).round() + steps as f32;
        Self {
            depth: (stepped * DEPTH_STEP).clamp(DEPTH_STEP, 1.0),
            ..self
        }
    }

    /// The same pad held or latched the other way.
    pub fn toggle_trigger(self) -> Self {
        Self {
            trigger: match self.trigger {
                PadTrigger::Hold => PadTrigger::Latch,
                PadTrigger::Latch => PadTrigger::Hold,
            },
            ..self
        }
    }

    /// The same pad with its trail released the other way.
    pub fn toggle_release(self) -> Self {
        Self {
            release: match self.release {
                PadRelease::RingOut => PadRelease::Dry,
                PadRelease::Dry => PadRelease::RingOut,
            },
            ..self
        }
    }
}

/// A built-in set of 32 pads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PadFxPreset {
    /// Rekordbox's kind of default set: rolls from 1/4 to 1/32, echoes and spirals, reverb,
    /// gates, filters, brakes and back spin, pitch and noise.
    #[default]
    Classic,
    /// For afro house: echo outs and long delays to leave a track, filtered echoes, tribal
    /// rolls on the half and quarter beat, and slow sweeps.
    AfroHouse,
    /// For amapiano: rolls on the log drum, dotted echoes, washes of reverb, pitch drops and
    /// brakes into the drop.
    Amapiano,
}

impl PadFxPreset {
    /// Every preset, as the editor lists them.
    pub const ALL: [Self; 3] = [Self::Classic, Self::AfroHouse, Self::Amapiano];

    /// Its name on screen.
    pub fn label(self) -> &'static str {
        match self {
            Self::Classic => "Classic",
            Self::AfroHouse => "Afro house",
            Self::Amapiano => "Amapiano",
        }
    }

    /// Its name in settings files.
    pub fn name(self) -> &'static str {
        match self {
            Self::Classic => "classic",
            Self::AfroHouse => "afro-house",
            Self::Amapiano => "amapiano",
        }
    }

    /// Its 32 pads, layer by layer, pad 1 first.
    pub fn slots(self) -> [PadFxSlot; SLOT_COUNT] {
        use BeatLength as Beat;
        use EffectKind as Kind;
        let semitones = |value| Some(ParameterValue::Semitones(value));
        let signed = |length, falling| {
            Some(ParameterValue::SignedBeats(dubplate_engine::SignedBeats {
                length,
                falling,
            }))
        };
        let roll = PadFxSlot::roll;
        let beats = PadFxSlot::beats;
        let percent = PadFxSlot::percent;
        let effect = PadFxSlot::effect;
        // Noise is generated into a delay. A short tap needs both a short first tap time
        // and a ringing release to let the generated burst reach the output.
        let noise = beats(Kind::Noise, Beat::EIGHTH).at_depth(FULL_DEPTH);
        match self {
            Self::Classic => [
                roll(Beat::QUARTER),
                roll(Beat::EIGHTH),
                roll(Beat::SIXTEENTH),
                roll(Beat::THIRTY_SECOND),
                beats(Kind::Echo, Beat::HALF),
                beats(Kind::Echo, Beat::THREE_QUARTERS),
                beats(Kind::Spiral, Beat::HALF),
                percent(Kind::Reverb, 50),
                beats(Kind::Trans, Beat::QUARTER),
                beats(Kind::Trans, Beat::EIGHTH),
                percent(Kind::LowPass, 50).at_depth(0.75),
                percent(Kind::HighPass, 50).at_depth(0.75),
                beats(Kind::Flanger, Beat::FOUR),
                beats(Kind::Phaser, Beat::TWO),
                beats(Kind::VinylBrake, Beat::ONE).at_depth(1.0),
                beats(Kind::Backspin, Beat::TWO).at_depth(1.0),
                beats(Kind::Delay, Beat::HALF),
                beats(Kind::PingPong, Beat::HALF),
                beats(Kind::UpEcho, Beat::HALF),
                beats(Kind::DownEcho, Beat::HALF),
                beats(Kind::SpiralUp, Beat::HALF),
                beats(Kind::SpiralDown, Beat::HALF),
                percent(Kind::ReverbUp, 80),
                percent(Kind::ReverbDown, 80),
                effect(Kind::Pitch, semitones(12)).at_depth(1.0),
                effect(Kind::Pitch, semitones(-12)).at_depth(1.0),
                noise,
                beats(Kind::CrushEcho, Beat::HALF),
                percent(Kind::Robot, 0),
                effect(Kind::EnigmaJet, signed(Beat::FOUR, false)),
                beats(Kind::Stretch, Beat::ONE),
                beats(Kind::EchoOut, Beat::ONE).at_depth(0.6),
            ],
            Self::AfroHouse => [
                roll(Beat::HALF),
                roll(Beat::QUARTER),
                roll(Beat::EIGHTH),
                roll(Beat::ONE),
                beats(Kind::Echo, Beat::THREE_QUARTERS),
                beats(Kind::Echo, Beat::HALF),
                beats(Kind::Delay, Beat::THREE_QUARTERS),
                beats(Kind::EchoOut, Beat::ONE).at_depth(0.8),
                beats(Kind::LpfEcho, Beat::HALF),
                beats(Kind::HpfEcho, Beat::HALF),
                beats(Kind::Spiral, Beat::THREE_QUARTERS),
                percent(Kind::Reverb, 70),
                beats(Kind::PingPong, Beat::THREE_QUARTERS),
                beats(Kind::Flanger, Beat::EIGHT),
                beats(Kind::Phaser, Beat::FOUR),
                beats(Kind::VinylBrake, Beat::TWO).at_depth(1.0),
                beats(Kind::Trans, Beat::QUARTER),
                beats(Kind::Pan, Beat::HALF),
                beats(Kind::BpfEcho, Beat::HALF),
                beats(Kind::LowCutEcho, Beat::HALF),
                beats(Kind::MtDelay, Beat::ONE),
                percent(Kind::ReverbUp, 70),
                beats(Kind::UpEcho, Beat::HALF),
                noise,
                beats(Kind::Filter, Beat::EIGHT),
                beats(Kind::Helix, Beat::HALF),
                beats(Kind::SpiralUp, Beat::HALF),
                beats(Kind::SpiralDown, Beat::HALF),
                effect(Kind::Pitch, semitones(12)).at_depth(1.0),
                effect(Kind::Pitch, semitones(-12)).at_depth(1.0),
                beats(Kind::Backspin, Beat::TWO).at_depth(1.0),
                beats(Kind::Brake, Beat::FOUR).at_depth(1.0),
            ],
            Self::Amapiano => [
                roll(Beat::HALF),
                roll(Beat::QUARTER),
                roll(Beat::EIGHTH),
                roll(Beat::SIXTEENTH),
                beats(Kind::Echo, Beat::HALF),
                beats(Kind::Delay, Beat::ONE),
                percent(Kind::Reverb, 80),
                beats(Kind::VinylBrake, Beat::TWO).at_depth(1.0),
                beats(Kind::Trans, Beat::EIGHTH),
                beats(Kind::Trans, Beat::SIXTEENTH),
                beats(Kind::LpfEcho, Beat::THREE_QUARTERS),
                beats(Kind::HpfEcho, Beat::THREE_QUARTERS),
                beats(Kind::LowCutEcho, Beat::ONE),
                beats(Kind::PingPong, Beat::HALF),
                effect(Kind::Pitch, semitones(-12)).at_depth(1.0),
                beats(Kind::Backspin, Beat::ONE).at_depth(1.0),
                beats(Kind::DownEcho, Beat::HALF),
                beats(Kind::UpEcho, Beat::HALF),
                percent(Kind::ReverbDown, 60),
                percent(Kind::ReverbUp, 60),
                beats(Kind::Spiral, Beat::ONE),
                beats(Kind::Helix, Beat::ONE),
                beats(Kind::Phaser, Beat::EIGHT),
                beats(Kind::Flanger, Beat::SIXTEEN),
                percent(Kind::Robot, 0),
                beats(Kind::CrushEcho, Beat::HALF),
                noise,
                effect(Kind::EnigmaJet, signed(Beat::EIGHT, false)),
                effect(Kind::MobiusTriangle, signed(Beat::EIGHT, false)).at_depth(0.4),
                beats(Kind::Stretch, Beat::TWO),
                beats(Kind::EchoOut, Beat::ONE).at_depth(0.8),
                beats(Kind::Brake, Beat::TWO).at_depth(1.0),
            ],
        }
    }
}

impl fmt::Display for PadFxPreset {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

impl FromStr for PadFxPreset {
    type Err = ();

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|preset| preset.name() == text.trim())
            .ok_or(())
    }
}

/// The 32 pads the DJ plays, and the preset they started from.
#[derive(Debug, Clone, PartialEq)]
pub struct PadFxBanks {
    preset: PadFxPreset,
    slots: [PadFxSlot; SLOT_COUNT],
}

impl Default for PadFxBanks {
    fn default() -> Self {
        Self::from_preset(PadFxPreset::default())
    }
}

impl PadFxBanks {
    /// The pads of `preset`, untouched.
    pub fn from_preset(preset: PadFxPreset) -> Self {
        Self {
            preset,
            slots: preset.slots(),
        }
    }

    /// The preset the pads started from.
    pub fn preset(&self) -> PadFxPreset {
        self.preset
    }

    /// Whether the DJ changed any pad from the preset.
    pub fn edited(&self) -> bool {
        self.slots != self.preset.slots()
    }

    /// Pad `pad` of `layer`, pad 0 first.
    pub fn slot(&self, layer: PadFxLayer, pad: usize) -> Option<PadFxSlot> {
        (pad < PAD_COUNT).then(|| self.slots[layer.index() * PAD_COUNT + pad])
    }

    /// What pad `pad` plays in `mode`, with or without SHIFT.
    pub fn pad(&self, mode: PadMode, pad: usize, shift: bool) -> Option<PadFxSlot> {
        self.slot(PadFxLayer::of(mode, shift)?, pad)
    }

    /// Changes pad `pad` of `layer` with `change`.
    pub fn edit(
        &mut self,
        layer: PadFxLayer,
        pad: usize,
        change: impl FnOnce(PadFxSlot) -> PadFxSlot,
    ) {
        if pad < PAD_COUNT {
            let slot = &mut self.slots[layer.index() * PAD_COUNT + pad];
            *slot = change(*slot);
        }
    }

    /// Puts pad `pad` of `layer` back as the preset has it.
    pub fn reset_pad(&mut self, layer: PadFxLayer, pad: usize) {
        if let Some(original) = self.preset.slots().get(layer.index() * PAD_COUNT + pad) {
            self.edit(layer, pad, |_| *original);
        }
    }

    /// The pads as settings files keep them.
    pub fn setting(&self) -> PadFxSetting {
        PadFxSetting {
            preset: self.preset.name().to_owned(),
            pads: self.slots.iter().map(PadFxSlotSetting::of).collect(),
        }
    }

    /// The pads `setting` describes: its preset, with every pad it reads back over it.
    pub fn from_setting(setting: &PadFxSetting) -> Self {
        let mut banks = Self::from_preset(setting.preset.parse().unwrap_or_default());
        for (slot, saved) in banks.slots.iter_mut().zip(&setting.pads) {
            if let Some(read) = saved.slot() {
                *slot = read;
            }
        }
        banks
    }
}

/// The Pad FX as remembered between launches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PadFxSetting {
    /// The preset the pads started from, such as `classic` or `afro-house`.
    pub preset: String,
    /// The 32 pads, PAD FX1 pad 1 first; empty for the preset as it is.
    pub pads: Vec<PadFxSlotSetting>,
}

impl Default for PadFxSetting {
    fn default() -> Self {
        Self {
            preset: PadFxPreset::default().name().to_owned(),
            pads: Vec::new(),
        }
    }
}

/// One pad as remembered. The values are text, so a file written by a later version with new
/// effects still loads, a pad it cannot read left as the preset has it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PadFxSlotSetting {
    /// `loop-roll` or the effect's name, such as `echo`.
    pub sound: String,
    /// The roll length or the effect's parameter, such as `1/2` or `80%`.
    pub parameter: String,
    /// The depth, from 0.0 to 1.0.
    pub depth: f32,
    /// Whether a press latches it on instead of playing it while held.
    #[serde(default)]
    pub latch: bool,
    /// `dry` to take its trail out on release, else it rings out.
    #[serde(default)]
    pub release: String,
}

const DRY_RELEASE: &str = "dry";
const RING_OUT_RELEASE: &str = "ring-out";

impl PadFxSlotSetting {
    fn of(slot: &PadFxSlot) -> Self {
        Self {
            sound: slot.sound.name().to_owned(),
            parameter: slot
                .parameter
                .map_or_else(String::new, |value| value.to_string()),
            depth: slot.depth,
            latch: slot.trigger == PadTrigger::Latch,
            release: match slot.release {
                PadRelease::Dry => DRY_RELEASE,
                PadRelease::RingOut => RING_OUT_RELEASE,
            }
            .to_owned(),
        }
    }

    fn slot(&self) -> Option<PadFxSlot> {
        let sound: PadSound = self.sound.parse().ok()?;
        let parameter = sound.parameter().map(|spec| {
            self.parameter
                .parse()
                .ok()
                .filter(|value| spec.accepts(*value))
                .unwrap_or(spec.default)
        });
        Some(PadFxSlot {
            sound,
            parameter,
            depth: if self.depth.is_finite() {
                self.depth.clamp(0.0, 1.0)
            } else {
                FULL_DEPTH
            },
            trigger: if self.latch {
                PadTrigger::Latch
            } else {
                PadTrigger::Hold
            },
            release: if self.release == DRY_RELEASE {
                PadRelease::Dry
            } else {
                PadRelease::RingOut
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn every_preset_fills_all_32_pads_with_sounds_that_take_their_parameter() {
        for preset in PadFxPreset::ALL {
            let slots = preset.slots();
            for slot in slots {
                if let (Some(spec), Some(value)) = (slot.sound.parameter(), slot.parameter) {
                    assert!(spec.accepts(value), "{preset:?}: {slot:?}");
                }
                assert!((0.0..=1.0).contains(&slot.depth), "{preset:?}: {slot:?}");
            }
            let distinct: HashSet<PadSound> = slots.iter().map(|slot| slot.sound).collect();
            assert!(
                distinct.len() >= 20,
                "{preset:?} has only {} sounds",
                distinct.len()
            );
        }
    }

    #[test]
    fn each_factory_noise_pad_sounds_on_a_short_tap() {
        use dubplate_engine::{BeatGrid, DeckId, Track, offline_engine};

        let rate = 24_000;
        for preset in PadFxPreset::ALL {
            let slot = preset
                .slots()
                .into_iter()
                .find(|slot| slot.sound == PadSound::Effect(EffectKind::Noise))
                .expect("factory Noise pad");
            let PadFx::Effect(pad) = slot.play() else {
                panic!("expected a beat effect")
            };
            let (mut controller, mut renderer) = offline_engine(rate);
            let track = Track::from_interleaved_stereo(&vec![0.0; rate as usize * 2], rate)
                .expect("silent track");
            controller.load_track(DeckId::A, track).expect("load");
            controller
                .set_beat_grid(
                    DeckId::A,
                    Some(BeatGrid::new(120.0, 0.0).expect("valid grid")),
                )
                .expect("set grid");
            controller.play(DeckId::A).expect("play");
            controller.hold_pad_effect(DeckId::A, pad).expect("press");
            // This 40 ms tap ends before a 1/8-beat delay at 120 BPM.
            let mut pressed = vec![0.0; 960 * 2];
            renderer.render(&mut pressed, 2);
            controller.release_pad_effect(DeckId::A).expect("release");
            let mut released = vec![0.0; 2_880 * 2];
            renderer.render(&mut released, 2);
            assert!(
                released.iter().any(|x| x.abs() > 0.01),
                "{preset:?} lost the burst"
            );
        }
    }

    #[test]
    fn the_classic_set_opens_on_rolls_from_a_quarter_to_a_thirty_second() {
        let banks = PadFxBanks::default();
        let rolls: Vec<PadFx> = (0..4)
            .map(|pad| banks.pad(PadMode::PadFx1, pad, false).unwrap().play())
            .collect();
        assert_eq!(
            rolls,
            [
                PadFx::Roll(BeatLength::QUARTER),
                PadFx::Roll(BeatLength::EIGHTH),
                PadFx::Roll(BeatLength::SIXTEENTH),
                PadFx::Roll(BeatLength::THIRTY_SECOND),
            ]
        );
        assert_eq!(banks.pad(PadMode::HotCue, 0, false), None);
        assert_eq!(banks.pad(PadMode::PadFx1, 8, false), None);
        let shifted = banks.pad(PadMode::PadFx2, 7, true).unwrap();
        assert_eq!(shifted.sound, PadSound::Effect(EffectKind::EchoOut));
    }

    #[test]
    fn editing_a_pad_steps_its_sound_parameter_depth_trigger_and_release() {
        let mut banks = PadFxBanks::default();
        let layer = PadFxLayer::Fx1;
        banks.edit(layer, 4, |slot| slot.step_parameter(1));
        assert_eq!(
            banks.slot(layer, 4).unwrap().label(),
            ("Echo", "3/4".to_owned())
        );
        banks.edit(layer, 4, |slot| slot.step_depth(-20));
        assert_eq!(banks.slot(layer, 4).unwrap().depth, 0.05);
        banks.edit(layer, 4, |slot| slot.toggle_trigger().toggle_release());
        let edited = banks.slot(layer, 4).unwrap();
        assert_eq!(edited.trigger, PadTrigger::Latch);
        assert_eq!(edited.release, PadRelease::Dry);
        assert!(banks.edited());

        banks.edit(layer, 0, |slot| slot.step_sound(1));
        let first_effect = banks.slot(layer, 0).unwrap();
        assert_eq!(first_effect.sound, PadSound::Effect(EffectKind::Delay));
        assert_eq!(first_effect.release, PadRelease::RingOut);
        banks.edit(layer, 0, |slot| slot.step_sound(-2));
        assert_eq!(
            banks.slot(layer, 0).unwrap().sound,
            PadSound::Effect(EffectKind::EchoOut),
            "the list wraps"
        );
        banks.edit(layer, 0, |slot| slot.step_parameter(-99));
        assert_eq!(
            banks.slot(layer, 0).unwrap().parameter,
            EffectKind::EchoOut.parameter().map(|spec| spec.steps[0])
        );

        banks.reset_pad(layer, 4);
        banks.reset_pad(layer, 0);
        assert!(!banks.edited());
    }

    #[test]
    fn the_pads_survive_a_round_trip_through_settings_and_a_bad_pad_keeps_the_preset() {
        let mut banks = PadFxBanks::from_preset(PadFxPreset::AfroHouse);
        banks.edit(PadFxLayer::Fx2Shift, 3, |slot| {
            slot.step_sound(5).toggle_trigger().step_depth(2)
        });
        let setting = banks.setting();
        let text = toml::to_string(&setting).unwrap();
        let read: PadFxSetting = toml::from_str(&text).unwrap();
        assert_eq!(PadFxBanks::from_setting(&read), banks);

        let mut damaged = setting;
        damaged.pads[0].sound = "warp-drive".to_owned();
        damaged.pads[1].parameter = "lots".to_owned();
        let restored = PadFxBanks::from_setting(&damaged);
        assert_eq!(
            restored.slot(PadFxLayer::Fx1, 0),
            banks.slot(PadFxLayer::Fx1, 0)
        );
        assert_eq!(
            restored.slot(PadFxLayer::Fx1, 1).unwrap().parameter,
            Some(ParameterValue::Beats(BeatLength::QUARTER)),
            "an unreadable roll length takes the roll's default"
        );
        assert_eq!(
            PadFxBanks::from_setting(&PadFxSetting::default()),
            PadFxBanks::default()
        );
    }

    #[test]
    fn layers_follow_rekordbox_numbering() {
        assert_eq!(
            PadFxLayer::of(PadMode::PadFx1, true),
            Some(PadFxLayer::Fx1Shift)
        );
        assert_eq!(
            PadFxLayer::of(PadMode::PadFx2, false),
            Some(PadFxLayer::Fx2)
        );
        assert_eq!(PadFxLayer::of(PadMode::Sampler, false), None);
        assert_eq!(PadFxLayer::Fx1Shift.numbers(), "9\u{2013}16");
        assert_eq!(PadFxLayer::Fx2Shift.numbers(), "25\u{2013}32");
    }
}
