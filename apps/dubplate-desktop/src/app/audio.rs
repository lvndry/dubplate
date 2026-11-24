//! The sound card: starting the engine on the one the DJ chose, listing the others, and moving
//! to another while the decks and sampler are stopped, keeping the tracks, their places,
//! the sampler assignments, the mixer and the effects as they were.

use dubplate_engine::{
    AudioOutput, Controller, DeckId, EffectUnitId, EngineError, MasterTempo, OutputConfig,
    OutputDevice,
};
use gpui::{Context, SharedString};

use super::notice::NoticePlace;
use super::{DubplateApp, Engine, error_chain};
use crate::controls::{MixerPositions, master_gain_at, tempo_fader, trim_gain_at};
use crate::devices::{BufferSize, DeviceAccess};

/// Starts the engine on the sound card called `device`, or on the system's default one when
/// none is chosen, or on the silent output when the devices are offline. Returns the engine,
/// and why the chosen card was not used if it was not.
pub(super) fn start_engine(
    mixer: &MixerPositions,
    device: Option<&str>,
    buffer: BufferSize,
    devices: &DeviceAccess,
) -> (Result<Engine, SharedString>, Option<String>) {
    let device = device.filter(|_| !devices.is_offline());
    match (device, engine_on(mixer, device, buffer, devices)) {
        (Some(name), Err(problem)) => (
            engine_on(mixer, None, buffer, devices),
            Some(format!(
                "{name} is not available, so Dubplate plays on the default output: {problem}"
            )),
        ),
        (_, engine) => (engine, None),
    }
}

/// The engine on the sound card called `device`, or the default one, or the silent output
/// when the devices are offline, with the mixer set.
fn engine_on(
    mixer: &MixerPositions,
    device: Option<&str>,
    buffer: BufferSize,
    devices: &DeviceAccess,
) -> Result<Engine, SharedString> {
    let config = OutputConfig {
        buffer_frames: Some(buffer.frames()),
    };
    let started = match (devices, device) {
        (DeviceAccess::Offline { .. }, _) => AudioOutput::start_silent(&config),
        (DeviceAccess::Real, Some(name)) => AudioOutput::start_on(name, &config),
        (DeviceAccess::Real, None) => AudioOutput::start(&config),
    };
    let (mut controller, output) =
        started.map_err(|error| SharedString::from(error_chain(&error)))?;
    apply_mixer(&mut controller, mixer).map_err(|error| SharedString::from(error.to_string()))?;
    Ok(Engine { controller, output })
}

/// Sets every fader and knob of the mixer on a fresh engine.
fn apply_mixer(controller: &mut Controller, mixer: &MixerPositions) -> Result<(), EngineError> {
    for deck in DeckId::ALL {
        let index = deck.index();
        controller.set_channel_fader(deck, mixer.channel_faders[index])?;
        controller.set_gain(deck, trim_gain_at(mixer.trims[index]))?;
        for band in dubplate_engine::EqBand::ALL {
            controller.set_eq(deck, band, mixer.eqs[index][band.index()])?;
        }
        controller.set_filter(deck, mixer.filters[index])?;
        controller.set_tempo_fader(deck, tempo_fader(mixer.tempo_sliders[index]))?;
    }
    controller.set_crossfader(mixer.crossfader)?;
    controller.set_master_gain(master_gain_at(mixer.master_level))?;
    controller.set_headphone_mix(mixer.headphone_mix)?;
    controller.set_headphone_level(mixer.headphone_level)?;
    controller.set_sampler_volume(mixer.sampler_volume)
}

/// Puts on `target` what `source` has beyond the mixer: the tracks at their places with their
/// cue points and grids, the sampler, headphone buttons, effect units, tempo ranges and sync.
fn carry_over(source: &Controller, target: &mut Controller) -> Result<(), EngineError> {
    for deck in DeckId::ALL {
        let snapshot = source.last_published_deck(deck);
        target.restore_deck(deck, &snapshot)?;
        let tempo = source.tempo_state(deck);
        if tempo.is_master {
            target.set_master_tempo(MasterTempo::Deck(deck))?;
        }
    }
    let headphones = source.headphones();
    for deck in DeckId::ALL {
        target.set_headphone_cue(deck, headphones.is_cued(deck))?;
    }
    target.set_master_cue(headphones.master_cue)?;
    target.restore_sampler_from(source)?;
    for unit in EffectUnitId::ALL {
        let state = source.effect_unit(unit);
        target.set_effect_kind(unit, state.kind)?;
        target.set_effect_target(unit, state.target)?;
        if let Some(value) = state.parameter {
            target.set_effect_parameter(unit, value)?;
        }
        target.set_effect_depth(unit, state.depth)?;
        target.set_effect_enabled(unit, state.enabled)?;
        target.set_release_fx(unit, state.release)?;
    }
    for deck in DeckId::ALL {
        target.set_smart_cfx(deck, source.smart_cfx(deck))?;
    }
    target.set_smart_fader(source.smart_fader())?;
    target.set_smart_fader_source(source.smart_fader_source())?;
    target.set_fader_echo(source.fader_echo())?;
    target.set_crossfader_enabled(source.crossfader_enabled())?;
    target.set_crossfader_curve(source.crossfader_curve())?;
    target.set_channel_fader_curve(source.channel_fader_curve())?;
    for deck in DeckId::ALL {
        target.set_crossfader_assign(deck, source.crossfader_assign(deck))?;
    }
    target.set_quantize(source.quantize())
}

impl DubplateApp {
    /// Reads the sound cards on a background thread, for the settings. With the devices
    /// offline, the silent output is the only one.
    pub(super) fn refresh_output_devices(&mut self, cx: &mut Context<'_, Self>) {
        if self.devices.is_offline() {
            self.output_devices = self
                .engine
                .as_ref()
                .ok()
                .map(|engine| OutputDevice {
                    name: engine.output.device_name().to_owned(),
                    channel_count: engine.output.channel_count(),
                    sample_rate: engine.output.sample_rate(),
                    is_default: true,
                })
                .into_iter()
                .collect();
            cx.notify();
            return;
        }
        cx.spawn(async move |this, cx| {
            let devices = cx
                .background_executor()
                .spawn(async move { AudioOutput::output_devices() })
                .await;
            this.update(cx, |this, cx| {
                match devices {
                    Ok(devices) => this.output_devices = devices,
                    Err(error) => this.show_notice(
                        NoticePlace::Window,
                        format!(
                            "The sound cards could not be listed: {}",
                            error_chain(&error)
                        ),
                        cx,
                    ),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The sound cards last listed.
    pub(super) fn output_devices(&self) -> &[OutputDevice] {
        &self.output_devices
    }

    /// The sound card playing now.
    pub(super) fn current_output(&self) -> Option<&str> {
        self.engine
            .as_ref()
            .ok()
            .map(|engine| engine.output.device_name())
    }

    /// Moves the engine to another sound card and remembers it. Playback must be stopped: a
    /// change of sound card always drops a moment of audio.
    pub(super) fn choose_output(&mut self, name: String, cx: &mut Context<'_, Self>) {
        if self.current_output() == Some(name.as_str()) {
            return;
        }
        if self.restart_engine(Some(&name), self.buffer_size(), "the sound card", cx) {
            self.settings.audio_device = Some(name);
            self.save_settings(cx);
        }
    }

    /// The buffer the sound card is fed in.
    pub(super) fn buffer_size(&self) -> BufferSize {
        BufferSize::from_setting(self.settings.buffer_frames)
    }

    /// Starts the sound card again with a buffer of `buffer` and remembers it. Playback must
    /// be stopped, as for a change of sound card.
    pub(super) fn choose_buffer_size(&mut self, buffer: BufferSize, cx: &mut Context<'_, Self>) {
        if buffer == self.buffer_size() {
            return;
        }
        let device = self.current_output().map(str::to_owned);
        if self.restart_engine(device.as_deref(), buffer, "the buffer size", cx) {
            self.settings.buffer_frames = Some(buffer.frames());
            self.save_settings(cx);
        }
    }

    /// The dropouts since launch: buffers the sound card asked for that were not ready in
    /// time.
    pub(super) fn dropouts(&self) -> u64 {
        self.engine
            .as_ref()
            .map_or(0, |engine| engine.controller.stats().overruns())
    }

    /// Replaces the engine with one on `device` with `buffer`, carrying the decks, sampler,
    /// mixer and effects over. Refused while recording or playing, since the change drops a moment of
    /// audio; `change` names what the DJ is changing for the notice. Returns whether it
    /// happened.
    fn restart_engine(
        &mut self,
        device: Option<&str>,
        buffer: BufferSize,
        change: &str,
        cx: &mut Context<'_, Self>,
    ) -> bool {
        if self.is_recording() {
            self.show_notice(
                NoticePlace::Window,
                format!("Stop the recording to change {change}."),
                cx,
            );
            return false;
        }
        if DeckId::ALL
            .into_iter()
            .any(|deck| self.deck_state(deck).playing)
        {
            self.show_notice(
                NoticePlace::Window,
                format!("Stop both decks to change {change}."),
                cx,
            );
            return false;
        }
        if self
            .engine
            .as_ref()
            .is_ok_and(|engine| engine.controller.sampler_is_playing())
        {
            self.show_notice(
                NoticePlace::Window,
                format!("Stop the sampler to change {change}."),
                cx,
            );
            return false;
        }
        let needs_sampler_restore = self.engine.is_err();
        if needs_sampler_restore && self.sample_loads_pending() {
            self.show_notice(
                NoticePlace::Window,
                format!("Wait for samples to finish loading to change {change}."),
                cx,
            );
            return false;
        }
        let name = device.unwrap_or("the default output").to_owned();
        let mut engine = match engine_on(&self.mixer, device, buffer, &self.devices) {
            Ok(engine) => engine,
            Err(problem) => {
                self.show_notice(
                    NoticePlace::Window,
                    format!("{name} could not be opened: {problem}"),
                    cx,
                );
                return false;
            }
        };
        if let Ok(previous) = &self.engine
            && let Err(error) = carry_over(&previous.controller, &mut engine.controller)
        {
            self.show_notice(
                NoticePlace::Window,
                format!("Some settings did not follow to {name}: {error}"),
                cx,
            );
        }
        self.engine = Ok(engine);
        self.held_sample_slots = Default::default();
        if needs_sampler_restore {
            self.restore_sampler(cx);
        }
        cx.notify();
        true
    }
}

#[cfg(test)]
mod tests {
    use dubplate_engine::{
        BeatLength, ChannelFaderCurve, CrossfaderAssign, CrossfaderCurve, EffectKind, EffectTarget,
        ParameterValue, Renderer, SAMPLER_SLOTS, SampleMode, SampleSettings, SyncMode, TempoRange,
        Track, offline_engine,
    };

    use super::*;

    const SAMPLE_RATE: u32 = 48_000;

    fn render(renderer: &mut Renderer) {
        let mut block = vec![0.0; 480 * 2];
        renderer.render(&mut block, 2);
    }

    #[test]
    fn a_new_sound_card_takes_the_tracks_places_headphones_effects_and_tempo() {
        let (mut source, mut source_renderer) = offline_engine(SAMPLE_RATE);
        let samples = vec![0.1; SAMPLE_RATE as usize * 10 * 2];
        let track = Track::from_interleaved_stereo(&samples, SAMPLE_RATE).unwrap();
        source.load_track(DeckId::B, track).unwrap();
        source.set_cue_point(DeckId::B, 2.0).unwrap();
        source.seek(DeckId::B, 4.5).unwrap();
        source.set_headphone_cue(DeckId::B, true).unwrap();
        source.set_master_cue(true).unwrap();
        let sample_slot = SAMPLER_SLOTS - 1;
        let sample = Track::from_interleaved_stereo(&samples, SAMPLE_RATE).unwrap();
        let sample_settings = SampleSettings {
            mode: SampleMode::Loop,
            gate: true,
            bpm: Some(120.0),
            ..SampleSettings::default()
        };
        source
            .load_sample(sample_slot, sample, sample_settings)
            .unwrap();
        source.set_sampler_cue(true).unwrap();
        source.set_tempo_range(DeckId::B, TempoRange::Wide).unwrap();
        source.set_sync(DeckId::A, SyncMode::Beat).unwrap();
        source.set_key_lock(DeckId::B, true).unwrap();
        let unit = EffectUnitId::ALL[2];
        source.set_effect_kind(unit, EffectKind::Trans).unwrap();
        source
            .set_effect_target(unit, EffectTarget::Deck(DeckId::B))
            .unwrap();
        source
            .set_effect_parameter(unit, ParameterValue::Beats(BeatLength::HALF))
            .unwrap();
        source.set_effect_depth(unit, 0.8).unwrap();
        source
            .set_crossfader_assign(DeckId::A, CrossfaderAssign::Thru)
            .unwrap();
        source
            .set_crossfader_curve(CrossfaderCurve::Scratch)
            .unwrap();
        source
            .set_channel_fader_curve(ChannelFaderCurve::Fast)
            .unwrap();
        render(&mut source_renderer);

        let (mut target, mut target_renderer) = offline_engine(SAMPLE_RATE);
        carry_over(&source, &mut target).unwrap();
        render(&mut target_renderer);

        assert_eq!(target.crossfader_assign(DeckId::A), CrossfaderAssign::Thru);
        assert_eq!(target.crossfader_assign(DeckId::B), CrossfaderAssign::B);
        assert_eq!(target.crossfader_curve(), CrossfaderCurve::Scratch);
        assert_eq!(target.channel_fader_curve(), ChannelFaderCurve::Fast);
        let deck = target.deck_state(DeckId::B);
        assert!(deck.loaded && !deck.playing);
        assert!((deck.position_seconds - 4.5).abs() < 1e-6, "{deck:?}");
        assert!((deck.cue_point_seconds - 2.0).abs() < 1e-6);
        let headphones = target.headphones();
        assert!(headphones.is_cued(DeckId::B) && headphones.master_cue);
        assert!(!headphones.is_cued(DeckId::A));
        let sampler = target.sampler_state();
        assert!(sampler.cue);
        let sample = sampler.slots[sample_slot].unwrap();
        assert_eq!(sample.settings, sample_settings);
        assert!(!sample.playing);
        target.trigger_sample(sample_slot).unwrap();
        render(&mut target_renderer);
        assert!(target.sampler_state().slots[sample_slot].unwrap().playing);
        assert_eq!(target.tempo_state(DeckId::B).tempo_range, TempoRange::Wide);
        assert_eq!(target.tempo_state(DeckId::A).sync, SyncMode::Beat);
        assert!(target.key_lock(DeckId::B) && !target.key_lock(DeckId::A));
        let effect = target.effect_unit(unit);
        assert_eq!(effect.kind, EffectKind::Trans);
        assert_eq!(effect.target, EffectTarget::Deck(DeckId::B));
        assert_eq!(
            effect.parameter,
            Some(ParameterValue::Beats(BeatLength::HALF))
        );
        assert!((effect.depth - 0.8).abs() < 1e-6);
    }

    #[test]
    fn a_sound_card_that_stopped_still_hands_over_what_it_last_played() {
        let (mut source, mut source_renderer) = offline_engine(SAMPLE_RATE);
        let samples = vec![0.1; SAMPLE_RATE as usize * 10 * 2];
        let track = Track::from_interleaved_stereo(&samples, SAMPLE_RATE).unwrap();
        source.load_track(DeckId::A, track).unwrap();
        source.seek(DeckId::A, 4.0).unwrap();
        render(&mut source_renderer);
        drop(source_renderer);
        source.set_headphone_cue(DeckId::A, true).unwrap();

        let (mut target, mut target_renderer) = offline_engine(SAMPLE_RATE);
        carry_over(&source, &mut target).unwrap();
        render(&mut target_renderer);
        let state = target.deck_state(DeckId::A);
        assert!(
            state.loaded,
            "deck A is carried over though the old stream died"
        );
        assert!((state.position_seconds - 4.0).abs() < 1e-3, "{state:?}");
    }

    #[test]
    fn the_mixer_is_set_on_a_fresh_engine() {
        let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
        let mixer = MixerPositions {
            headphone_mix: 0.9,
            headphone_level: 0.2,
            ..MixerPositions::default()
        };
        apply_mixer(&mut controller, &mixer).unwrap();
        render(&mut renderer);
        let headphones = controller.headphones();
        assert!((headphones.mix - 0.9).abs() < 1e-6);
        assert!((headphones.level - 0.2).abs() < 1e-6);
    }
}
