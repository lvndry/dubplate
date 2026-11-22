//! The effects as the DJ picks them: their names on screen, the groups of the effect picker,
//! and the order FX SELECT steps through.

use dubplate_engine::{BeatLength, DeckId, EffectKind, EffectTarget, EffectUnitState};
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
pub const CATALOGUE: [EffectGroup; 3] = [
    EffectGroup {
        name: "Beat",
        effects: &[
            EffectKind::Echo,
            EffectKind::PingPong,
            EffectKind::Reverb,
            EffectKind::Flanger,
            EffectKind::Phaser,
            EffectKind::Gate,
        ],
    },
    EffectGroup {
        name: "Sweep",
        effects: &[
            EffectKind::Filter,
            EffectKind::LowPass,
            EffectKind::HighPass,
            EffectKind::BandPass,
            EffectKind::Crusher,
        ],
    },
    EffectGroup {
        name: "Release",
        effects: &[EffectKind::Brake, EffectKind::Backspin],
    },
];

/// The effect's name on screen.
pub fn effect_label(kind: EffectKind) -> &'static str {
    match kind {
        EffectKind::Echo => "Echo",
        EffectKind::PingPong => "Ping pong",
        EffectKind::Reverb => "Reverb",
        EffectKind::LowPass => "Low-pass",
        EffectKind::HighPass => "High-pass",
        EffectKind::BandPass => "Band-pass",
        EffectKind::Filter => "Filter",
        EffectKind::Flanger => "Flanger",
        EffectKind::Phaser => "Phaser",
        EffectKind::Gate => "Gate",
        EffectKind::Crusher => "Crush",
        EffectKind::Brake => "Brake",
        EffectKind::Backspin => "Backspin",
    }
}

/// The effect's name on a performance pad, short enough to sit beside the beat length:
/// the filters by their DJ shorthand, such as `BPF` for the band-pass.
pub fn effect_pad_label(kind: EffectKind) -> &'static str {
    match kind {
        EffectKind::Echo => "Echo",
        EffectKind::PingPong => "Ping",
        EffectKind::Reverb => "Reverb",
        EffectKind::LowPass => "LPF",
        EffectKind::HighPass => "HPF",
        EffectKind::BandPass => "BPF",
        EffectKind::Filter => "Filter",
        EffectKind::Flanger => "Flanger",
        EffectKind::Phaser => "Phaser",
        EffectKind::Gate => "Gate",
        EffectKind::Crusher => "Crush",
        EffectKind::Brake => "Brake",
        EffectKind::Backspin => "Spin",
    }
}

/// Every effect in the picker's order, which FX SELECT steps through.
fn picker_order() -> impl DoubleEndedIterator<Item = EffectKind> + Clone {
    CATALOGUE
        .into_iter()
        .flat_map(|group| group.effects.iter().copied())
}

/// The effect `steps` places after `kind` in the picker's order, going round at either end.
pub fn step_effect(kind: EffectKind, steps: i32) -> EffectKind {
    let order: Vec<EffectKind> = picker_order().collect();
    let Some(position) = order.iter().position(|candidate| *candidate == kind) else {
        return kind;
    };
    let count = i64::try_from(order.len()).unwrap_or(1);
    let index = (i64::try_from(position).unwrap_or(0) + i64::from(steps)).rem_euclid(count);
    usize::try_from(index)
        .ok()
        .and_then(|index| order.get(index).copied())
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

/// The next standard beat length up, or down.
pub fn step_beat(beat: BeatLength, direction: Direction) -> BeatLength {
    match direction {
        Direction::Up => beat.longer(),
        Direction::Down => beat.shorter(),
    }
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
    /// The beat length, such as `1/2`.
    pub beat: String,
    /// The level and depth knob, from 0.0 to 1.0.
    pub depth: f32,
}

impl EffectUnitSetting {
    /// The setting for a unit in `state`.
    pub fn of(state: &EffectUnitState) -> Self {
        Self {
            effect: state.kind.name().to_owned(),
            target: target_letter(state.target).to_owned(),
            beat: state.beat.to_string(),
            depth: state.depth,
        }
    }

    /// The effect, if the name is one Dubplate knows.
    pub fn kind(&self) -> Option<EffectKind> {
        self.effect.parse().ok()
    }

    /// The target, if the letter names one.
    pub fn target(&self) -> Option<EffectTarget> {
        target_from_letter(&self.target)
    }

    /// The beat length, if it reads as one.
    pub fn beat(&self) -> Option<BeatLength> {
        self.beat.parse().ok()
    }

    /// The depth, kept between 0.0 and 1.0.
    pub fn depth(&self) -> Option<f32> {
        self.depth.is_finite().then(|| self.depth.clamp(0.0, 1.0))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn pad_labels_leave_room_for_the_beat_length() {
        for kind in picker_order() {
            let label = effect_pad_label(kind);
            assert!(label.chars().count() <= 7, "{label}");
        }
    }

    #[test]
    fn every_effect_is_in_exactly_one_group() {
        let listed: Vec<EffectKind> = picker_order().collect();
        let unique: HashSet<EffectKind> = listed.iter().copied().collect();
        assert_eq!(listed.len(), unique.len());
        for kind in EffectKind::ALL {
            assert!(unique.contains(&kind), "{kind} is in no group");
        }
    }

    #[test]
    fn fx_select_steps_round_the_picker() {
        assert_eq!(step_effect(EffectKind::Echo, 1), EffectKind::PingPong);
        assert_eq!(step_effect(EffectKind::Echo, -1), EffectKind::Backspin);
        assert_eq!(step_effect(EffectKind::Backspin, 1), EffectKind::Echo);
        assert_eq!(step_effect(EffectKind::Gate, 1), EffectKind::Filter);
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
    fn the_beat_arrows_step_through_the_standard_lengths() {
        assert_eq!(step_beat(BeatLength::ONE, Direction::Up), BeatLength::TWO);
        assert_eq!(
            step_beat(BeatLength::ONE, Direction::Down),
            BeatLength::THREE_QUARTERS
        );
        assert_eq!(
            step_beat(BeatLength::THIRTY_TWO, Direction::Up),
            BeatLength::THIRTY_TWO
        );
    }

    #[test]
    fn a_unit_setting_reads_back_what_it_wrote_and_ignores_what_it_does_not_know() {
        let state = EffectUnitState {
            kind: EffectKind::PingPong,
            target: EffectTarget::Deck(DeckId::B),
            beat: BeatLength::THREE_QUARTERS,
            depth: 0.4,
            enabled: true,
            sounding: true,
        };
        let setting = EffectUnitSetting::of(&state);
        assert_eq!(setting.kind(), Some(EffectKind::PingPong));
        assert_eq!(setting.target(), Some(EffectTarget::Deck(DeckId::B)));
        assert_eq!(setting.beat(), Some(BeatLength::THREE_QUARTERS));
        assert_eq!(setting.depth(), Some(0.4));
        let unknown = EffectUnitSetting {
            effect: "wah".to_owned(),
            target: "C".to_owned(),
            beat: "64".to_owned(),
            depth: f32::NAN,
        };
        assert_eq!(unknown.kind(), None);
        assert_eq!(unknown.target(), None);
        assert_eq!(unknown.beat(), None);
        assert_eq!(unknown.depth(), None);
        assert_eq!(target_from_letter("m"), Some(EffectTarget::Master));
    }
}
