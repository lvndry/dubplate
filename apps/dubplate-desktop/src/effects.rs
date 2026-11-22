//! The effects as the DJ picks them: their names on screen, the groups of the effect picker,
//! and the order FX SELECT steps through.

use dubplate_engine::{
    DeckId, EffectKind, EffectTarget, EffectUnitState, ParameterValue, ReleaseFx,
};
use serde::{Deserialize, Serialize};

use crate::commands::Direction;

/// A group of the effect picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectGroup {
    /// The group's name.
    pub name: &'static str,
    /// Its effects, in the order shown.
    pub effects: &'static [EffectKind],
}

/// The effect picker's groups, in order. Every effect is in exactly one.
pub const CATALOGUE: [EffectGroup; 6] = [
    EffectGroup {
        name: "Echo",
        effects: &[
            EffectKind::Delay,
            EffectKind::Echo,
            EffectKind::PingPong,
            EffectKind::Spiral,
            EffectKind::RevDelay,
            EffectKind::MtDelay,
            EffectKind::UpEcho,
            EffectKind::DownEcho,
            EffectKind::LowCutEcho,
            EffectKind::Helix,
            EffectKind::HpfEcho,
            EffectKind::LpfEcho,
            EffectKind::BpfEcho,
            EffectKind::CrushEcho,
            EffectKind::SpiralUp,
            EffectKind::SpiralDown,
        ],
    },
    EffectGroup {
        name: "Space",
        effects: &[
            EffectKind::Reverb,
            EffectKind::ReverbUp,
            EffectKind::ReverbDown,
        ],
    },
    EffectGroup {
        name: "Rhythm",
        effects: &[
            EffectKind::Trans,
            EffectKind::Pan,
            EffectKind::Roll,
            EffectKind::SlipRoll,
            EffectKind::RevRoll,
            EffectKind::Stretch,
        ],
    },
    EffectGroup {
        name: "Sweep",
        effects: &[
            EffectKind::Filter,
            EffectKind::Flanger,
            EffectKind::Phaser,
            EffectKind::EnigmaJet,
            EffectKind::LowPass,
            EffectKind::HighPass,
            EffectKind::BandPass,
        ],
    },
    EffectGroup {
        name: "Tone",
        effects: &[
            EffectKind::Pitch,
            EffectKind::Robot,
            EffectKind::MobiusSaw,
            EffectKind::MobiusTriangle,
            EffectKind::Noise,
            EffectKind::Crusher,
        ],
    },
    EffectGroup {
        name: "Release",
        effects: &[
            EffectKind::EchoOut,
            EffectKind::VinylBrake,
            EffectKind::Brake,
            EffectKind::Backspin,
        ],
    },
];

/// The effect's name on screen.
pub fn effect_label(kind: EffectKind) -> &'static str {
    match kind {
        EffectKind::Delay => "Delay",
        EffectKind::Echo => "Echo",
        EffectKind::Spiral => "Spiral",
        EffectKind::Reverb => "Reverb",
        EffectKind::RevDelay => "Rev delay",
        EffectKind::MtDelay => "MT delay",
        EffectKind::UpEcho => "Up echo",
        EffectKind::DownEcho => "Down echo",
        EffectKind::Trans => "Trans",
        EffectKind::Pan => "Pan",
        EffectKind::Filter => "Filter",
        EffectKind::Flanger => "Flanger",
        EffectKind::Phaser => "Phaser",
        EffectKind::SlipRoll => "Slip roll",
        EffectKind::Roll => "Roll",
        EffectKind::RevRoll => "Rev roll",
        EffectKind::Robot => "Robot",
        EffectKind::Pitch => "Pitch",
        EffectKind::EnigmaJet => "Enigma jet",
        EffectKind::MobiusSaw => "Mobius saw",
        EffectKind::MobiusTriangle => "Mobius tri",
        EffectKind::LowCutEcho => "Low cut echo",
        EffectKind::PingPong => "Ping pong",
        EffectKind::Helix => "Helix",
        EffectKind::VinylBrake => "Vinyl brake",
        EffectKind::Stretch => "Stretch",
        EffectKind::BpfEcho => "BPF echo",
        EffectKind::Noise => "Noise",
        EffectKind::SpiralUp => "Spiral up",
        EffectKind::ReverbUp => "Reverb up",
        EffectKind::HpfEcho => "HPF echo",
        EffectKind::LpfEcho => "LPF echo",
        EffectKind::CrushEcho => "Crush echo",
        EffectKind::SpiralDown => "Spiral down",
        EffectKind::ReverbDown => "Reverb down",
        EffectKind::LowPass => "Low-pass",
        EffectKind::HighPass => "High-pass",
        EffectKind::BandPass => "Band-pass",
        EffectKind::Crusher => "Crush",
        EffectKind::Brake => "Brake",
        EffectKind::Backspin => "Backspin",
        EffectKind::EchoOut => "Echo out",
    }
}

/// The effect's name on a performance pad, short enough to sit beside the beat length:
/// the filters by their DJ shorthand, such as `BPF` for the band-pass.
pub fn effect_pad_label(kind: EffectKind) -> &'static str {
    match kind {
        EffectKind::Delay => "Delay",
        EffectKind::Echo => "Echo",
        EffectKind::Spiral => "Spiral",
        EffectKind::Reverb => "Reverb",
        EffectKind::RevDelay => "Rev dly",
        EffectKind::MtDelay => "MT dly",
        EffectKind::UpEcho => "Up echo",
        EffectKind::DownEcho => "Dn echo",
        EffectKind::Trans => "Trans",
        EffectKind::Pan => "Pan",
        EffectKind::Filter => "Filter",
        EffectKind::Flanger => "Flanger",
        EffectKind::Phaser => "Phaser",
        EffectKind::SlipRoll => "Slip",
        EffectKind::Roll => "Roll",
        EffectKind::RevRoll => "RevRoll",
        EffectKind::Robot => "Robot",
        EffectKind::Pitch => "Pitch",
        EffectKind::EnigmaJet => "Enigma",
        EffectKind::MobiusSaw => "Mob saw",
        EffectKind::MobiusTriangle => "Mob tri",
        EffectKind::LowCutEcho => "LC echo",
        EffectKind::PingPong => "Ping",
        EffectKind::Helix => "Helix",
        EffectKind::VinylBrake => "Vinyl",
        EffectKind::Stretch => "Stretch",
        EffectKind::BpfEcho => "BPF Ec",
        EffectKind::Noise => "Noise",
        EffectKind::SpiralUp => "Spir Up",
        EffectKind::ReverbUp => "Rev Up",
        EffectKind::HpfEcho => "HPF Ec",
        EffectKind::LpfEcho => "LPF Ec",
        EffectKind::CrushEcho => "Crsh Ec",
        EffectKind::SpiralDown => "Spir Dn",
        EffectKind::ReverbDown => "Rev Dn",
        EffectKind::LowPass => "LPF",
        EffectKind::HighPass => "HPF",
        EffectKind::BandPass => "BPF",
        EffectKind::Crusher => "Crush",
        EffectKind::Brake => "Brake",
        EffectKind::Backspin => "Spin",
        EffectKind::EchoOut => "EchoOut",
    }
}

/// The effect `steps` places after `kind` in rekordbox's Beat FX order, which FX SELECT steps
/// through, going round at either end.
pub fn step_effect(kind: EffectKind, steps: i32) -> EffectKind {
    let count = i64::try_from(EffectKind::ALL.len()).unwrap_or(1);
    let index = (i64::try_from(kind.index()).unwrap_or(0) + i64::from(steps)).rem_euclid(count);
    usize::try_from(index)
        .ok()
        .and_then(|index| EffectKind::ALL.get(index).copied())
        .unwrap_or(kind)
}

/// The effects whose name contains `query`, ignoring case, in each group. Groups left empty are
/// left out.
pub fn search(query: &str) -> Vec<(&'static str, Vec<EffectKind>)> {
    let query = query.trim().to_lowercase();
    CATALOGUE
        .iter()
        .filter_map(|group| {
            let effects: Vec<EffectKind> = group
                .effects
                .iter()
                .copied()
                .filter(|kind| effect_label(*kind).to_lowercase().contains(&query))
                .collect();
            (!effects.is_empty()).then_some((group.name, effects))
        })
        .collect()
}

/// The value one BEAT button press up or down from `value` gives `kind`'s parameter, stopping
/// at either end, or `None` for an effect without a parameter.
pub fn step_parameter(
    kind: EffectKind,
    value: Option<ParameterValue>,
    direction: Direction,
) -> Option<ParameterValue> {
    let spec = kind.parameter()?;
    let steps = match direction {
        Direction::Up => 1,
        Direction::Down => -1,
    };
    Some(spec.step(value.unwrap_or(spec.default), steps))
}

/// The parameter as the strip shows it: `1/2`, `75%`, `+3 st`, or a dash for none.
pub fn parameter_label(value: Option<ParameterValue>) -> String {
    value.map_or_else(|| "\u{2013}".to_owned(), |value| value.to_string())
}

/// The letter of a target on the strip's target buttons.
pub fn target_letter(target: EffectTarget) -> &'static str {
    match target {
        EffectTarget::Deck(DeckId::A) => "A",
        EffectTarget::Deck(DeckId::B) => "B",
        EffectTarget::BothDecks => "AB",
        EffectTarget::Master => "M",
    }
}

/// The target a letter of the strip names.
fn target_from_letter(letter: &str) -> Option<EffectTarget> {
    EffectTarget::ALL
        .into_iter()
        .find(|target| target_letter(*target).eq_ignore_ascii_case(letter))
}

/// An effect unit as remembered between launches. The values are text, so a file written by a
/// later version with new effects still loads, the unknown values left at their default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectUnitSetting {
    /// The effect's name, such as `echo`.
    pub effect: String,
    /// `A`, `B` or `M` for the master.
    pub target: String,
    /// The effect's parameter, such as `1/2`, `75%` or `+3 st`. The key keeps its first name,
    /// `beat`, so files written before effects had other parameters still load.
    #[serde(rename = "beat")]
    pub parameter: String,
    /// The level and depth knob, from 0.0 to 1.0.
    pub depth: f32,
    /// The Release FX, such as `echo` or `vinyl brake`; empty in files written before units had
    /// one.
    #[serde(default)]
    pub release: String,
}

impl EffectUnitSetting {
    /// The setting for a unit in `state`.
    pub fn of(state: &EffectUnitState) -> Self {
        Self {
            effect: state.kind.name().to_owned(),
            target: target_letter(state.target).to_owned(),
            parameter: state
                .parameter
                .map_or_else(String::new, |value| value.to_string()),
            depth: state.depth,
            release: state.release.to_string(),
        }
    }

    /// The Release FX, if the name is one Dubplate knows.
    pub fn release(&self) -> Option<ReleaseFx> {
        ReleaseFx::ALL
            .into_iter()
            .find(|release| release.to_string() == self.release)
    }

    /// The effect, if the name is one Dubplate knows.
    pub fn kind(&self) -> Option<EffectKind> {
        self.effect.parse().ok()
    }

    /// The target, if the letter names one.
    pub fn target(&self) -> Option<EffectTarget> {
        target_from_letter(&self.target)
    }

    /// The parameter, if it reads as one.
    pub fn parameter(&self) -> Option<ParameterValue> {
        self.parameter.parse().ok()
    }

    /// The depth, kept between 0.0 and 1.0.
    pub fn depth(&self) -> Option<f32> {
        self.depth.is_finite().then(|| self.depth.clamp(0.0, 1.0))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use dubplate_engine::BeatLength;

    use super::*;

    #[test]
    fn pad_labels_leave_room_for_the_beat_length() {
        for kind in EffectKind::ALL {
            let label = effect_pad_label(kind);
            assert!(label.chars().count() <= 7, "{label}");
        }
    }

    #[test]
    fn every_effect_is_in_exactly_one_group() {
        let listed: Vec<EffectKind> = CATALOGUE
            .iter()
            .flat_map(|group| group.effects.iter().copied())
            .collect();
        let unique: HashSet<EffectKind> = listed.iter().copied().collect();
        assert_eq!(listed.len(), unique.len());
        for kind in EffectKind::ALL {
            assert!(unique.contains(&kind), "{kind} is in no group");
        }
    }

    #[test]
    fn fx_select_steps_through_rekordboxs_beat_fx_order() {
        assert_eq!(step_effect(EffectKind::Delay, 1), EffectKind::Echo);
        assert_eq!(step_effect(EffectKind::Echo, 1), EffectKind::Spiral);
        assert_eq!(step_effect(EffectKind::Delay, -1), EffectKind::EchoOut);
        assert_eq!(step_effect(EffectKind::EchoOut, 1), EffectKind::Delay);
        assert_eq!(step_effect(EffectKind::Trans, 1), EffectKind::Pan);
        assert_eq!(step_effect(EffectKind::ReverbDown, 1), EffectKind::LowPass);
    }

    #[test]
    fn searching_keeps_the_groups_that_match() {
        let found = search("pass");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, "Sweep");
        assert_eq!(
            found[0].1,
            [
                EffectKind::LowPass,
                EffectKind::HighPass,
                EffectKind::BandPass
            ]
        );
        assert_eq!(search(" ").len(), CATALOGUE.len());
        assert!(search("wah").is_empty());
    }

    #[test]
    fn the_beat_arrows_step_through_each_effects_own_values() {
        let beat = |length| Some(ParameterValue::Beats(length));
        assert_eq!(
            step_parameter(EffectKind::Echo, beat(BeatLength::ONE), Direction::Up),
            beat(BeatLength::TWO)
        );
        assert_eq!(
            step_parameter(EffectKind::Echo, beat(BeatLength::ONE), Direction::Down),
            beat(BeatLength::THREE_QUARTERS)
        );
        assert_eq!(
            step_parameter(EffectKind::Echo, beat(BeatLength::SIXTEEN), Direction::Up),
            beat(BeatLength::SIXTEEN)
        );
        assert_eq!(
            step_parameter(
                EffectKind::Reverb,
                Some(ParameterValue::Percent(50)),
                Direction::Up
            ),
            Some(ParameterValue::Percent(60))
        );
        assert_eq!(
            step_parameter(EffectKind::Crusher, None, Direction::Up),
            None
        );
        assert_eq!(parameter_label(Some(ParameterValue::Percent(75))), "75%");
        assert_eq!(parameter_label(None), "\u{2013}");
    }

    #[test]
    fn a_unit_setting_reads_back_what_it_wrote_and_ignores_what_it_does_not_know() {
        let state = EffectUnitState {
            kind: EffectKind::PingPong,
            target: EffectTarget::Deck(DeckId::B),
            parameter: Some(ParameterValue::Beats(BeatLength::THREE_QUARTERS)),
            depth: 0.4,
            enabled: true,
            release: ReleaseFx::BackSpin,
            releasing: false,
            sounding: true,
        };
        let setting = EffectUnitSetting::of(&state);
        assert_eq!(setting.release(), Some(ReleaseFx::BackSpin));
        assert_eq!(setting.kind(), Some(EffectKind::PingPong));
        assert_eq!(setting.target(), Some(EffectTarget::Deck(DeckId::B)));
        assert_eq!(
            setting.parameter(),
            Some(ParameterValue::Beats(BeatLength::THREE_QUARTERS))
        );
        assert_eq!(setting.depth(), Some(0.4));
        let unknown = EffectUnitSetting {
            effect: "wah".to_owned(),
            target: "C".to_owned(),
            parameter: "64".to_owned(),
            depth: f32::NAN,
            release: "scratch".to_owned(),
        };
        assert_eq!(unknown.kind(), None);
        assert_eq!(unknown.target(), None);
        assert_eq!(unknown.parameter(), None);
        assert_eq!(unknown.release(), None);
        assert_eq!(unknown.depth(), None);
        assert_eq!(target_from_letter("m"), Some(EffectTarget::Master));
    }
}
