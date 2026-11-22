//! The sound card: starting the engine on the one the DJ chose, listing the others, and moving
//! to another while the decks are stopped, keeping the tracks, their places, the mixer and the
//! effects as they were.

use dubplate_engine::{
    AudioOutput, Controller, DeckId, EffectUnitId, EngineError, MasterTempo, OutputConfig,
    OutputDevice,
};
use gpui::{Context, SharedString};

use super::notice::NoticePlace;
use super::{BUFFER_FRAMES, DubplateApp, Engine, error_chain};
use crate::controls::{MixerPositions, master_gain_at, tempo_fader, trim_gain_at};

/// Starts the engine on the sound card called `device`, or on the system's default one when
/// none is chosen. Returns the engine, and why the chosen card was not used if it was not.
pub(super) fn start_engine(
    mixer: &MixerPositions,
    device: Option<&str>,
) -> (Result<Engine, SharedString>, Option<String>) {
    match (device, engine_on(mixer, device)) {
        (Some(name), Err(problem)) => (
            engine_on(mixer, None),
            Some(format!(
                "{name} is not available, so Dubplate plays on the default output: {problem}"
            )),
        ),
        (_, engine) => (engine, None),
    }
}

/// The engine on the sound card called `device`, or the default one, with the mixer set.
fn engine_on(mixer: &MixerPositions, device: Option<&str>) -> Result<Engine, SharedString> {
    let config = OutputConfig {
        buffer_frames: Some(BUFFER_FRAMES),
    };
    let started = match device {
        Some(name) => AudioOutput::start_on(name, &config),
        None => AudioOutput::start(&config),
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
/// cue points and grids, the headphone buttons, the effect units, the tempo ranges and sync.
fn carry_over(source: &Controller, target: &mut Controller) -> Result<(), EngineError> {
    for deck in DeckId::ALL {
        let snapshot = source.snapshot_deck(deck)?;
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
    for unit in EffectUnitId::ALL {
        let state = source.effect_unit(unit);
        target.set_effect_kind(unit, state.kind)?;
        target.set_effect_target(unit, state.target)?;
        target.set_effect_beat(unit, state.beat)?;
        target.set_effect_depth(unit, state.depth)?;
        target.set_effect_enabled(unit, state.enabled)?;
    }
    for deck in DeckId::ALL {
        target.set_smart_cfx(deck, source.smart_cfx(deck))?;
    }
    target.set_smart_fader(source.smart_fader())?;
    target.set_quantize(source.quantize())
}

impl DubplateApp {
    /// Reads the sound cards on a background thread, for the settings.
    pub(super) fn refresh_output_devices(&mut self, cx: &mut Context<'_, Self>) {
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

    /// Moves the engine to another sound card and remembers it. The decks must be stopped: a
    /// change of sound card always drops a moment of audio.
    pub(super) fn choose_output(&mut self, name: String, cx: &mut Context<'_, Self>) {
        if self.current_output() == Some(name.as_str()) {
            return;
        }
        if self.is_recording() {
            self.show_notice(
                NoticePlace::Window,
                "Stop the recording to change the sound card.",
                cx,
            );
            return;
        }
        if DeckId::ALL
            .into_iter()
            .any(|deck| self.deck_state(deck).playing)
        {
            self.show_notice(
                NoticePlace::Window,
                "Stop both decks to change the sound card.",
                cx,
            );
            return;
        }
        let mut engine = match engine_on(&self.mixer, Some(&name)) {
            Ok(engine) => engine,
            Err(problem) => {
                self.show_notice(
                    NoticePlace::Window,
                    format!("{name} could not be opened: {problem}"),
                    cx,
                );
                return;
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
        self.restore_sampler(cx);
        self.settings.audio_device = Some(name);
        self.save_settings(cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use dubplate_engine::{
        BeatLength, EffectKind, EffectTarget, Renderer, SyncMode, TempoRange, Track, offline_engine,
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
        source.set_tempo_range(DeckId::B, TempoRange::Wide).unwrap();
        source.set_sync(DeckId::A, SyncMode::Beat).unwrap();
        source.set_key_lock(DeckId::B, true).unwrap();
        let unit = EffectUnitId::ALL[2];
        source.set_effect_kind(unit, EffectKind::Gate).unwrap();
        source
            .set_effect_target(unit, EffectTarget::Deck(DeckId::B))
            .unwrap();
        source.set_effect_beat(unit, BeatLength::HALF).unwrap();
        source.set_effect_depth(unit, 0.8).unwrap();
        render(&mut source_renderer);

        let (mut target, mut target_renderer) = offline_engine(SAMPLE_RATE);
        carry_over(&source, &mut target).unwrap();
        render(&mut target_renderer);

        let deck = target.deck_state(DeckId::B);
        assert!(deck.loaded && !deck.playing);
        assert!((deck.position_seconds - 4.5).abs() < 1e-6, "{deck:?}");
        assert!((deck.cue_point_seconds - 2.0).abs() < 1e-6);
        let headphones = target.headphones();
        assert!(headphones.is_cued(DeckId::B) && headphones.master_cue);
        assert!(!headphones.is_cued(DeckId::A));
        assert_eq!(target.tempo_state(DeckId::B).tempo_range, TempoRange::Wide);
        assert_eq!(target.tempo_state(DeckId::A).sync, SyncMode::Beat);
        assert!(target.key_lock(DeckId::B) && !target.key_lock(DeckId::A));
        let effect = target.effect_unit(unit);
        assert_eq!(effect.kind, EffectKind::Gate);
        assert_eq!(effect.target, EffectTarget::Deck(DeckId::B));
        assert_eq!(effect.beat, BeatLength::HALF);
        assert!((effect.depth - 0.8).abs() < 1e-6);
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
