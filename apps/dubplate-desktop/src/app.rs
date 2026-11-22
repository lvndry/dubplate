//! The window: owns the audio engine, the two decks, the mixer positions and the browser, turns
//! clicks and shortcuts into engine commands, and redraws at the screen's frame rate while
//! anything moves.

mod analysis;
mod audio;
mod audition;
mod deck_panel;
mod drop_target;
mod effects_strip;
mod export;
mod header;
mod help;
mod knob;
mod lanes;
mod library;
mod library_panel;
mod mixer;
mod pad_grid;
mod pads;
mod prepare;
mod recording;
mod relink;
mod sampler;
mod set_panel;
mod slider;
mod smart;
mod track_check;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dubplate_engine::{
    AudioOutput, Controller, DECK_COUNT, DeckId, DeckState, EFFECT_UNIT_COUNT, EffectUnitId,
    EffectUnitState, EngineError, HeadphoneSettings, MasterTempo, OutputDevice, SyncMode,
    TempoRange, TempoState,
};
use dubplate_midi::{Feedback, PadMode};
use dubplate_prep::{AnalysisQueue, GridCorrection};
use dubplate_rekordbox::HotCuePad;
use gpui::component::input::{InputEvent, InputState};
use gpui::prelude::*;
use gpui::{
    Bounds, Context, Entity, FocusHandle, MouseButton, MouseMoveEvent, MouseUpEvent, Pixels, Point,
    ScrollHandle, SharedString, StyleRefinement, Subscription, Task, Window, div, px,
};

use crate::analysis::{AnalysisPriority, TrackAnalyser, TrackAnalyses};
use crate::browser::{Browser, BrowserEvent, LibraryStatus};
use crate::collection::{Collection, CollectionTrack};
use crate::commands::{Command, EffectEdit, JogMotion, Mode, Press};
use crate::controls::{
    CROSSFADER_STEP, Control, MixerPositions, Orientation, Slider, fraction_at, master_gain_at,
    nudged_tempo_slider, tempo_fader, trim_gain_at,
};
use crate::deck::{
    Deck, DeckContent, GridSource, HotCues, LoadError, PreparedTrack, TrackVisuals, deck_letter,
    prepare_track,
};
use crate::effects::{EffectUnitSetting, search, step_beat, step_effect};
use crate::fonts::UI_FAMILY;
use crate::frame_stats::FrameStats;
use crate::hardware::{
    DeckLights, Hardware, Lights, LinkEvent, Surface, controller_deck, meter_level,
};
use crate::library::LibrarySources;
use crate::loading::{LoadRefusal, LoadSource, check_deck_free, check_file};
use crate::meter::PeakMeter;
use crate::pads::{PadFx, next_mode};
use crate::prep::{Catalogue, TrackCheck, TrackDetail};
use crate::recording::Recording;
use crate::relink::Missing;
use crate::set::SetEditor;
use crate::settings::Settings;
use crate::shortcuts::{Perform, WINDOW_CONTEXT};
use crate::theme;
use crate::transport::{self, CueDown, is_at_cue, other_deck};
use crate::waveform::Zoom;
use audition::Audition;
use pads::HeldPad;
use prepare::PrepareFocus;
use relink::RelinkReview;

/// Frames per device buffer: about 5 ms at 48 kHz.
const BUFFER_FRAMES: u32 = 256;
/// How long a notice stays on screen.
const NOTICE_DURATION: Duration = Duration::from_secs(6);
/// How wide a notice grows before its text wraps.
const NOTICE_MAX_WIDTH: Pixels = px(560.0);
/// How often the controller lights follow the decks while nothing else updates them, for the
/// blinking lights and the level meters.
const LIGHTS_INTERVAL: Duration = Duration::from_millis(30);
/// How long after the last change to an effect unit its settings are saved.
const SETTINGS_SAVE_DELAY: Duration = Duration::from_millis(500);
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
    check_overview: Cell<Option<Bounds<Pixels>>>,
    effect_levels: [Cell<Option<Bounds<Pixels>>>; EFFECT_UNIT_COUNT],
}

/// What the pointer is turning or sliding while the button is held.
#[derive(Debug, Clone, Copy)]
enum Drag {
    /// A slider follows the pointer.
    Slider(Slider),
    /// A knob turns as the pointer moves up or down from where it was pressed.
    Knob {
        control: Control,
        from_y: Pixels,
        from_value: f32,
    },
    /// An effect unit's level bar follows the pointer.
    EffectLevel(EffectUnitId),
}

/// How far the pointer moves up or down to turn a knob from one end to the other.
const KNOB_TRAVEL: f32 = 160.0;

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
    sources: LibrarySources,
    library_generation: u64,
    library_open: bool,
    settings_saving: Option<Task<()>>,
    decks: [Deck; DECK_COUNT],
    deck_meters: [PeakMeter; DECK_COUNT],
    master_meter: PeakMeter,
    mixer: MixerPositions,
    dragging: Option<Drag>,
    header_drag_armed: bool,
    lock_playing_deck: bool,
    settings: Settings,
    settings_path: Option<PathBuf>,
    hardware: Option<Hardware>,
    edited_hot_cues: HashMap<String, HotCues>,
    lights_settling: bool,
    effect_focus: EffectUnitId,
    mode: Mode,
    pad_modes: [PadMode; DECK_COUNT],
    beat_jump_scales: [i32; DECK_COUNT],
    held_pads: [Option<HeldPad>; DECK_COUNT],
    key_pad_offsets: [i32; DECK_COUNT],
    keyboard_cues: [HotCuePad; DECK_COUNT],
    selected_samples: [usize; DECK_COUNT],
    set_editor: Option<SetEditor>,
    set_selection: Option<usize>,
    prepare_focus: PrepareFocus,
    next_cursor: usize,
    set_scroll: ScrollHandle,
    track_checks: HashMap<PathBuf, TrackCheck>,
    checks_pending: HashSet<PathBuf>,
    check_generation: u64,
    track_detail: Option<(PathBuf, Arc<TrackDetail>)>,
    detail_pending: Option<PathBuf>,
    check_cursor_seconds: f64,
    catalogue: Arc<Catalogue>,
    missing: Arc<Vec<Missing>>,
    relinking: bool,
    relink_review: Option<RelinkReview>,
    audition: Option<Audition>,
    audition_generation: u64,
    analyses: TrackAnalyses,
    analysis_collecting: bool,
    recording: Option<Recording>,
    recording_skip_reported: bool,
    smart_cfx_menu: bool,
    effect_menu: Option<EffectUnitId>,
    refocus_window: bool,
    effect_search: Entity<InputState>,
    settings_save_pending: bool,
    output_devices: Vec<OutputDevice>,
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
    /// Starts the audio engine, opens the browser and opens the library the DJ chose in the
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
                BrowserEvent::AddMusicFolders => this.add_music_folders(cx),
                BrowserEvent::ImportRekordbox => this.set_rekordbox_import(true, cx),
                BrowserEvent::RelinkAll => this.start_relink(None, cx),
                BrowserEvent::Relink(track) => this.start_relink(Some(track), cx),
            });
        let effect_search = cx.new(|cx| InputState::new(window, cx).placeholder("Find an effect"));
        let quit_subscription = cx.on_app_quit(|this: &mut Self, _| {
            this.finish_recording_on_quit();
            async {}
        });
        let search_subscription = cx.subscribe(
            &effect_search,
            |this, input, event: &InputEvent, cx| match event {
                InputEvent::Change => cx.notify(),
                InputEvent::PressEnter { .. } => {
                    let query = input.read(cx).value().to_string();
                    let first = search(&query)
                        .into_iter()
                        .flat_map(|(_, effects)| effects)
                        .next();
                    if let (Some(unit), Some(kind)) = (this.effect_menu, first) {
                        this.edit_effect(unit, EffectEdit::Kind(kind), cx);
                    }
                    this.close_effect_menu(cx);
                }
                InputEvent::Focus | InputEvent::Blur => {}
            },
        );
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
        let mixer = MixerPositions {
            sampler_volume: settings.sampler.volume.clamp(0.0, 1.0),
            ..MixerPositions::default()
        };
        let (engine, audio_problem) = audio::start_engine(&mixer, settings.audio_device.as_deref());
        problems.extend(audio_problem);
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
        let set_editor = Settings::sets_path()
            .map_err(|error| error.to_string())
            .and_then(|path| SetEditor::open(&path).map_err(|error| error.to_string()))
            .map_err(|problem| problems.push(format!("Sets cannot be kept: {problem}")))
            .ok();
        let analysis_queue = Settings::analysis_cache_path()
            .map_err(|error| error.to_string())
            .and_then(|path| AnalysisQueue::start(path).map_err(|error| error.to_string()))
            .map_err(|problem| problems.push(format!("Tracks cannot be analysed: {problem}")))
            .ok();
        let sources = LibrarySources::new(
            settings.library.music_folders.clone(),
            settings.library.import_rekordbox,
        );
        let mut app = Self {
            engine,
            library: LibraryStatus::Reading,
            sources,
            library_generation: 0,
            library_open: false,
            settings_saving: None,
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
            effect_focus: EffectUnitId::ALL[0],
            mode: Mode::Perform,
            pad_modes: [PadMode::HotCue; DECK_COUNT],
            beat_jump_scales: [0; DECK_COUNT],
            held_pads: [None; DECK_COUNT],
            key_pad_offsets: [0; DECK_COUNT],
            keyboard_cues: [HotCuePad::ALL[0]; DECK_COUNT],
            selected_samples: [0; DECK_COUNT],
            set_editor,
            set_selection: None,
            prepare_focus: PrepareFocus::Library,
            next_cursor: 0,
            set_scroll: ScrollHandle::new(),
            track_checks: HashMap::new(),
            checks_pending: HashSet::new(),
            check_generation: 0,
            track_detail: None,
            detail_pending: None,
            check_cursor_seconds: 0.0,
            catalogue: Arc::default(),
            missing: Arc::default(),
            relinking: false,
            relink_review: None,
            audition: None,
            audition_generation: 0,
            analyses: TrackAnalyses::new(
                analysis_queue.map(|queue| Box::new(queue) as Box<dyn TrackAnalyser>),
            ),
            analysis_collecting: false,
            recording: None,
            recording_skip_reported: false,
            smart_cfx_menu: false,
            effect_menu: None,
            refocus_window: false,
            effect_search,
            settings_save_pending: false,
            output_devices: Vec::new(),
            hit_areas: Rc::default(),
            browser,
            help_open: false,
            notice: None,
            notice_count: 0,
            focus_handle,
            last_frame: Instant::now(),
            frame_stats: FrameStats::from_environment().map(|stats| Rc::new(RefCell::new(stats))),
            _subscriptions: vec![subscription, search_subscription, quit_subscription],
        };
        app.restore_effect_units(cx);
        app.restore_sampler(cx);
        app.start_library(cx);
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

    /// Writes the settings on a background thread, after any earlier write, so the last
    /// settings always win.
    fn save_settings(&mut self, cx: &mut Context<'_, Self>) {
        let Some(path) = self.settings_path.clone() else {
            return;
        };
        let settings = self.settings.clone();
        let previous = self.settings_saving.take();
        self.settings_saving = Some(cx.spawn(async move |this, cx| {
            if let Some(previous) = previous {
                previous.await;
            }
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
        }));
    }

    fn collection(&self) -> Option<&Arc<Collection>> {
        match &self.library {
            LibraryStatus::Ready(collection) => Some(collection),
            LibraryStatus::Reading | LibraryStatus::Unchosen => None,
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
        self.load_cued(deck, source, None, cx);
    }

    /// Loads a track as [`Self::load`] does, then puts the deck at `cue` seconds, stopped, with
    /// its cue point there.
    pub(super) fn load_cued(
        &mut self,
        deck: DeckId,
        source: LoadSource,
        cue: Option<f64>,
        cx: &mut Context<'_, Self>,
    ) {
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
            .and_then(|collection| collection.share_directory().map(Path::to_path_buf));
        let correction = self.grid_correction(&track);
        let analysis = track.track.file_path.as_deref().and_then(|path| {
            self.request_analysis([path], AnalysisPriority::Set, cx);
            self.analyses.get(path).cloned()
        });
        cx.spawn(async move |this, cx| {
            let prepared = cx
                .background_executor()
                .spawn(async move {
                    prepare_track(
                        &track,
                        share_directory.as_deref(),
                        Zoom::DEFAULT,
                        correction,
                        analysis.as_deref(),
                    )
                })
                .await;
            this.update(cx, |this, cx| {
                this.finish_load(deck, generation, prepared, cx);
                if let Some(seconds) = cue
                    && this.deck(deck).is_current_load(generation)
                    && this.deck(deck).visuals().is_some()
                {
                    this.jump(deck, seconds, cx);
                }
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
        let grid_source = prepared
            .as_ref()
            .ok()
            .and_then(|prepared| prepared.grid_source);
        let (content, grid_source) =
            match prepared.and_then(|prepared| self.put_on_deck(deck, prepared)) {
                Ok(visuals) => {
                    self.signal_controllers(Feedback::TrackLoaded(controller_deck(deck)));
                    (DeckContent::Loaded(visuals), grid_source)
                }
                Err(error) => (DeckContent::Failed(error_chain(&error)), None),
            };
        self.decks[deck.index()].content = content;
        self.decks[deck.index()].grid_source = grid_source;
        self.give_deck_analysed_grid(deck, cx);
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
        engine.controller.set_beat_grid(deck, prepared.beat_grid)?;
        engine.controller.set_key_shift(deck, 0.0)?;
        if let Some(held) = self.held_pads[deck.index()].take() {
            match held.fx {
                PadFx::Roll(_) => engine.controller.release_roll(deck)?,
                PadFx::Effect(_) => engine.controller.release_pad_effect(deck)?,
            }
        }
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
            Command::Pad(press) => self.press_pad(press, cx),
            Command::SetPadMode(deck, mode) => self.set_pad_mode(deck, mode, cx),
            Command::CyclePadMode(deck) => {
                self.set_pad_mode(deck, next_mode(self.pad_modes[deck.index()]), cx);
            }
            Command::ToggleSlip(deck) => self.toggle_slip(deck, cx),
            Command::Loop(deck, action) => self.loop_button(deck, action, cx),
            Command::ToggleHeadphoneCue(deck) => {
                let cued = self
                    .headphones()
                    .is_some_and(|headphones| headphones.is_cued(deck));
                self.engine_command(cx, |controller| controller.set_headphone_cue(deck, !cued));
            }
            Command::TapTempo(deck) => self.tap_tempo(deck, cx),
            Command::AuditionSeam => self.toggle_seam_audition(cx),
            Command::ToggleRecording => self.toggle_recording(cx),
            Command::ToggleSmartCfx => self.toggle_smart_cfx(cx),
            Command::NextSmartCfx => {
                let next = self.smart_cfx_preset().next();
                self.choose_smart_cfx(next, cx);
            }
            Command::ChooseSmartCfx(preset) => self.choose_smart_cfx(preset, cx),
            Command::ToggleSmartFader => self.toggle_smart_fader(cx),
            Command::ToggleMasterCue => {
                let cued = self
                    .headphones()
                    .is_some_and(|headphones| headphones.master_cue);
                self.engine_command(cx, |controller| controller.set_master_cue(!cued));
            }
            Command::LoadSelected(deck) => {
                if let Some(track) = self.browser.read(cx).selected() {
                    self.load(deck, LoadSource::Collection(track), cx);
                }
            }
            Command::InstantDouble(deck) => self.instant_double(deck, cx),
            Command::NudgeTempo(deck, direction) => {
                let slider = nudged_tempo_slider(
                    self.mixer.tempo_sliders[deck.index()],
                    direction.sign(),
                    self.tempo_range(deck),
                );
                self.set_control(Control::Tempo(deck), slider, cx);
            }
            Command::ToggleSync(deck) => {
                let synced = self
                    .tempo_state(deck)
                    .is_some_and(|state| state.sync != SyncMode::Off);
                let mode = if synced {
                    SyncMode::Off
                } else {
                    SyncMode::Beat
                };
                self.engine_command(cx, |controller| controller.set_sync(deck, mode));
                let nothing_to_follow = DeckId::ALL
                    .into_iter()
                    .all(|other| !self.deck_state(other).playing);
                if mode != SyncMode::Off && nothing_to_follow {
                    self.show_notice(
                        NoticePlace::Deck(deck),
                        format!(
                            "Sync armed: deck {} locks to the tempo master as soon as a deck \
                             with a beat grid plays.",
                            deck_letter(deck)
                        ),
                        cx,
                    );
                }
            }
            Command::MakeTempoMaster(deck) => self.engine_command(cx, |controller| {
                controller.set_master_tempo(MasterTempo::Deck(deck))
            }),
            Command::ToggleKeyLock(deck) => {
                let locked = self.key_lock(deck);
                self.engine_command(cx, |controller| controller.set_key_lock(deck, !locked));
            }
            Command::CycleTempoRange(deck) => {
                let range = self.tempo_range(deck).next();
                self.engine_command(cx, |controller| controller.set_tempo_range(deck, range));
            }
            Command::SetControl(control, fraction) => self.set_control(control, fraction, cx),
            Command::MoveCrossfader(direction) => {
                let position = self.mixer.crossfader + CROSSFADER_STEP * direction.sign() as f32;
                self.set_control(Control::Crossfader, position.clamp(0.0, 1.0), cx);
            }
            Command::MoveSelection(direction) => match (self.mode, self.prepare_focus) {
                (Mode::Prepare, PrepareFocus::Set) => {
                    self.move_set_selection(direction);
                    self.ensure_track_detail(cx);
                }
                (Mode::Prepare, PrepareFocus::Next) => self.move_next_cursor(direction),
                _ => self
                    .browser
                    .update(cx, |browser, cx| browser.move_selection(direction, cx)),
            },
            Command::SwitchMode(mode) => {
                self.mode = mode;
                self.browser.update(cx, |browser, cx| {
                    browser.show_load_buttons(mode == Mode::Perform, cx)
                });
            }
            Command::EditSet(edit) => self.edit_set(edit, cx),
            Command::SwitchPrepareFocus(direction) => {
                self.prepare_focus = self.prepare_focus.step(direction);
                self.next_cursor = 0;
            }
            Command::LoadSet => self.load_set(cx),
            Command::ExportSet => self.export_set(cx),
            Command::Browse(steps) => self
                .browser
                .update(cx, |browser, cx| browser.browse(steps, cx)),
            Command::SwitchBrowserPane => self
                .browser
                .update(cx, |browser, cx| browser.switch_pane(cx)),
            Command::Jog(deck, motion, ticks) => {
                if self.deck(deck).visuals().is_some() {
                    self.engine_command(cx, |controller| match motion {
                        JogMotion::Scratch => controller.jog_scratch(deck, ticks),
                        JogMotion::Nudge => controller.jog_nudge(deck, ticks),
                        JogMotion::Search => controller.jog_search(deck, ticks),
                    });
                }
            }
            Command::JogTouch(deck, touched) => {
                self.engine_command(cx, |controller| controller.set_jog_touch(deck, touched));
            }
            Command::Effect(unit, edit) => self.edit_effect(unit, edit, cx),
            Command::FocusEffectUnit(steps) => {
                self.effect_focus = step_unit(self.effect_focus, steps);
            }
            Command::FocusSearch => self
                .browser
                .update(cx, |browser, cx| browser.focus_search(window, cx)),
            Command::ToggleHelp => {
                self.help_open = !self.help_open;
                self.library_open = false;
            }
            Command::ToggleLibrary => {
                self.library_open = !self.library_open;
                if self.library_open {
                    self.refresh_output_devices(cx);
                }
                self.help_open = false;
            }
            Command::AddMusicFolders => self.add_music_folders(cx),
            Command::Dismiss => {
                if self.effect_menu.is_some() {
                    self.close_effect_menu(cx);
                } else if self.relink_review.is_some() {
                    self.close_relink_review(cx);
                } else if self.help_open || self.library_open {
                    self.help_open = false;
                    self.library_open = false;
                } else if self.notice.is_some() {
                    self.notice = None;
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
                self.engine_command(cx, |controller| controller.hot_cue(deck, seconds));
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
        let range = self.tempo_range(source);
        self.engine_command(cx, |controller| controller.set_tempo_range(deck, range));
        self.set_control(
            Control::Tempo(deck),
            self.mixer.tempo_sliders[source.index()],
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
        self.show_notice_for(place, text, NOTICE_DURATION, cx);
    }

    /// Shows a notice for `duration`, or until Escape closes it.
    fn show_notice_for(
        &mut self,
        place: NoticePlace,
        text: impl Into<SharedString>,
        duration: Duration,
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
            cx.background_executor().timer(duration).await;
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
                self.mixer.tempo_sliders[deck.index()] = fraction;
                self.engine_command(cx, |controller| {
                    controller.set_tempo_fader(deck, tempo_fader(fraction))
                });
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
            Control::Eq(deck, band) => {
                self.mixer.eqs[deck.index()][band.index()] = fraction;
                self.engine_command(cx, |controller| controller.set_eq(deck, band, fraction));
            }
            Control::Filter(deck) => {
                self.mixer.filters[deck.index()] = fraction;
                self.engine_command(cx, |controller| controller.set_filter(deck, fraction));
            }
            Control::HeadphoneMix => {
                self.mixer.headphone_mix = fraction;
                self.engine_command(cx, |controller| controller.set_headphone_mix(fraction));
            }
            Control::HeadphoneLevel => {
                self.mixer.headphone_level = fraction;
                self.engine_command(cx, |controller| controller.set_headphone_level(fraction));
            }
            Control::SamplerVolume => {
                self.mixer.sampler_volume = fraction;
                self.settings.sampler.volume = fraction;
                self.engine_command(cx, |controller| controller.set_sampler_volume(fraction));
                self.save_settings(cx);
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
                LinkEvent::Connected {
                    name,
                    jog_ticks_per_revolution,
                    ..
                } => {
                    if let Some(ticks) = jog_ticks_per_revolution {
                        self.engine_command(cx, |controller| {
                            controller.set_jog_ticks_per_revolution(u32::from(ticks))
                        });
                    }
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
                        focused_effect: Some(self.effect_focus),
                        effect_depth: self
                            .effect_unit(self.effect_focus)
                            .map_or(0.0, |unit| unit.depth),
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

    /// A tap on a deck's beat. Enough taps give a track without rekordbox's beat grid its tempo
    /// and its beats, so sync and the effects follow it, in place of any grid Dubplate's
    /// analysis gave it. A track with rekordbox's grid keeps it.
    fn tap_tempo(&mut self, deck: DeckId, cx: &mut Context<'_, Self>) {
        if self.deck(deck).visuals().is_none() {
            return;
        }
        if self.deck(deck).grid_source == Some(GridSource::Rekordbox) {
            self.show_notice(
                NoticePlace::Deck(deck),
                "This track keeps rekordbox's beat grid; tap tempo is for tracks without one.",
                cx,
            );
            return;
        }
        let heard = self.heard_seconds(deck);
        let Some(tapped) = self.decks[deck.index()].tap_tempo.tap(heard) else {
            return;
        };
        let grid = match dubplate_engine::BeatGrid::new(tapped.bpm, tapped.first_beat_seconds) {
            Ok(grid) => grid,
            Err(error) => {
                self.show_notice(NoticePlace::Deck(deck), error.to_string(), cx);
                return;
            }
        };
        let deck_state = &mut self.decks[deck.index()];
        if deck_state.grid_source == Some(GridSource::Analysis)
            && let DeckContent::Loaded(visuals) = &mut deck_state.content
        {
            visuals.beats = Arc::default();
        }
        deck_state.grid_source = Some(GridSource::Tapped);
        self.engine_command(cx, |controller| controller.set_beat_grid(deck, Some(grid)));
    }

    /// The DJ's fix of a track's grid, when the track is in the set.
    fn grid_correction(&self, track: &CollectionTrack) -> GridCorrection {
        let entry = track.track.file_path.as_deref().and_then(|path| {
            let editor = self.set_editor.as_ref()?;
            editor
                .set()
                .entries
                .iter()
                .find(|entry| entry.track_path == path)
        });
        entry.map(GridCorrection::of).unwrap_or_default()
    }

    /// The headphone section, when the engine runs.
    pub(super) fn headphones(&self) -> Option<HeadphoneSettings> {
        self.engine
            .as_ref()
            .ok()
            .map(|engine| engine.controller.headphones())
    }

    /// A deck's tempo and sync, when the engine runs.
    pub(super) fn tempo_state(&self, deck: DeckId) -> Option<TempoState> {
        self.engine
            .as_ref()
            .ok()
            .map(|engine| engine.controller.tempo_state(deck))
    }

    /// Whether a deck's key lock is on.
    pub(super) fn key_lock(&self, deck: DeckId) -> bool {
        self.engine
            .as_ref()
            .is_ok_and(|engine| engine.controller.key_lock(deck))
    }

    /// How far a deck's tempo slider reaches.
    pub(super) fn tempo_range(&self, deck: DeckId) -> TempoRange {
        self.tempo_state(deck)
            .map(|state| state.tempo_range)
            .unwrap_or_default()
    }

    /// An effect unit's settings, when the engine runs.
    pub(super) fn effect_unit(&self, unit: EffectUnitId) -> Option<EffectUnitState> {
        self.engine
            .as_ref()
            .ok()
            .map(|engine| engine.controller.effect_unit(unit))
    }

    /// Changes an effect unit, from the strip or the controller, and remembers it.
    pub(super) fn edit_effect(
        &mut self,
        unit: EffectUnitId,
        edit: EffectEdit,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(state) = self.effect_unit(unit) else {
            return;
        };
        self.engine_command(cx, |controller| match edit {
            EffectEdit::Kind(kind) => controller.set_effect_kind(unit, kind),
            EffectEdit::StepKind(steps) => {
                controller.set_effect_kind(unit, step_effect(state.kind, steps))
            }
            EffectEdit::Beat(direction) => {
                controller.set_effect_beat(unit, step_beat(state.beat, direction))
            }
            EffectEdit::Target(target) => controller.set_effect_target(unit, target),
            EffectEdit::Depth(depth) => controller.set_effect_depth(unit, depth.clamp(0.0, 1.0)),
            EffectEdit::Toggle => controller.toggle_effect(unit),
        });
        if !matches!(edit, EffectEdit::Toggle) {
            self.remember_effect_units(cx);
        }
        cx.notify();
    }

    /// Opens the effect picker of `unit` with the cursor in its search field, or closes it.
    pub(super) fn toggle_effect_menu(
        &mut self,
        unit: EffectUnitId,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if self.effect_menu == Some(unit) {
            self.close_effect_menu(cx);
            return;
        }
        self.effect_menu = Some(unit);
        self.effect_search.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    /// Closes the effect picker and gives the keys back to the window.
    pub(super) fn close_effect_menu(&mut self, cx: &mut Context<'_, Self>) {
        if self.effect_menu.take().is_some() {
            self.refocus_window = true;
            cx.notify();
        }
    }

    /// Whether a controller is connected.
    pub(super) fn hardware_connected(&self) -> bool {
        self.hardware.as_ref().is_some_and(Hardware::is_connected)
    }

    /// Puts each effect unit back as it was when Dubplate last closed.
    fn restore_effect_units(&mut self, cx: &mut Context<'_, Self>) {
        let saved = self.settings.effect_units.clone();
        for (unit, setting) in EffectUnitId::ALL.into_iter().zip(saved) {
            self.engine_command(cx, |controller| {
                if let Some(kind) = setting.kind() {
                    controller.set_effect_kind(unit, kind)?;
                }
                if let Some(target) = setting.target() {
                    controller.set_effect_target(unit, target)?;
                }
                if let Some(beat) = setting.beat() {
                    controller.set_effect_beat(unit, beat)?;
                }
                match setting.depth() {
                    Some(depth) => controller.set_effect_depth(unit, depth),
                    None => Ok(()),
                }
            });
        }
    }

    /// Saves the effect units' settings a moment after the last change, so turning the level
    /// knob writes the file once rather than on every step.
    fn remember_effect_units(&mut self, cx: &mut Context<'_, Self>) {
        let units: Vec<EffectUnitSetting> = EffectUnitId::ALL
            .into_iter()
            .filter_map(|unit| self.effect_unit(unit))
            .map(|state| EffectUnitSetting::of(&state))
            .collect();
        if units.len() == EFFECT_UNIT_COUNT {
            self.settings.effect_units = units;
        }
        if std::mem::replace(&mut self.settings_save_pending, true) {
            return;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SETTINGS_SAVE_DELAY).await;
            this.update(cx, |this, cx| {
                this.settings_save_pending = false;
                this.save_settings(cx);
            })
            .ok();
        })
        .detach();
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
            beat_fx_on: self
                .effect_unit(self.effect_focus)
                .is_some_and(|unit| unit.enabled),
            smart_cfx: self.smart_cfx_on(),
            smart_fader: self.smart_fader_on(),
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
        let pad_lights = self.pad_lights(deck, blink);
        DeckLights {
            play,
            cue,
            pad_mode: self.pad_modes[deck.index()],
            pads: pad_lights.pads,
            headphone_cue: self
                .headphones()
                .is_some_and(|headphones| headphones.is_cued(deck)),
            sync: self
                .tempo_state(deck)
                .is_some_and(|state| state.sync != SyncMode::Off),
            level: meter_level(self.deck_meters[deck.index()].decibels()),
            loop_in: pad_lights.loop_in,
            loop_out: pad_lights.loop_out,
            four_beat: pad_lights.four_beat,
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
        self.dragging = Some(Drag::Slider(slider));
        self.drag_to(Drag::Slider(slider), position, cx);
    }

    /// Starts turning a knob, or puts it back where it rests on a double-click.
    pub(super) fn begin_knob_drag(
        &mut self,
        control: Control,
        position: Point<Pixels>,
        click_count: usize,
        cx: &mut Context<'_, Self>,
    ) {
        if click_count == 2 {
            self.set_control(control, control.resting(), cx);
            return;
        }
        self.dragging = Some(Drag::Knob {
            control,
            from_y: position.y,
            from_value: self.control_fraction(control),
        });
    }

    /// Starts sliding an effect unit's level bar.
    pub(super) fn begin_effect_level_drag(
        &mut self,
        unit: EffectUnitId,
        position: Point<Pixels>,
        cx: &mut Context<'_, Self>,
    ) {
        self.dragging = Some(Drag::EffectLevel(unit));
        self.drag_to(Drag::EffectLevel(unit), position, cx);
    }

    fn drag_to(&mut self, drag: Drag, position: Point<Pixels>, cx: &mut Context<'_, Self>) {
        match drag {
            Drag::Slider(slider) => {
                if let Some(bounds) = self.hit_areas.sliders[slider.index()].get() {
                    self.set_control(
                        Control::from(slider),
                        fraction_at(position, bounds, slider.orientation()),
                        cx,
                    );
                }
            }
            Drag::Knob {
                control,
                from_y,
                from_value,
            } => {
                let turned = f32::from(from_y - position.y) / KNOB_TRAVEL;
                self.set_control(control, from_value + turned, cx);
            }
            Drag::EffectLevel(unit) => {
                if let Some(bounds) = self.hit_areas.effect_levels[unit.index()].get() {
                    let depth = fraction_at(position, bounds, Orientation::Horizontal);
                    self.edit_effect(unit, EffectEdit::Depth(depth), cx);
                }
            }
        }
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(drag) = self.dragging else {
            return;
        };
        if event.pressed_button == Some(MouseButton::Left) {
            self.drag_to(drag, event.position, cx);
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
            for (meter, peak) in self.deck_meters.iter_mut().zip(meters.channel_peaks) {
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

/// The effect unit `steps` along from `unit`, going round at either end.
fn step_unit(unit: EffectUnitId, steps: i32) -> EffectUnitId {
    let count = i64::try_from(EFFECT_UNIT_COUNT).unwrap_or(1);
    let index = (i64::try_from(unit.index()).unwrap_or(0) + i64::from(steps)).rem_euclid(count);
    usize::try_from(index)
        .ok()
        .and_then(|index| EffectUnitId::ALL.get(index).copied())
        .unwrap_or(unit)
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
        if std::mem::take(&mut self.refocus_window) {
            window.focus(&self.focus_handle, cx);
        }
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
            .map(|root| match self.mode {
                Mode::Perform => root
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
                    .child(effects_strip::render(self, cx))
                    .child(
                        self.browser
                            .clone()
                            .cached(StyleRefinement::default().flex_1().min_h_0().w_full()),
                    ),
                Mode::Prepare => root.child(track_check::render(self, cx)).child(
                    div()
                        .flex()
                        .flex_1()
                        .min_h_0()
                        .child(
                            self.browser
                                .clone()
                                .cached(StyleRefinement::default().flex_1().min_h_0().h_full()),
                        )
                        .child(set_panel::render(self, cx)),
                ),
            })
            .children(
                self.notice
                    .as_ref()
                    .filter(|notice| notice.place == NoticePlace::Window)
                    .map(|notice| notice_toast(notice.text.clone())),
            )
            .when(self.help_open, |root| root.child(help::render(cx)))
            .when_some(self.relink_review.as_ref(), |root, review| {
                root.child(relink::render(self, review, cx))
            })
            .when(self.library_open, |root| {
                root.child(library_panel::render(self, cx))
            });
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
                .max_w(NOTICE_MAX_WIDTH)
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
