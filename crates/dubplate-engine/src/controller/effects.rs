//! The beat effect units.

use std::sync::atomic::Ordering;

use crate::command::Command;
use crate::effects::{EffectChange, EffectTarget, EffectUnitId, EffectUnitState};
use crate::error::{EngineError, check_range};
use crate::{BeatLength, EffectKind};

use super::Controller;

impl Controller {
    /// Selects an effect unit's effect. When the unit is on, the old effect fades out, or rings
    /// out its trail, while the new one fades in.
    pub fn set_effect_kind(
        &mut self,
        unit: EffectUnitId,
        kind: EffectKind,
    ) -> Result<(), EngineError> {
        self.change_effect(unit, EffectChange::Kind(kind))
    }

    /// Moves an effect unit to a deck or the master, like a mixer's channel select switch. When
    /// the unit is on, the effect fades out, or rings out its trail, where it was.
    pub fn set_effect_target(
        &mut self,
        unit: EffectUnitId,
        target: EffectTarget,
    ) -> Result<(), EngineError> {
        self.change_effect(unit, EffectChange::Target(target))
    }

    /// Sets an effect unit's beat length, like a mixer's beat buttons. Step through the
    /// buttons' lengths with [`BeatLength::longer`] and [`BeatLength::shorter`].
    pub fn set_effect_beat(
        &mut self,
        unit: EffectUnitId,
        beat: BeatLength,
    ) -> Result<(), EngineError> {
        self.change_effect(unit, EffectChange::Beat(beat))
    }

    /// Turns an effect unit's level and depth knob, from 0 to 1.
    pub fn set_effect_depth(&mut self, unit: EffectUnitId, depth: f32) -> Result<(), EngineError> {
        check_range("effect depth", f64::from(depth), 0.0, 1.0)?;
        self.change_effect(unit, EffectChange::Depth(depth))
    }

    /// Switches an effect unit on or off. Echo, ping-pong and reverb ring out when switched off.
    pub fn set_effect_enabled(
        &mut self,
        unit: EffectUnitId,
        enabled: bool,
    ) -> Result<(), EngineError> {
        self.change_effect(unit, EffectChange::Enabled(enabled))
    }

    /// The effect on button: switches a unit on when it is off and off when it is on.
    pub fn toggle_effect(&mut self, unit: EffectUnitId) -> Result<(), EngineError> {
        let enabled = self.effect_units[unit.index()].is_enabled();
        self.set_effect_enabled(unit, !enabled)
    }

    /// An effect unit's settings, and whether the audio thread last found it sounding.
    pub fn effect_unit(&self, unit: EffectUnitId) -> EffectUnitState {
        let sounding = self.shared.effect_sounding[unit.index()].load(Ordering::Relaxed);
        self.effect_units[unit.index()].with_sounding(sounding)
    }

    fn change_effect(
        &mut self,
        unit: EffectUnitId,
        change: EffectChange,
    ) -> Result<(), EngineError> {
        self.send(Command::ChangeEffect { unit, change })?;
        self.effect_units[unit.index()].apply(change);
        Ok(())
    }
}
