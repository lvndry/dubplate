//! The sampler's slots, volume and CUE.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use crate::command::{Command, SamplerCommand};
use crate::error::{EngineError, check_range};
use crate::sampler::{SAMPLER_SLOTS, SampleSettings, SamplerState, SlotState};
use crate::track::Track;
use dubplate_fx::{MAX_TEMPO_BPM, MIN_TEMPO_BPM};

use super::{Controller, MAX_GAIN};

impl Controller {
    /// Puts a sample in a sampler slot, counted from 0, with its settings. Any sample there
    /// stops. Slots 0 to 7 sit on the left pads of a DDJ-FLX4 and 8 to 15 on the right.
    pub fn load_sample(
        &mut self,
        slot: usize,
        track: Track,
        settings: SampleSettings,
    ) -> Result<(), EngineError> {
        check_sampler_slot(slot)?;
        check_sample_settings(settings)?;
        self.send(Command::Sampler(SamplerCommand::Load {
            slot,
            track: Some(Arc::clone(track.buffer())),
            settings,
        }))?;
        self.samples[slot] = Some((track, settings));
        Ok(())
    }

    /// Empties a sampler slot.
    pub fn eject_sample(&mut self, slot: usize) -> Result<(), EngineError> {
        check_sampler_slot(slot)?;
        self.send(Command::Sampler(SamplerCommand::Load {
            slot,
            track: None,
            settings: SampleSettings::default(),
        }))?;
        self.samples[slot] = None;
        Ok(())
    }

    /// Changes a loaded slot's settings while it plays or not.
    pub fn set_sample_settings(
        &mut self,
        slot: usize,
        settings: SampleSettings,
    ) -> Result<(), EngineError> {
        check_sampler_slot(slot)?;
        check_sample_settings(settings)?;
        self.send(Command::Sampler(SamplerCommand::SetSettings {
            slot,
            settings,
        }))?;
        if let Some((_, current)) = &mut self.samples[slot] {
            *current = settings;
        }
        Ok(())
    }

    /// A sampler pad went down: plays the slot from its start, or a synced loop in phase with
    /// the tempo master; a slot already playing starts again.
    pub fn trigger_sample(&mut self, slot: usize) -> Result<(), EngineError> {
        check_sampler_slot(slot)?;
        self.send(Command::Sampler(SamplerCommand::Trigger(slot)))
    }

    /// A sampler pad came up: a gate slot stops.
    pub fn release_sample(&mut self, slot: usize) -> Result<(), EngineError> {
        check_sampler_slot(slot)?;
        self.send(Command::Sampler(SamplerCommand::Release(slot)))
    }

    /// Stops a slot with a short fade, as SHIFT and a sampler pad do.
    pub fn stop_sample(&mut self, slot: usize) -> Result<(), EngineError> {
        check_sampler_slot(slot)?;
        self.send(Command::Sampler(SamplerCommand::Stop(slot)))
    }

    /// The sampler volume, from 0.0 (silent) to 1.0, on the channel fader's curve. The sampler
    /// plays to the master after the channel faders.
    pub fn set_sampler_volume(&mut self, position: f32) -> Result<(), EngineError> {
        check_range("sampler volume", f64::from(position), 0.0, 1.0)?;
        self.send(Command::Sampler(SamplerCommand::SetVolume(position)))?;
        self.sampler_volume = position;
        Ok(())
    }

    /// The sampler's CUE button: sends the sampler to the headphones too.
    pub fn set_sampler_cue(&mut self, enabled: bool) -> Result<(), EngineError> {
        self.send(Command::Sampler(SamplerCommand::SetCue(enabled)))?;
        self.sampler_cue = enabled;
        Ok(())
    }

    /// The sampler's slots, volume and CUE, with which slots play as the audio thread last
    /// published it.
    pub fn sampler_state(&self) -> SamplerState {
        let playing = self.shared.sampler_playing.load(Ordering::Relaxed);
        SamplerState {
            slots: std::array::from_fn(|index| {
                self.samples[index].as_ref().map(|(_, settings)| SlotState {
                    settings: *settings,
                    playing: playing & (1 << index) != 0,
                })
            }),
            volume: self.sampler_volume,
            cue: self.sampler_cue,
        }
    }
}

fn check_sampler_slot(slot: usize) -> Result<(), EngineError> {
    if slot < SAMPLER_SLOTS {
        Ok(())
    } else {
        Err(EngineError::InvalidSamplerSlot {
            slot,
            slot_count: SAMPLER_SLOTS,
        })
    }
}

fn check_sample_settings(settings: SampleSettings) -> Result<(), EngineError> {
    check_range(
        "sample gain",
        f64::from(settings.gain),
        0.0,
        f64::from(MAX_GAIN),
    )?;
    if let Some(bpm) = settings.bpm {
        check_range("sample BPM", bpm, MIN_TEMPO_BPM, MAX_TEMPO_BPM)?;
    }
    Ok(())
}
