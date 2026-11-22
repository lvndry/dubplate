use std::sync::Arc;

use crate::curves::{Ramp, channel_fader_gain};
use crate::mixer::RetireQueue;
use crate::processor::StereoBlock;
use crate::sync::{MasterBeat, sync_multiple};
use crate::track::TrackBuffer;
use crate::voice::Voice;

/// Number of sampler slots: the DDJ-FLX4's left pads play slots 1 to 8, its right pads 9 to 16.
pub const SAMPLER_SLOTS: usize = 16;

/// The sampler volume knob's position when the engine starts.
pub(crate) const DEFAULT_SAMPLER_VOLUME: f32 = 0.8;

/// How a sampler slot plays when its pad is pressed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum SampleMode {
    /// Plays once from the start; pressing again restarts it.
    #[default]
    OneShot,
    /// Plays round and round until stopped.
    Loop,
}

/// A sampler slot's settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SampleSettings {
    /// One-shot or loop. A loop goes round sample-accurately, as a sample cut to loop expects.
    pub mode: SampleMode,
    /// The slot plays only while its pad is held: releasing it stops the sample.
    pub gate: bool,
    /// The slot plays at the tempo master's BPM, which needs the sample's own `bpm`. A synced
    /// loop starts in phase with the master's beats.
    pub sync: bool,
    /// The sample's own tempo in beats per minute, when it has one.
    pub bpm: Option<f64>,
    /// The slot's gain, a linear factor where 1.0 leaves the sample unchanged.
    pub gain: f32,
}

impl Default for SampleSettings {
    fn default() -> Self {
        Self {
            mode: SampleMode::default(),
            gate: false,
            sync: false,
            bpm: None,
            gain: 1.0,
        }
    }
}

/// A sampler slot as the control side sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlotState {
    /// The slot's settings.
    pub settings: SampleSettings,
    /// The slot plays, as the audio thread last published it.
    pub playing: bool,
}

/// The sampler as the control side sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SamplerState {
    /// Each slot, `None` when empty.
    pub slots: [Option<SlotState>; SAMPLER_SLOTS],
    /// The sampler volume knob.
    pub volume: f32,
    /// The sampler's CUE button.
    pub cue: bool,
}

/// One slot on the audio thread.
struct Slot {
    voice: Voice,
    settings: SampleSettings,
    playing: bool,
}

/// The audio-thread side of the sampler: its slots and its own bus, which goes to the master
/// after the channel faders, at the sampler volume.
pub(crate) struct Sampler {
    slots: [Slot; SAMPLER_SLOTS],
    volume: Ramp,
    cue: bool,
    output_rate: f64,
    pub(crate) block: StereoBlock,
}

impl Sampler {
    pub(crate) fn new(output_rate: u32, max_block_frames: usize, declick_frames: u32) -> Self {
        Self {
            slots: std::array::from_fn(|_| Slot {
                voice: Voice::new(declick_frames),
                settings: SampleSettings::default(),
                playing: false,
            }),
            volume: Ramp::new(channel_fader_gain(DEFAULT_SAMPLER_VOLUME), declick_frames),
            cue: false,
            output_rate: f64::from(output_rate.max(1)),
            block: StereoBlock::with_capacity(max_block_frames),
        }
    }

    /// Puts a sample in a slot, or empties it, stopping what it played.
    pub(crate) fn load(
        &mut self,
        slot: usize,
        track: Option<Arc<TrackBuffer>>,
        settings: SampleSettings,
        retire: &mut RetireQueue,
    ) {
        let Some(slot) = self.slots.get_mut(slot) else {
            return;
        };
        slot.voice.start_tail(0.0, retire);
        slot.playing = false;
        if let Some(previous) = std::mem::replace(&mut slot.voice.track, track) {
            retire.track(previous);
        }
        slot.settings = settings;
    }

    pub(crate) fn set_settings(&mut self, slot: usize, settings: SampleSettings) {
        if let Some(slot) = self.slots.get_mut(slot) {
            slot.settings = settings;
        }
    }

    /// Plays a slot, from the start or, for a synced loop, in phase with the master; a slot
    /// already playing starts again with a crossfade.
    pub(crate) fn trigger(
        &mut self,
        index: usize,
        master: Option<MasterBeat>,
        retire: &mut RetireQueue,
    ) {
        let output_rate = self.output_rate;
        let Some(slot) = self.slots.get_mut(index) else {
            return;
        };
        let Some(track) = &slot.voice.track else {
            return;
        };
        let source_rate = f64::from(track.sample_rate());
        let start = match (
            slot.settings.mode,
            slot.settings.sync,
            slot.settings.bpm,
            master,
        ) {
            (SampleMode::Loop, true, Some(bpm), Some(master)) => {
                let multiple = sync_multiple(master.bpm, bpm, true);
                let length_beats = track
                    .total_frames()
                    .map_or(0.0, |frames| frames as f64 / source_rate * bpm / 60.0);
                if length_beats > 0.0 {
                    let beat = (master.beat * multiple).rem_euclid(length_beats);
                    beat * 60.0 / bpm * source_rate
                } else {
                    0.0
                }
            }
            _ => 0.0,
        };
        let step = source_rate / output_rate;
        if slot.playing {
            slot.voice.jump(start, step, retire);
        } else {
            slot.voice.position = start;
            slot.voice.transport.jump_to(0.0);
            slot.voice.transport.set_target(1.0);
            slot.playing = true;
        }
    }

    /// The pad came up: a gate slot stops.
    pub(crate) fn release(&mut self, index: usize, retire: &mut RetireQueue) {
        if self.slots.get(index).is_some_and(|slot| slot.settings.gate) {
            self.stop(index, retire);
        }
    }

    /// Stops a slot with a short fade.
    pub(crate) fn stop(&mut self, index: usize, retire: &mut RetireQueue) {
        if let Some(slot) = self.slots.get_mut(index)
            && slot.playing
        {
            slot.playing = false;
            slot.voice.start_tail(0.0, retire);
        }
    }

    pub(crate) fn set_volume(&mut self, position: f32) {
        self.volume.set_target(channel_fader_gain(position));
    }

    pub(crate) fn set_cue(&mut self, enabled: bool) {
        self.cue = enabled;
    }

    pub(crate) fn is_cued(&self) -> bool {
        self.cue
    }

    /// Which slots play, one bit per slot, and which hold a sample.
    pub(crate) fn playing_and_loaded(&self) -> (u32, u32) {
        self.slots
            .iter()
            .enumerate()
            .fold((0, 0), |(playing, loaded), (index, slot)| {
                (
                    playing | u32::from(slot.playing) << index,
                    loaded | u32::from(slot.voice.track.is_some()) << index,
                )
            })
    }

    /// Renders every slot into the sampler bus at the sampler volume. `master_bpm` is the
    /// tempo synced slots follow.
    pub(crate) fn render(
        &mut self,
        frame_count: usize,
        master_bpm: Option<f64>,
        retire: &mut RetireQueue,
        starved: &mut bool,
    ) {
        self.block.set_frame_count(frame_count);
        self.block.silence();
        let output_rate = self.output_rate;
        let (left_bus, right_bus) = self.block.channels_mut();
        for slot in &mut self.slots {
            let Some(track) = &slot.voice.track else {
                continue;
            };
            if !slot.playing && !slot.voice.has_tails() {
                continue;
            }
            let source_rate = f64::from(track.sample_rate());
            let end = track.total_frames().map(|frames| frames as f64);
            let tempo = match (slot.settings.sync, slot.settings.bpm, master_bpm) {
                (true, Some(bpm), Some(master)) => master * sync_multiple(master, bpm, true) / bpm,
                _ => 1.0,
            };
            let step = tempo * source_rate / output_rate;
            let gain = slot.settings.gain;
            for (left, right) in left_bus.iter_mut().zip(right_bus.iter_mut()) {
                let playhead_gain = slot.voice.transport.next_value() * gain;
                let (slot_left, slot_right) = slot.voice.next_frame(playhead_gain, retire, starved);
                *left += slot_left;
                *right += slot_right;
                if !slot.playing {
                    continue;
                }
                slot.voice.position += step;
                if let Some(end) = end
                    && slot.voice.position >= end
                {
                    match slot.settings.mode {
                        SampleMode::Loop => slot.voice.position -= end,
                        SampleMode::OneShot => {
                            slot.playing = false;
                            slot.voice.transport.jump_to(0.0);
                        }
                    }
                }
            }
        }
        self.volume.apply(left_bus, right_bus);
    }
}
