//! The window: owns the audio engine, the two decks, the mixer positions and the browser, turns
//! clicks and shortcuts into engine commands, and redraws at the screen's frame rate while
//! anything moves.

mod deck_panel;
mod drop_target;
mod header;
mod help;
mod lanes;
mod mixer;
mod slider;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dubplate_engine::{
    AudioOutput, Controller, DECK_COUNT, DeckId, DeckState, EngineError, OutputConfig,
};
use dubplate_midi::{Feedback, PadMode};
use dubplate_rekordbox::HotCuePad;
use gpui::prelude::*;
use gpui::{
    Bounds, Context, Entity, FocusHandle, MouseButton, MouseMoveEvent, MouseUpEvent, Pixels, Point,
    SharedString, StyleRefinement, Subscription, Window, div, px,
};

use crate::browser::{Browser, BrowserEvent, LibraryStatus};
use crate::collection::{Collection, CollectionTrack};
use crate::commands::{Command, Press};
use crate::controls::{
    CROSSFADER_STEP, Control, MixerPositions, Slider, fraction_at, master_gain_at, nudged_tempo,
    tempo_at, tempo_fraction, trim_gain_at,
};
use crate::deck::{
    Deck, DeckContent, HotCues, LoadError, PreparedTrack, TrackVisuals, pad_position, prepare_track,
};
use crate::fonts::UI_FAMILY;
use crate::frame_stats::FrameStats;
use crate::hardware::{
    DeckLights, Hardware, Lights, LinkEvent, PAD_COUNT, Surface, controller_deck, meter_level,
};
use crate::loading::{LoadRefusal, LoadSource, check_deck_free, check_file};
use crate::meter::PeakMeter;
use crate::settings::Settings;
use crate::shortcuts::{Perform, WINDOW_CONTEXT};
use crate::theme;
use crate::transport::{self, CueDown, is_at_cue, other_deck};
use crate::waveform::Zoom;

/// Frames per device buffer: about 5 ms at 48 kHz.
const BUFFER_FRAMES: u32 = 256;
/// How long a notice stays on screen.
const NOTICE_DURATION: Duration = Duration::from_secs(6);
/// How often the controller lights follow the decks while nothing else updates them, for the
/// blinking lights and the level meters.
const LIGHTS_INTERVAL: Duration = Duration::from_millis(30);
/// How long after a command the controller lights look again, once the audio thread has
/// carried it out: a few audio buffers.
const LIGHTS_SETTLE: Duration = Duration::from_millis(12);

/// The sound card and the engine playing on it.
struct Engine {
    controller: Controller,
    output: AudioOutput,
}

/// Where a notice shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NoticePlace {
    /// At the bottom of the window.
    Window,
    /// On a deck.
    Deck(DeckId),
}

/// A short message about something the DJ asked for that did not happen.
struct Notice {
    id: u64,
    place: NoticePlace,
    text: SharedString,
}

/// Where the sliders and overviews were last drawn, so a pointer can be turned into a value.
#[derive(Default)]
struct HitAreas {
    sliders: [Cell<Option<Bounds<Pixels>>>; Slider::ALL.len()],
    overviews: [Cell<Option<Bounds<Pixels>>>; DECK_COUNT],
}

/// What one deck shows in this frame.
#[derive(Debug, Clone, Copy)]
struct DeckFrame {
    state: DeckState,
    playhead_seconds: f64,
    duration_seconds: Option<f64>,
}

/// The root view.
pub struct DubplateApp {
    engine: Result<Engine, SharedString>,
    library: LibraryStatus,
    decks: [Deck; DECK_COUNT],
    deck_meters: [PeakMeter; DECK_COUNT],
    master_meter: PeakMeter,
    mixer: MixerPositions,
    dragging: Option<Slider>,
    header_drag_armed: bool,
    lock_playing_deck: bool,
    settings: Settings,
    settings_path: Option<PathBuf>,
    hardware: Option<Hardware>,
    edited_hot_cues: HashMap<String, HotCues>,
    lights_settling: bool,
    hit_areas: Rc<HitAreas>,
    browser: Entity<Browser>,
    help_open: bool,
    notice: Option<Notice>,
    notice_count: u64,
    focus_handle: FocusHandle,
    last_frame: Instant,
    frame_stats: Option<Rc<RefCell<FrameStats>>>,
    _subscriptions: Vec<Subscription>,
}

impl DubplateApp {
    /// Starts the audio engine, opens the browser and reads the rekordbox library in the
    /// background.
    pub fn new(window: &mut Window, cx: &mut Context<'_, Self>) -> Self {
        let browser = cx.new(|cx| Browser::new(window, cx));
        let subscription =
            cx.subscribe(&browser, |this, _, event: &BrowserEvent, cx| match *event {
                BrowserEvent::Load { deck, track } => {
                    this.load(deck, LoadSource::Collection(track), cx);
                }
                BrowserEvent::LoadOnFreeDeck(track) => match this.free_deck() {
                    Some(deck) => this.load(deck, LoadSource::Collection(track), cx),
                    None => this.show_notice(NoticePlace::Window, "Both decks are playing.", cx),
                },
            });
        let mixer = MixerPositions::default();
        let engine = start_engine(&mixer);
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        let mut problems: Vec<String> = Vec::new();
        let settings_path = Settings::default_path()
            .map_err(|error| problems.push(format!("Settings are not saved: {error}")))
            .ok();
        let settings = settings_path
            .as_deref()
            .map(|path| {
                Settings::load(path).unwrap_or_else(|error| {
                    problems.push(format!("Settings were reset: {error}"));
                    Settings::default()
                })
            })
            .unwrap_or_default();
        let hardware = match Hardware::start() {
            Ok((hardware, events)) => {
                listen_to_controllers(events, window, cx);
                Some(hardware)
            }
            Err(error) => {
                problems.push(format!("Controllers cannot be used: {error}"));
                None
            }
        };
        keep_lights_moving(cx);
        let mut app = Self {
            engine,
            library: LibraryStatus::Reading,
            decks: Default::default(),
            deck_meters: [PeakMeter::default(); DECK_COUNT],
            master_meter: PeakMeter::default(),
            mixer,
            dragging: None,
            header_drag_armed: false,
            lock_playing_deck: settings.lock_playing_deck,
            settings,
            settings_path,
            hardware,
            edited_hot_cues: HashMap::new(),
            lights_settling: false,
            hit_areas: Rc::default(),
            browser,
            help_open: false,
            notice: None,
            notice_count: 0,
            focus_handle,
            last_frame: Instant::now(),
            frame_stats: FrameStats::from_environment().map(|stats| Rc::new(RefCell::new(stats))),
            _subscriptions: vec![subscription],
        };
        app.read_library(cx);
        if let Some(problem) = problems.into_iter().next() {
            app.show_notice(NoticePlace::Window, problem, cx);
        }
        app
    }

    /// Turns Lock playing deck on or off and remembers it.
    pub(super) fn toggle_lock_playing_deck(&mut self, cx: &mut Context<'_, Self>) {
        self.lock_playing_deck = !self.lock_playing_deck;
        self.settings.lock_playing_deck = self.lock_playing_deck;
        self.save_settings(cx);
        cx.notify();
    }

    /// Writes the settings on a background thread.
    fn save_settings(&self, cx: &mut Context<'_, Self>) {
        let Some(path) = self.settings_path.clone() else {
            return;
        };
        let settings = self.settings.clone();
        cx.spawn(async move |this, cx| {
            let saved = cx
                .background_executor()
                .spawn(async move { settings.save(&path) })
                .await;
            if let Err(error) = saved {
                this.update(cx, |this, cx| {
                    this.show_notice(
                        NoticePlace::Window,
                        format!("Settings could not be saved: {error}"),
                        cx,
                    );
                })
                .ok();
            }
        })
        .detach();
    }

    fn read_library(&self, cx: &mut Context<'_, Self>) {
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { Collection::load_default() })
                .await;
            this.update(cx, |this, cx| {
                this.library = match result {
                    Ok(collection) => LibraryStatus::Ready(Arc::new(collection)),
                    Err(error) => LibraryStatus::Failed(error_chain(&error).into()),
                };
                let library = this.library.clone();
                this.browser
                    .update(cx, |browser, cx| browser.set_library(library, cx));
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn collection(&self) -> Option<&Arc<Collection>> {
        match &self.library {
            LibraryStatus::Ready(collection) => Some(collection),
            LibraryStatus::Reading | LibraryStatus::Failed(_) => None,
        }
    }

    fn deck(&self, deck: DeckId) -> &Deck {
        &self.decks[deck.index()]
    }

    fn deck_state(&self, deck: DeckId) -> DeckState {
        match &self.engine {
            Ok(engine) => engine.controller.deck_state(deck),
            Err(_) => DeckState {
                loaded: false,
                playing: false,
                position_seconds: 0.0,
                cue_point_seconds: 0.0,
                tempo: 1.0,
            },
        }
    }

    /// An empty deck, else a stopped one, A before B.
    fn free_deck(&self) -> Option<DeckId> {
        let empty = DeckId::ALL
            .into_iter()
            .find(|deck| self.deck(*deck).track.is_none());
        empty.or_else(|| {
            DeckId::ALL
                .into_iter()
                .find(|deck| !self.deck_state(*deck).playing)
        })
    }

    /// The one way a track reaches a deck, whatever asked for it: drag and drop, double-click,
    /// the Load buttons or the keys.
    fn load(&mut self, deck: DeckId, source: LoadSource, cx: &mut Context<'_, Self>) {
        let (track, generation) = match self.begin_load(deck, source) {
            Ok(started) => started,
            Err(refusal) => {
                self.show_notice(NoticePlace::Deck(deck), refusal.to_string(), cx);
                return;
            }
        };
        cx.notify();
        let share_directory = self
            .collection()
            .map(|collection| collection.share_directory().to_path_buf());
        cx.spawn(async move |this, cx| {
            let prepared = cx
                .background_executor()
                .spawn(
                    async move { prepare_track(&track, share_directory.as_deref(), Zoom::DEFAULT) },
                )
                .await;
            this.update(cx, |this, cx| {
                this.finish_load(deck, generation, prepared, cx)
            })
            .ok();
        })
        .detach();
    }

    fn begin_load(
        &mut self,
        deck: DeckId,
        source: LoadSource,
    ) -> Result<(Arc<CollectionTrack>, u64), LoadRefusal> {
        let track = match source {
            LoadSource::Collection(index) => Arc::clone(
                self.collection()
                    .ok_or(LoadRefusal::LibraryNotReady)?
                    .track(index),
            ),
            LoadSource::File(path) => {
                check_file(&path)?;
                Arc::new(CollectionTrack::from_file(&path))
            }
        };
        check_deck_free(deck, self.deck_state(deck).playing, self.lock_playing_deck)?;
        let hot_cues = self
            .edited_hot_cues
            .get(&track.identity())
            .copied()
            .unwrap_or_else(|| HotCues::of(&track));
        let generation = self.decks[deck.index()].begin_load(Arc::clone(&track), hot_cues);
        Ok((track, generation))
    }

    fn finish_load(
        &mut self,
        deck: DeckId,
        generation: u64,
        prepared: Result<PreparedTrack, LoadError>,
        cx: &mut Context<'_, Self>,
    ) {
        if !self.deck(deck).is_current_load(generation) {
            return;
        }
        let content = match prepared.and_then(|prepared| self.put_on_deck(deck, prepared)) {
            Ok(visuals) => {
                self.signal_controllers(Feedback::TrackLoaded(controller_deck(deck)));
                DeckContent::Loaded(visuals)
            }
            Err(error) => DeckContent::Failed(error_chain(&error)),
        };
        self.decks[deck.index()].content = content;
        self.update_lights();
        cx.notify();
    }

    fn put_on_deck(
        &mut self,
        deck: DeckId,
        mut prepared: PreparedTrack,
    ) -> Result<TrackVisuals, LoadError> {
        let engine = self
            .engine
            .as_mut()
            .map_err(|problem| LoadError::Audio(EngineError::Output(problem.to_string())))?;
        let visuals = TrackVisuals::new(&mut prepared);
        engine.controller.load_track(deck, prepared.audio)?;
        Ok(visuals)
    }

    /// Carries out a command from the keyboard, the mouse or a controller.
    pub(super) fn perform(
        &mut self,
        command: Command,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        match command {
            Command::TogglePlay(deck) => {
                if std::mem::take(&mut self.decks[deck.index()].cue_preview) {
                    return;
                }
                self.engine_command(cx, |controller| controller.toggle_play(deck));
            }
            Command::Cue(deck, press) => {
                if matches!(press, Press::Down | Press::Tap) {
                    self.cue_down(deck, cx);
                }
                if matches!(press, Press::Up | Press::Tap) {
                    self.cue_up(deck, cx);
                }
            }
            Command::JumpToStart(deck) => {
                if self.deck(deck).visuals().is_some() {
                    self.jump(deck, 0.0, cx);
                }
            }
            Command::HotCue(deck, pad, press) => {
                if matches!(press, Press::Down | Press::Tap) {
                    self.press_hot_cue(deck, pad, cx);
                }
            }
            Command::DeleteHotCue(deck, pad) => self.edit_hot_cues(deck, |hot_cues| {
                hot_cues.delete(pad);
            }),
            Command::LoadSelected(deck) => {
                if let Some(track) = self.browser.read(cx).selected() {
                    self.load(deck, LoadSource::Collection(track), cx);
                }
            }
            Command::InstantDouble(deck) => self.instant_double(deck, cx),
            Command::NudgeTempo(deck, direction) => {
                let tempo = nudged_tempo(self.mixer.tempos[deck.index()], direction.sign());
                self.set_control(Control::Tempo(deck), tempo_fraction(tempo), cx);
            }
            Command::SetControl(control, fraction) => self.set_control(control, fraction, cx),
            Command::MoveCrossfader(direction) => {
                let position = self.mixer.crossfader + CROSSFADER_STEP * direction.sign() as f32;
                self.set_control(Control::Crossfader, position.clamp(0.0, 1.0), cx);
            }
            Command::MoveSelection(direction) => self
                .browser
                .update(cx, |browser, cx| browser.move_selection(direction, cx)),
            Command::Browse(steps) => self
                .browser
                .update(cx, |browser, cx| browser.browse(steps, cx)),
            Command::SwitchBrowserPane => self
                .browser
                .update(cx, |browser, cx| browser.switch_pane(cx)),
            Command::FocusSearch => self
                .browser
                .update(cx, |browser, cx| browser.focus_search(window, cx)),
            Command::ToggleHelp => self.help_open = !self.help_open,
            Command::Dismiss => {
                if self.help_open {
                    self.help_open = false;
                } else if self.browser.read(cx).search_has_focus(window, cx) {
                    self.browser
                        .update(cx, |browser, cx| browser.focus_table(window, cx));
                }
            }
        }
        self.update_lights();
        self.settle_lights(cx);
        cx.notify();
    }

    /// Looks at the decks again for the lights once the audio thread has carried out what was
    /// just asked, so PLAY and CUE change with the music rather than on the next refresh.
    fn settle_lights(&mut self, cx: &mut Context<'_, Self>) {
        if self.lights_settling || !self.hardware.as_ref().is_some_and(Hardware::is_connected) {
            return;
        }
        self.lights_settling = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(LIGHTS_SETTLE).await;
            this.update(cx, |this, _| {
                this.lights_settling = false;
                this.update_lights();
            })
            .ok();
        })
        .detach();
    }

    /// CUE going down, as on a CDJ: a playing deck goes back to its cue point and stops; a
    /// stopped deck at its cue point plays while CUE is held; a stopped deck elsewhere takes
    /// the beat nearest its position as the new cue point.
    fn cue_down(&mut self, deck: DeckId, cx: &mut Context<'_, Self>) {
        let Some(visuals) = self.deck(deck).visuals() else {
            return;
        };
        let beats = Arc::clone(&visuals.beats);
        let mut outcome = None;
        self.engine_command(cx, |controller| {
            outcome = Some(transport::cue_down(controller, deck, |seconds| {
                beats.nearest(seconds).unwrap_or(seconds)
            })?);
            Ok(())
        });
        self.decks[deck.index()].cue_preview = outcome == Some(CueDown::Preview);
    }

    /// CUE coming up: a deck playing because CUE was held goes back to the cue point.
    fn cue_up(&mut self, deck: DeckId, cx: &mut Context<'_, Self>) {
        if std::mem::take(&mut self.decks[deck.index()].cue_preview) {
            self.engine_command(cx, |controller| transport::end_preview(controller, deck));
        }
    }

    /// A hot cue pad, as in rekordbox: a set pad plays from its cue, whether the deck was
    /// playing or not; an empty pad takes the beat nearest to what the DJ hears now.
    fn press_hot_cue(&mut self, deck: DeckId, pad: HotCuePad, cx: &mut Context<'_, Self>) {
        if self.deck(deck).visuals().is_none() {
            return;
        }
        match self.deck(deck).hot_cues.get(pad) {
            Some(seconds) => {
                self.decks[deck.index()].cue_preview = false;
                self.engine_command(cx, |controller| {
                    transport::call_hot_cue(controller, deck, seconds)
                });
            }
            None => {
                let seconds = self.quantized(deck, self.heard_seconds(deck));
                self.edit_hot_cues(deck, |hot_cues| hot_cues.set(pad, seconds));
            }
        }
    }

    /// Changes the hot cues of the track on `deck`, and keeps the change for the next time the
    /// track is loaded in this session.
    fn edit_hot_cues(&mut self, deck: DeckId, edit: impl FnOnce(&mut HotCues)) {
        let side = &mut self.decks[deck.index()];
        let Some(track) = side.track.as_ref() else {
            return;
        };
        if side.visuals().is_none() {
            return;
        }
        edit(&mut side.hot_cues);
        self.edited_hot_cues.insert(track.identity(), side.hot_cues);
    }

    /// Where the music a deck plays reaches the speakers now: the engine's position less what
    /// is still on its way through the sound card.
    fn heard_seconds(&self, deck: DeckId) -> f64 {
        let state = self.deck_state(deck);
        let Ok(engine) = &self.engine else {
            return state.position_seconds;
        };
        if !state.playing {
            return state.position_seconds;
        }
        let stats = engine.controller.stats();
        let latency_seconds = stats.output_latency_ms.unwrap_or(stats.buffer_latency_ms) / 1000.0;
        (state.position_seconds - latency_seconds * state.tempo).max(0.0)
    }

    /// `seconds` on the beat nearest to it, when the track has a beat grid, as with rekordbox's
    /// quantize on.
    fn quantized(&self, deck: DeckId, seconds: f64) -> f64 {
        self.deck(deck)
            .visuals()
            .and_then(|visuals| visuals.beats.nearest(seconds))
            .unwrap_or(seconds)
    }

    /// Loads the track playing on the other deck at the same place, tempo and hot cues, as
    /// rekordbox's instant doubles.
    fn instant_double(&mut self, deck: DeckId, cx: &mut Context<'_, Self>) {
        let source = other_deck(deck);
        let (Some(track), Some(visuals)) = (
            self.deck(source).track.clone(),
            self.deck(source).visuals().cloned(),
        ) else {
            return;
        };
        if let Err(refusal) =
            check_deck_free(deck, self.deck_state(deck).playing, self.lock_playing_deck)
        {
            self.show_notice(NoticePlace::Deck(deck), refusal.to_string(), cx);
            return;
        }
        let hot_cues = self.deck(source).hot_cues;
        let mut doubled = false;
        self.engine_command(cx, |controller| {
            doubled = transport::double(controller, source, deck)?;
            Ok(())
        });
        if !doubled {
            return;
        }
        let side = &mut self.decks[deck.index()];
        side.begin_load(track, hot_cues);
        side.content = DeckContent::Loaded(visuals);
        self.set_control(
            Control::Tempo(deck),
            tempo_fraction(self.mixer.tempos[source.index()]),
            cx,
        );
        self.signal_controllers(Feedback::TrackLoaded(controller_deck(deck)));
    }

    fn seek_to_fraction(&mut self, deck: DeckId, fraction: f32, cx: &mut Context<'_, Self>) {
        let Some(duration) = self.duration_seconds(deck) else {
            return;
        };
        let seconds = duration * f64::from(fraction);
        self.jump(deck, seconds, cx);
    }

    /// Moves a deck to `seconds`. A stopped deck also takes it as its cue point, as a CDJ does.
    fn jump(&mut self, deck: DeckId, seconds: f64, cx: &mut Context<'_, Self>) {
        self.engine_command(cx, |controller| transport::jump(controller, deck, seconds));
    }

    fn engine_command(
        &mut self,
        cx: &mut Context<'_, Self>,
        command: impl FnOnce(&mut Controller) -> Result<(), EngineError>,
    ) {
        let result = match &mut self.engine {
            Ok(engine) => command(&mut engine.controller).map_err(|error| error.to_string()),
            Err(problem) => Err(problem.to_string()),
        };
        if let Err(problem) = result {
            self.show_notice(NoticePlace::Window, problem, cx);
        }
    }

    fn show_notice(
        &mut self,
        place: NoticePlace,
        text: impl Into<SharedString>,
        cx: &mut Context<'_, Self>,
    ) {
        self.notice_count += 1;
        let id = self.notice_count;
        self.notice = Some(Notice {
            id,
            place,
            text: text.into(),
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(NOTICE_DURATION).await;
            this.update(cx, |this, cx| {
                if this.notice.as_ref().is_some_and(|notice| notice.id == id) {
                    this.notice = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Where a fader or knob is, from 0.0 to 1.0.
    pub(super) fn control_fraction(&self, control: Control) -> f32 {
        self.mixer.fraction(control)
    }

    fn set_control(&mut self, control: Control, fraction: f32, cx: &mut Context<'_, Self>) {
        let fraction = fraction.clamp(0.0, 1.0);
        match control {
            Control::ChannelFader(deck) => {
                self.mixer.channel_faders[deck.index()] = fraction;
                self.engine_command(cx, |controller| {
                    controller.set_channel_fader(deck, fraction)
                });
            }
            Control::Crossfader => {
                self.mixer.crossfader = fraction;
                self.engine_command(cx, |controller| controller.set_crossfader(fraction));
            }
            Control::Tempo(deck) => {
                let tempo = tempo_at(fraction);
                self.mixer.tempos[deck.index()] = tempo;
                self.engine_command(cx, |controller| controller.set_tempo(deck, tempo));
            }
            Control::MasterLevel => {
                self.mixer.master_level = fraction;
                self.engine_command(cx, |controller| {
                    controller.set_master_gain(master_gain_at(fraction))
                });
            }
            Control::Trim(deck) => {
                self.mixer.trims[deck.index()] = fraction;
                self.engine_command(cx, |controller| {
                    controller.set_gain(deck, trim_gain_at(fraction))
                });
            }
        }
        cx.notify();
    }

    /// Carries out what the controller thread reports, in arrival order.
    fn handle_link_events(
        &mut self,
        events: Vec<LinkEvent>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        for event in events {
            if let Some(hardware) = self.hardware.as_mut() {
                hardware.note(&event);
            }
            match event {
                LinkEvent::Connected { name, .. } => {
                    self.show_notice(NoticePlace::Window, format!("{name} connected"), cx);
                }
                LinkEvent::Disconnected { name, .. } => {
                    self.show_notice(NoticePlace::Window, format!("{name} disconnected"), cx);
                }
                LinkEvent::Problem(problem) => {
                    self.show_notice(NoticePlace::Window, problem, cx);
                }
                LinkEvent::Control { control, received } => {
                    let surface = Surface {
                        positions: self.mixer,
                        playing: DeckId::ALL.map(|deck| self.deck_state(deck).playing),
                    };
                    let command = self.hardware.as_mut().and_then(|hardware| {
                        hardware
                            .dispatcher
                            .dispatch(control, Instant::now(), &surface)
                    });
                    if let Some(command) = command {
                        self.perform(command, window, cx);
                    }
                    if let Some(stats) = &self.frame_stats {
                        stats.borrow_mut().record_control(received.elapsed());
                    }
                }
            }
        }
        self.update_lights();
        cx.notify();
    }

    /// Shows the decks on the controllers' lights.
    fn update_lights(&mut self) {
        let Some(hardware) = self.hardware.as_ref() else {
            return;
        };
        if !hardware.is_connected() {
            return;
        }
        let blink = hardware.blink_on(Instant::now());
        let lights = Lights {
            decks: DeckId::ALL.map(|deck| self.deck_lights(deck, blink)),
            beat_fx_on: false,
        };
        if let Some(hardware) = self.hardware.as_mut() {
            hardware.show(lights);
        }
    }

    /// A deck's lights, as on a CDJ: PLAY lit while playing and blinking while paused, CUE lit
    /// at the cue point and blinking where a new cue point can be set.
    fn deck_lights(&self, deck: DeckId, blink: bool) -> DeckLights {
        let side = self.deck(deck);
        let loaded = side.visuals().is_some();
        let state = self.deck_state(deck);
        let at_cue = is_at_cue(&state);
        let play = state.playing || (loaded && blink);
        let cue = if state.playing {
            side.cue_preview
        } else {
            loaded && (at_cue || blink)
        };
        let mut pads = [false; PAD_COUNT];
        if loaded {
            for (pad, _) in side.hot_cues.iter() {
                if let Some(lit) = pads.get_mut(pad_position(pad)) {
                    *lit = true;
                }
            }
        }
        DeckLights {
            play,
            cue,
            pad_mode: self.hardware.as_ref().map_or(PadMode::HotCue, |hardware| {
                hardware.dispatcher.pad_mode(deck)
            }),
            pads,
            level: meter_level(self.deck_meters[deck.index()].decibels()),
        }
    }

    fn signal_controllers(&self, feedback: Feedback) {
        if let Some(hardware) = &self.hardware {
            hardware.signal(feedback);
        }
    }

    /// The controllers connected, for the header.
    pub(super) fn controller_names(&self) -> Vec<&str> {
        self.hardware
            .as_ref()
            .map(|hardware| hardware.connected_names().collect())
            .unwrap_or_default()
    }

    fn begin_drag(
        &mut self,
        slider: Slider,
        position: Point<Pixels>,
        click_count: usize,
        cx: &mut Context<'_, Self>,
    ) {
        if click_count == 2 {
            let control = Control::from(slider);
            self.set_control(control, control.resting(), cx);
            return;
        }
        self.dragging = Some(slider);
        self.drag_to(slider, position, cx);
    }

    fn drag_to(&mut self, slider: Slider, position: Point<Pixels>, cx: &mut Context<'_, Self>) {
        if let Some(bounds) = self.hit_areas.sliders[slider.index()].get() {
            self.set_control(
                Control::from(slider),
                fraction_at(position, bounds, slider.orientation()),
                cx,
            );
        }
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(slider) = self.dragging else {
            return;
        };
        if event.pressed_button == Some(MouseButton::Left) {
            self.drag_to(slider, event.position, cx);
        } else {
            self.dragging = None;
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<'_, Self>) {
        self.dragging = None;
    }

    fn duration_seconds(&self, deck: DeckId) -> Option<f64> {
        let engine_duration = self.engine.as_ref().ok().and_then(|engine| {
            engine
                .controller
                .track(deck)
                .and_then(dubplate_engine::Track::duration_seconds)
        });
        engine_duration.or_else(|| {
            let seconds = self.deck(deck).track.as_ref()?.track.length_seconds?;
            Some(f64::from(seconds))
        })
    }

    /// Reads the engine for this frame and keeps the frames coming while anything moves.
    fn advance_frame(&mut self, window: &mut Window) -> [DeckFrame; DECK_COUNT] {
        let now = Instant::now();
        let elapsed = (now - self.last_frame).as_secs_f32();
        self.last_frame = now;
        if let Ok(engine) = &self.engine {
            let meters = engine.controller.take_meters();
            for (meter, peak) in self.deck_meters.iter_mut().zip(meters.deck_peaks) {
                meter.update(peak, elapsed);
            }
            self.master_meter.update(meters.master_peak, elapsed);
        }
        let frames = DeckId::ALL.map(|deck| {
            let state = self.deck_state(deck);
            DeckFrame {
                state,
                playhead_seconds: self.decks[deck.index()].clock.position(&state, now),
                duration_seconds: self.duration_seconds(deck),
            }
        });
        let moving = frames.iter().any(|frame| frame.state.playing)
            || self.master_meter.is_moving()
            || self.deck_meters.iter().any(PeakMeter::is_moving)
            || self
                .decks
                .iter()
                .any(|deck| matches!(deck.content, DeckContent::Loading));
        if moving {
            window.request_animation_frame();
        }
        frames
    }

    /// The message showing on `deck`, if any.
    fn deck_notice(&self, deck: DeckId) -> Option<&SharedString> {
        self.notice
            .as_ref()
            .filter(|notice| notice.place == NoticePlace::Deck(deck))
            .map(|notice| &notice.text)
    }

    fn dropouts(&self) -> u64 {
        self.engine
            .as_ref()
            .map_or(0, |engine| engine.controller.stats().overruns())
    }
}

/// Hands what the controllers do to the window as it arrives, a burst at a time.
fn listen_to_controllers(
    events: async_channel::Receiver<LinkEvent>,
    window: &mut Window,
    cx: &mut Context<'_, DubplateApp>,
) {
    cx.spawn_in(window, async move |this, cx| {
        while let Ok(first) = events.recv().await {
            let mut burst = vec![first];
            while let Ok(next) = events.try_recv() {
                burst.push(next);
            }
            let handled = this.update_in(cx, |this, window, cx| {
                this.handle_link_events(burst, window, cx);
            });
            if handled.is_err() {
                break;
            }
        }
    })
    .detach();
}

/// Keeps the blinking lights blinking and the level meters moving on the controllers.
fn keep_lights_moving(cx: &mut Context<'_, DubplateApp>) {
    cx.spawn(async move |this, cx| {
        loop {
            cx.background_executor().timer(LIGHTS_INTERVAL).await;
            if this.update(cx, |this, _| this.update_lights()).is_err() {
                break;
            }
        }
    })
    .detach();
}

fn start_engine(mixer: &MixerPositions) -> Result<Engine, SharedString> {
    let (mut controller, output) = AudioOutput::start(&OutputConfig {
        buffer_frames: Some(BUFFER_FRAMES),
    })
    .map_err(|error| SharedString::from(error_chain(&error)))?;
    for deck in DeckId::ALL {
        controller
            .set_channel_fader(deck, mixer.channel_faders[deck.index()])
            .map_err(|error| SharedString::from(error.to_string()))?;
    }
    controller
        .set_crossfader(mixer.crossfader)
        .and_then(|()| controller.set_master_gain(master_gain_at(mixer.master_level)))
        .map_err(|error| SharedString::from(error.to_string()))?;
    Ok(Engine { controller, output })
}

/// An error with its causes, as one sentence.
fn error_chain(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

impl Render for DubplateApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let render_started = Instant::now();
        let frames = self.advance_frame(window);
        let root = div()
            .key_context(WINDOW_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, action: &Perform, window, cx| {
                this.perform(action.0, window, cx);
            }))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme::BACKGROUND)
            .text_color(theme::TEXT)
            .font_family(UI_FAMILY)
            .text_size(theme::TEXT_SIZE)
            .child(header::render(self, cx))
            .child(lanes::render(self, &frames, cx))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .border_b_1()
                    .border_color(theme::LINE)
                    .child(deck_panel::render(self, DeckId::A, &frames[0], cx))
                    .child(mixer::render(self, cx))
                    .child(deck_panel::render(self, DeckId::B, &frames[1], cx)),
            )
            .child(
                self.browser
                    .clone()
                    .cached(StyleRefinement::default().flex_1().min_h_0().w_full()),
            )
            .children(
                self.notice
                    .as_ref()
                    .filter(|notice| notice.place == NoticePlace::Window)
                    .map(|notice| notice_toast(notice.text.clone())),
            )
            .when(self.help_open, |root| root.child(help::render(cx)));
        if let Some(stats) = &self.frame_stats {
            stats.borrow_mut().record_render(render_started.elapsed());
        }
        root
    }
}

fn notice_toast(text: SharedString) -> impl IntoElement {
    div()
        .absolute()
        .bottom(px(16.0))
        .left_0()
        .right_0()
        .flex()
        .justify_center()
        .child(
            div()
                .px(px(12.0))
                .py(px(8.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(theme::LINE)
                .bg(theme::RAISED)
                .text_color(theme::TEXT)
                .child(text),
        )
}
