//! The window: owns the audio engine, the two decks, the mixer positions and the browser, turns
//! clicks and shortcuts into engine commands, and redraws at the screen's frame rate while
//! anything moves.

mod analysis;
mod audio;
mod audition;
mod cueing;
mod cues;
mod deck_grid;
mod deck_panel;
mod drop_target;
mod effects_strip;
mod export;
mod faders;
mod feel;
mod filter_reference;
mod grid_controls;
mod header;
mod help;
mod history;
mod key_sync;
mod knob;
mod lane_drag;
mod lanes;
mod library;
mod library_matches;
mod library_panel;
mod memory_cues;
mod mixer;
mod notice;
mod overview;
mod pad_fx_editor;
mod pad_grid;
mod pads;
mod pickup;
mod prepare;
mod recording;
mod relink;
mod sampler;
mod script;
mod set_panel;
mod slider;
mod smart;
mod stems;
mod stems_strip;
mod track_actions;
mod track_check;
mod track_preview;
mod usb_export;
mod waveforms;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use dubplate_engine::{
    AudioOutput, Controller, DECK_COUNT, DeckId, DeckState, EFFECT_UNIT_COUNT, EffectKind,
    EffectTarget, EffectUnitId, EffectUnitState, EngineError, HeadphoneSettings, MasterTempo,
    OutputDevice, SyncMode, TempoRange, TempoState,
};
use dubplate_midi::{Feedback, PadMode};
use dubplate_prep::{AnalysisQueue, GridCorrection, TrackGrid, TrackIdentity};
use dubplate_rekordbox::{HotCue, HotCuePad, TrackCues};
use gpui::component::input::{InputEvent, InputState};
use gpui::prelude::*;
use gpui::{
    Bounds, Context, Entity, FocusHandle, MouseButton, MouseMoveEvent, MouseUpEvent, Pixels, Point,
    ScrollHandle, SharedString, StyleRefinement, Subscription, Task, Window, div, px,
};

use crate::analysis::{AnalysisPriority, TrackAnalyser, TrackAnalyses};
use crate::browser::{Browser, BrowserEvent, LibraryStatus};
use crate::collection::{Collection, CollectionTrack, TrackIndex};
use crate::commands::{
    AutoCueChoice, CfxScope, Command, Direction, EffectEdit, JogMotion, Mode, Press,
};
use crate::controls::{
    CROSSFADER_STEP, Control, MixerPositions, Orientation, Slider, fraction_at, master_gain_at,
    nudged_tempo_slider, tempo_fader, trim_gain_at,
};
use crate::cue_marks::{MarkerPulse, SeenMarks};
use crate::deck::{
    Deck, DeckContent, GridSource, LoadError, PreparedTrack, TrackVisuals, deck_letter,
    prepare_track,
};
use crate::devices::{BufferSize, DeviceAccess};
use crate::effects::{
    EffectUnitSetting, PickerMove, PickerRow, picker_position, picker_rows, search, step_effect,
    step_highlight, step_parameter,
};
use crate::fonts::UI_FAMILY;
use crate::frame_stats::FrameStats;
use crate::glitches::GlitchWatch;
use crate::hardware::{
    DeckLights, Hardware, HeldControl, Lights, LinkEvent, Surface, controller_deck, meter_level,
};
use crate::history::{PlayHistory, PlayWatch, SessionRecord};
use crate::library::{LibrarySources, RekordboxSource};
use crate::library_view::SortSetting;
use crate::loading::{LoadRefusal, LoadSource, check_deck_free, check_file, check_present};
use crate::menus::{self, MenuState};
use crate::meter::PeakMeter;
use crate::pad_fx::PadFxBanks;
use crate::pads::next_mode;
use crate::prep::{Catalogue, TrackCheck, TrackDetail};
use crate::recording::Recording;
use crate::relink::Missing;
use crate::set::SetEditor;
use crate::settings::{LevelsSetting, Settings};
use crate::shortcuts::{Perform, WINDOW_CONTEXT};
use crate::space::{DeckOnAir, space_deck};
use crate::stems::TrackStems;
use crate::theme;
use crate::transport::{self, CueDown, is_at_cue, other_deck};
use crate::waveform::Zoom;
use crate::waveform_cache::WaveformCache;
use audition::Audition;
use cues::new_cue_span;
use notice::{NoticePlace, Notices};
use overview::DeckOverview;
use pad_fx_editor::PadFxEditing;
use pads::{HeldPad, PadNote};
use prepare::PrepareFocus;
use relink::RelinkReview;
use track_preview::TrackPreview;

/// How far a pinch goes to zoom the lanes one level: a quarter more or less.
const PINCH_STEP: f32 = 0.25;
/// What the window says when the audio drops out often.
const GLITCH_NOTICE: &str = "Audio is glitching. A larger buffer in Settings > Audio can help.";
/// Below this width the header shows the controller and the sound card as dots.
const NARROW_WINDOW_WIDTH: Pixels = px(1320.0);
/// Below this height the lanes are lower.
const SHORT_WINDOW_HEIGHT: Pixels = px(820.0);
/// How often the controller lights follow the decks while nothing else updates them, for the
/// blinking lights and the level meters.
const LIGHTS_INTERVAL: Duration = Duration::from_millis(30);
/// How long after the last change to an effect unit its settings are saved.
const SETTINGS_SAVE_DELAY: Duration = Duration::from_millis(500);
/// How long the effect picker that FX SELECT opens stays up after the last press, as
/// rekordbox's FX window does.
const FX_SELECT_GLANCE: Duration = Duration::from_millis(1_500);
/// How long after a command the controller lights look again, once the audio thread has
/// carried it out: a few audio buffers.
const LIGHTS_SETTLE: Duration = Duration::from_millis(12);

/// The sound card and the engine playing on it.
struct Engine {
    controller: Controller,
    output: AudioOutput,
}

/// Where the sliders and overviews were last drawn, so a pointer can be turned into a value.
#[derive(Default)]
struct HitAreas {
    sliders: [Cell<Option<Bounds<Pixels>>>; Slider::ALL.len()],
    overviews: [Cell<Option<Bounds<Pixels>>>; DECK_COUNT],
    check_overview: Cell<Option<Bounds<Pixels>>>,
    effect_levels: [Cell<Option<Bounds<Pixels>>>; EFFECT_UNIT_COUNT],
    effect_strip: Cell<Option<Bounds<Pixels>>>,
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
    /// A deck's waveform turns its jog as the pointer moves along the lane.
    Lane(lane_drag::LaneDrag),
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
    track_cues: HashMap<TrackIdentity, Arc<TrackCues>>,
    lights_settling: bool,
    effect_focus: EffectUnitId,
    mode: Mode,
    pad_modes: [PadMode; DECK_COUNT],
    beat_jump_scales: [i32; DECK_COUNT],
    held_pads: [Option<HeldPad>; DECK_COUNT],
    /// The 32 Pad FX pads as the DJ set them.
    pad_fx: PadFxBanks,
    /// The Pad FX editor, when it is open: the pad it is editing.
    pad_fx_editor: Option<PadFxEditing>,
    /// Whether each deck's SHIFT is held on the controller, so its pads show their SHIFT layer.
    shift_held: [bool; DECK_COUNT],
    /// Whether each deck's pads show their SHIFT layer on screen without SHIFT held.
    shift_layer_shown: [bool; DECK_COUNT],
    key_pad_offsets: [i32; DECK_COUNT],
    keyboard_cues: [HotCuePad; DECK_COUNT],
    selected_samples: [usize; DECK_COUNT],
    set_editor: Option<dubplate_prep::SetSession>,
    /// Every track's grid fix, by its file, as the set store keeps them.
    track_grids: HashMap<PathBuf, TrackGrid>,
    /// The deck whose grid bar is open, which the grid keys fix in Perform.
    grid_bar: Option<DeckId>,
    set_selection_generation: u64,
    set_selection: Option<usize>,
    modal_focus: FocusHandle,
    modal_return_focus: Option<FocusHandle>,
    modal_active: bool,
    quit_requested: bool,
    quit_barrier_ready: bool,
    set_edits_pending: usize,
    set_save_problem: Option<String>,
    quit_refused: bool,
    prepare_focus: PrepareFocus,
    next_cursor: usize,
    queued_suggestions: VecDeque<usize>,
    set_scroll: ScrollHandle,
    prepare_view: Arc<prepare::PrepareViewModel>,
    prepare_generation: u64,
    track_checks: HashMap<PathBuf, TrackCheck>,
    checks_pending: HashSet<PathBuf>,
    check_generation: u64,
    track_detail: Option<(PathBuf, Arc<TrackDetail>)>,
    detail_pending: Option<PathBuf>,
    waveform_cache: WaveformCache,
    check_cursor_seconds: f64,
    catalogue: Arc<Catalogue>,
    /// What the library's Match column was last worked out for.
    match_source: Option<library_matches::MatchSource>,
    missing: Arc<Vec<Missing>>,
    relinking: bool,
    relink_review: Option<RelinkReview>,
    audition: Option<Audition>,
    audition_generation: u64,
    /// The library track playing in the headphones.
    track_preview: Option<TrackPreview>,
    preview_generation: u64,
    analyses: TrackAnalyses,
    analysis_collecting: bool,
    stems: TrackStems,
    stems_changed: bool,
    recording: Option<Recording>,
    recording_skip_reported: bool,
    play_history: PlayHistory,
    play_watches: [PlayWatch<SessionRecord>; DECK_COUNT],
    time_zone: jiff::tz::TimeZone,
    smart_cfx_menu: bool,
    /// The Sound Color FX picker, when it is open, and the channels it sets.
    cfx_menu: Option<CfxScope>,
    effect_menu: Option<EffectMenu>,
    /// The effect the picker's arrow keys and Enter are on, when moved from the unit's own.
    effect_highlight: Option<EffectKind>,
    effect_picker_scroll: ScrollHandle,
    fx_select_presses: u64,
    refocus_window: bool,
    effect_search: Entity<InputState>,
    settings_save_pending: bool,
    output_devices: Vec<OutputDevice>,
    hit_areas: Rc<HitAreas>,
    browser: Entity<Browser>,
    overviews: [Entity<DeckOverview>; DECK_COUNT],
    help_open: bool,
    notices: Notices,
    focus_handle: FocusHandle,
    last_frame: Instant,
    frame_stats: Option<Rc<RefCell<FrameStats>>>,
    /// What each deck's last pad press did, shown over its lane.
    pad_notes: [Option<PadNote>; DECK_COUNT],
    /// The scrolling waveforms' zoom, shared by both lanes so they keep one time axis.
    lane_zoom: Zoom,
    /// How far a pinch on the lanes has gone since it last changed the zoom.
    pinch: f32,
    /// What the menu bar was last built for.
    menu_state: Option<MenuState>,
    /// The window's size at the last frame, for the layout of small windows.
    window_size: gpui::Size<Pixels>,
    /// Whether the sound cards and controllers may be opened.
    devices: DeviceAccess,
    /// Whether the audio glitches often enough to tell the DJ.
    glitches: GlitchWatch,
    _subscriptions: Vec<Subscription>,
}

impl DubplateApp {
    /// Starts the audio engine, opens the browser and opens the library the DJ chose in the
    /// background.
    pub fn new(devices: DeviceAccess, window: &mut Window, cx: &mut Context<'_, Self>) -> Self {
        let this = cx.weak_entity();
        window.on_window_should_close(cx, move |_, cx| {
            this.update(cx, |this, cx| this.request_quit(cx)).is_err()
        });
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
                BrowserEvent::TookKeys => {
                    this.prepare_focus = PrepareFocus::Library;
                    cx.notify();
                }
                BrowserEvent::ImportRekordbox => this.set_rekordbox_import(true, cx),
                BrowserEvent::RelinkAll => this.start_relink(None, cx),
                BrowserEvent::ExportCrateToUsb(crate_index) => {
                    this.export_crate_to_usb(crate_index, cx);
                }
                BrowserEvent::Relink(track) => this.start_relink(Some(track), cx),
                BrowserEvent::ExportHistory { page, format } => {
                    this.export_history(page, format, cx);
                }
                BrowserEvent::SeparateStems(track) => {
                    if let Some(path) = this.track_file(track) {
                        this.separate_stems_now(path, cx);
                    }
                }
                BrowserEvent::CancelStems(track) => {
                    if let Some(path) = this.track_file(track) {
                        this.cancel_stems(path, cx);
                    }
                }
                BrowserEvent::Sorted { screen, sort } => {
                    this.settings
                        .library_sort
                        .set(screen, sort.map(SortSetting::of));
                    this.save_settings(cx);
                }
            });
        let effect_search = cx.new(|cx| InputState::new(window, cx).placeholder("Find an effect"));
        let quit_subscription = cx.on_app_quit(|this: &mut Self, cx| {
            this.finish_recording_on_quit();
            this.finish_plays(cx);
            let barrier = this
                .set_editor
                .as_ref()
                .and_then(|session| session.submit(|_| Ok(())).ok());
            let task = cx.background_executor().spawn(async move {
                if let Some(barrier) = barrier {
                    let _ = barrier.wait();
                }
            });
            async move {
                task.await;
            }
        });
        let search_subscription = cx.subscribe(
            &effect_search,
            |this, input, event: &InputEvent, cx| match event {
                InputEvent::Change => cx.notify(),
                InputEvent::PressEnter { .. } => {
                    let query = input.read(cx).value().to_string();
                    let picked = this.effect_highlighted(&picker_rows(&search(&query)));
                    if let (Some(menu), Some(kind)) = (this.effect_menu, picked) {
                        this.edit_effect(menu.unit, EffectEdit::Kind(kind), cx);
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
        let pad_fx = PadFxBanks::from_setting(&settings.pad_fx);
        let mixer = settings.levels.apply(MixerPositions {
            sampler_volume: settings.sampler.volume.clamp(0.0, 1.0),
            ..MixerPositions::default()
        });
        let mode = settings.screen();
        theme::wear(settings.theme(), cx);
        let lane_zoom = Zoom::from_setting(settings.lane_zoom);
        let (mut engine, audio_problem) = audio::start_engine(
            &mixer,
            settings.audio_device.as_deref(),
            BufferSize::from_setting(settings.buffer_frames),
            &devices,
        );
        if let Ok(engine) = &mut engine {
            for deck in DeckId::ALL {
                if let Err(problem) = engine
                    .controller
                    .set_key_lock(deck, settings.key_lock_on_new_tracks)
                {
                    problems.push(format!("Key lock could not be set: {problem}"));
                }
            }
        }
        problems.extend(audio_problem);
        let hardware = match &devices {
            DeviceAccess::Offline { script } => {
                if let Some(script) = script {
                    script::listen(script.clone(), window, cx);
                }
                Some(Hardware::offline())
            }
            DeviceAccess::Real => match Hardware::start() {
                Ok((hardware, events)) => {
                    listen_to_controllers(events, window, cx);
                    Some(hardware)
                }
                Err(error) => {
                    problems.push(format!("Controllers cannot be used: {error}"));
                    None
                }
            },
        };
        keep_lights_moving(cx);
        Self::keep_watching_plays(cx);
        let analysis_queue = Settings::analysis_cache_path()
            .map_err(|error| error.to_string())
            .and_then(|path| AnalysisQueue::start(path).map_err(|error| error.to_string()))
            .map_err(|problem| problems.push(format!("Tracks cannot be analysed: {problem}")))
            .ok();
        let stems = match Settings::stems_location() {
            Ok(location) => {
                let (stems, problem) = TrackStems::start(location);
                if let Some(problem) = problem {
                    problems.push(format!("Tracks cannot be separated into stems: {problem}"));
                }
                stems
            }
            Err(error) => {
                problems.push(format!("Tracks cannot be separated into stems: {error}"));
                TrackStems::start_unavailable()
            }
        };
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
            track_cues: HashMap::new(),
            lights_settling: false,
            effect_focus: EffectUnitId::ALL[0],
            mode,
            pad_modes: [PadMode::HotCue; DECK_COUNT],
            beat_jump_scales: [0; DECK_COUNT],
            held_pads: [None; DECK_COUNT],
            pad_fx,
            pad_fx_editor: None,
            shift_held: [false; DECK_COUNT],
            shift_layer_shown: [false; DECK_COUNT],
            key_pad_offsets: [0; DECK_COUNT],
            keyboard_cues: [HotCuePad::ALL[0]; DECK_COUNT],
            selected_samples: [0; DECK_COUNT],
            set_editor: None,
            track_grids: HashMap::new(),
            grid_bar: None,
            set_selection_generation: 0,
            set_selection: None,
            modal_focus: cx.focus_handle(),
            modal_return_focus: None,
            modal_active: false,
            quit_requested: false,
            quit_barrier_ready: false,
            set_edits_pending: 0,
            set_save_problem: None,
            quit_refused: false,
            prepare_focus: PrepareFocus::Library,
            next_cursor: 0,
            queued_suggestions: VecDeque::new(),
            set_scroll: ScrollHandle::new(),
            prepare_view: Arc::default(),
            prepare_generation: 0,
            track_checks: HashMap::new(),
            checks_pending: HashSet::new(),
            check_generation: 0,
            track_detail: None,
            detail_pending: None,
            waveform_cache: WaveformCache::default(),
            check_cursor_seconds: 0.0,
            catalogue: Arc::default(),
            match_source: None,
            missing: Arc::default(),
            relinking: false,
            relink_review: None,
            audition: None,
            audition_generation: 0,
            track_preview: None,
            preview_generation: 0,
            analyses: TrackAnalyses::new(
                analysis_queue.map(|queue| Box::new(queue) as Box<dyn TrackAnalyser>),
            ),
            analysis_collecting: false,
            stems,
            stems_changed: true,
            recording: None,
            recording_skip_reported: false,
            play_history: PlayHistory::new(SystemTime::now()),
            play_watches: Default::default(),
            time_zone: jiff::tz::TimeZone::system(),
            smart_cfx_menu: false,
            cfx_menu: None,
            effect_menu: None,
            effect_highlight: None,
            effect_picker_scroll: ScrollHandle::new(),
            fx_select_presses: 0,
            refocus_window: false,
            effect_search,
            settings_save_pending: false,
            output_devices: Vec::new(),
            hit_areas: Rc::default(),
            browser,
            overviews: [
                cx.new(|_| DeckOverview::default()),
                cx.new(|_| DeckOverview::default()),
            ],
            help_open: false,
            notices: Notices::default(),
            focus_handle,
            last_frame: Instant::now(),
            frame_stats: FrameStats::from_environment().map(|stats| Rc::new(RefCell::new(stats))),
            pad_notes: Default::default(),
            lane_zoom,
            pinch: 0.0,
            menu_state: None,
            window_size: window.viewport_size(),
            devices,
            glitches: GlitchWatch::default(),
            _subscriptions: vec![subscription, search_subscription, quit_subscription],
        };
        app.browser
            .update(cx, |browser, cx| browser.show_screen(mode, cx));
        app.restore_effect_units(cx);
        app.restore_jog_and_quantize(cx);
        app.restore_mixer_switches(cx);
        app.restore_smart_cfx(cx);
        let fader_echo = app.settings.fader_echo;
        app.engine_command(cx, |controller| controller.set_fader_echo(fader_echo));
        app.restore_sampler(cx);
        app.restore_library_sorts(cx);
        app.start_library(cx);
        app.start_set_session(cx);
        Self::keep_collecting_stems(cx);
        if let Some(problem) = problems.into_iter().next() {
            app.show_notice(NoticePlace::Window, problem, cx);
        }
        app
    }

    /// Follows a pinch on the lanes: each step of [`PINCH_STEP`] in or out zooms one level.
    fn pinch_lanes(&mut self, delta: f32, cx: &mut Context<'_, Self>) {
        self.pinch += delta;
        while self.pinch.abs() >= PINCH_STEP {
            let closer = self.pinch > 0.0;
            self.pinch -= PINCH_STEP.copysign(self.pinch);
            self.set_lane_zoom(self.lane_zoom.step(closer), cx);
        }
    }

    /// Zooms both scrolling waveforms to `zoom` and remembers it.
    fn set_lane_zoom(&mut self, zoom: Zoom, cx: &mut Context<'_, Self>) {
        if zoom == self.lane_zoom {
            return;
        }
        self.lane_zoom = zoom;
        self.settings.lane_zoom = Some(zoom.pixels_per_second());
        self.save_settings_soon(cx);
        cx.notify();
    }

    /// Builds the menu bar again when what it shows has changed: a set to export, recording,
    /// the rekordbox collection, missing files or the screen.
    fn refresh_menus(&mut self, cx: &mut Context<'_, Self>) {
        let state = MenuState {
            mode: self.mode,
            has_set: self
                .set_editor
                .as_ref()
                .is_some_and(|editor| !editor.set().entries.is_empty()),
            recording: self.is_recording(),
            rekordbox_imported: !matches!(self.sources.rekordbox, RekordboxSource::Off),
            missing_files: !self.missing.is_empty(),
        };
        if self.menu_state != Some(state) {
            self.menu_state = Some(state);
            cx.set_menus(menus::menus(crate::APP_NAME, state));
        }
    }

    /// Whether the window is too narrow for the header to spell out the controller and the
    /// sound card: they shrink to their status dots, named in tooltips.
    pub(super) fn narrow_window(&self) -> bool {
        self.window_size.width < NARROW_WINDOW_WIDTH
    }

    /// Whether the window is too short for full-height lanes: they get lower so the library
    /// keeps some rows.
    pub(super) fn short_window(&self) -> bool {
        self.window_size.height < SHORT_WINDOW_HEIGHT
    }

    /// Whether `control`'s twin on the controller is pressed, or was tapped a moment ago, so the
    /// control on screen shows the press.
    pub(super) fn shows_pressed(&self, control: HeldControl) -> bool {
        self.hardware
            .as_ref()
            .is_some_and(|hardware| hardware.dispatcher.held.shows(control, Instant::now()))
    }

    /// Turns Lock playing deck on or off and remembers it.
    pub(super) fn toggle_lock_playing_deck(&mut self, cx: &mut Context<'_, Self>) {
        self.lock_playing_deck = !self.lock_playing_deck;
        self.settings.lock_playing_deck = self.lock_playing_deck;
        self.save_settings(cx);
        cx.notify();
    }

    /// Flushes queued edits before asking GPUI to quit (its shutdown hook has a short timeout).
    /// Keeping the window alive lets slow storage finish without blocking event delivery.
    fn request_quit(&mut self, cx: &mut Context<'_, Self>) {
        if self.quit_requested {
            return;
        }
        self.quit_requested = true;
        self.quit_barrier_ready = false;
        self.finish_plays(cx);
        let barrier = self
            .set_editor
            .as_ref()
            .map(|session| session.submit(|_| Ok(())));
        match barrier {
            Some(Ok(job)) => {
                cx.spawn(async move |this, cx| {
                    let result = cx
                        .background_executor()
                        .spawn(async move { job.wait() })
                        .await;
                    this.update(cx, |this, cx| {
                        match result {
                            Ok(completion) => {
                                if let Err(error) = completion.outcome {
                                    this.report_set_save_failure(error, cx);
                                }
                            }
                            Err(error) => this.report_set_save_failure(error, cx),
                        }
                        this.quit_barrier_ready = true;
                        this.finish_quit_if_saved(cx);
                    })
                    .ok();
                })
                .detach();
            }
            Some(Err(error)) => {
                self.report_set_save_failure(error, cx);
                self.quit_barrier_ready = true;
                self.finish_quit_if_saved(cx);
            }
            None => {
                self.quit_barrier_ready = true;
                self.finish_quit_if_saved(cx);
            }
        }
    }

    /// Keeps the precise failure available even if another notice replaced the original one.
    fn report_set_save_failure(
        &mut self,
        error: impl std::fmt::Display,
        cx: &mut Context<'_, Self>,
    ) {
        self.set_save_problem = Some(error.to_string());
        self.show_notice(
            NoticePlace::Window,
            format!("The set was not saved: {error}"),
            cx,
        );
    }

    /// A worker barrier is not enough: every submitted edit's UI callback must also report
    /// its outcome before a successful barrier can allow the application to exit. A failed save
    /// keeps Dubplate open once, to be reviewed; quitting again then quits even if saving still
    /// fails, so a set store that broke for good never leaves the app impossible to quit.
    fn finish_quit_if_saved(&mut self, cx: &mut Context<'_, Self>) {
        if !self.quit_requested || !self.quit_barrier_ready || self.set_edits_pending != 0 {
            return;
        }
        match self.set_save_problem.take() {
            Some(problem) if !self.quit_refused => {
                self.quit_requested = false;
                self.quit_barrier_ready = false;
                self.quit_refused = true;
                self.show_notice(
                    NoticePlace::Window,
                    format!(
                        "Not everything was saved: {problem}. Dubplate stayed open; review the \
                         set and the cues before quitting again."
                    ),
                    cx,
                );
            }
            Some(_) | None => cx.quit(),
        }
    }

    /// Opens and migrates set storage off the UI thread, then starts its serialized worker.
    fn start_set_session(&mut self, cx: &mut Context<'_, Self>) {
        cx.spawn(async move |this, cx| {
            let opened = cx
                .background_executor()
                .spawn(async {
                    let path = Settings::sets_path().map_err(|error| error.to_string())?;
                    let editor = SetEditor::open(&path).map_err(|error| error.to_string())?;
                    let grids = editor.track_grids().map_err(|error| error.to_string())?;
                    let track_cues = editor.track_cues().map_err(|error| error.to_string())?;
                    let session = dubplate_prep::SetSession::new(editor)
                        .map_err(|error| error.to_string())?;
                    Ok::<_, String>((session, grids, track_cues))
                })
                .await;
            this.update(cx, |this, cx| match opened {
                Ok((session, grids, track_cues)) => {
                    this.set_editor = Some(session);
                    this.take_track_grids(grids, cx);
                    this.restore_play_history(cx);
                    this.restore_track_cues(track_cues);
                    this.set_changed(cx);
                    // The first library build may have completed before set storage opened.
                    this.rebuild_collection(cx);
                }
                Err(problem) => this.show_notice(
                    NoticePlace::Window,
                    format!("Sets cannot be kept: {problem}"),
                    cx,
                ),
            })
            .ok();
        })
        .detach();
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
        let track_index = match &source {
            LoadSource::Collection(index) => Some(*index),
            LoadSource::File(_) => None,
        };
        let (track, generation) = match self.begin_load(deck, source) {
            Ok(started) => started,
            Err(refusal @ LoadRefusal::FileMissing(_)) => {
                let tracks: Vec<TrackIndex> = track_index.into_iter().collect();
                self.report_missing_file(refusal.to_string(), &tracks, cx);
                return;
            }
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
            self.analyses.retry_failed(path);
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
                let audio = prepared
                    .as_ref()
                    .ok()
                    .map(|prepared| prepared.audio.clone());
                if !this.finish_load(deck, generation, prepared, cx) {
                    return;
                }
                match cue {
                    Some(seconds) => this.jump(deck, seconds, cx),
                    None => this.auto_cue(deck, generation, audio, cx),
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
            LoadSource::Collection(index) => {
                let track = Arc::clone(
                    self.collection()
                        .ok_or(LoadRefusal::LibraryNotReady)?
                        .track(index),
                );
                check_present(&track)?;
                track
            }
            LoadSource::File(path) => {
                check_file(&path)?;
                Arc::new(CollectionTrack::from_file(&path))
            }
        };
        check_deck_free(deck, self.deck_state(deck).playing, self.lock_playing_deck)?;
        let cues = self.cues_of(&track);
        let generation = self.decks[deck.index()].begin_load(Arc::clone(&track), cues);
        Ok((track, generation))
    }

    fn finish_load(
        &mut self,
        deck: DeckId,
        generation: u64,
        prepared: Result<PreparedTrack, LoadError>,
        cx: &mut Context<'_, Self>,
    ) -> bool {
        if !self.deck(deck).is_current_load(generation) {
            return false;
        }
        // A DJ may have resumed the old track while this replacement decoded.
        if let Err(refusal) =
            check_deck_free(deck, self.deck_state(deck).playing, self.lock_playing_deck)
        {
            self.decks[deck.index()].fail_load(generation, refusal.to_string());
            self.show_notice(NoticePlace::Deck(deck), refusal.to_string(), cx);
            return false;
        }
        let grid_source = prepared
            .as_ref()
            .ok()
            .and_then(|prepared| prepared.grid_source);
        let loaded = match prepared.and_then(|prepared| self.put_on_deck(deck, prepared)) {
            Ok(visuals) => {
                let needs_waveform = visuals.zoomed.is_none();
                self.decks[deck.index()].commit_load(generation, visuals, grid_source);
                self.reset_deck_stems(deck, cx);
                self.signal_controllers(Feedback::TrackLoaded(controller_deck(deck)));
                self.give_deck_analysed_grid(deck, cx);
                self.refresh_filter_reference(cx);
                if needs_waveform {
                    self.draw_waveform_from_audio(deck, generation, cx);
                }
                true
            }
            Err(error) => {
                let problem = error_chain(&error);
                self.decks[deck.index()].fail_load(generation, problem.clone());
                self.show_notice(NoticePlace::Deck(deck), problem, cx);
                false
            }
        };
        self.update_lights();
        cx.notify();
        loaded
    }

    fn put_on_deck(
        &mut self,
        deck: DeckId,
        mut prepared: PreparedTrack,
    ) -> Result<TrackVisuals, LoadError> {
        let key_lock = self.settings.key_lock_on_new_tracks;
        let engine = self
            .engine
            .as_mut()
            .map_err(|problem| LoadError::Audio(EngineError::Output(problem.to_string())))?;
        let visuals = TrackVisuals::new(&mut prepared);
        engine
            .controller
            .load_prepared_track(deck, prepared.audio, prepared.beat_grid)?;
        engine.controller.set_key_lock(deck, key_lock)?;
        // Loading clears transient pad gestures on the audio thread in the same command.
        self.held_pads[deck.index()] = None;
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
            Command::TogglePlayOnAir => {
                self.perform(Command::TogglePlay(self.space_deck()), window, cx);
                return;
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
            Command::ToggleQuantize => self.toggle_quantize(cx),
            Command::ToggleLockPlayingDeck => self.toggle_lock_playing_deck(cx),
            Command::ToggleVinylMode(deck) => self.toggle_vinyl_mode(deck, cx),
            Command::Loop(deck, action) => self.loop_button(deck, action, cx),
            Command::CueLoopCall(deck, direction) => self.cue_loop_call(deck, direction, cx),
            Command::StoreMemoryCue(deck) => self.store_memory_cue(deck, cx),
            Command::DeleteMemoryCue(deck) => self.delete_memory_cue(deck, cx),
            Command::ToggleHeadphoneCue(deck) => {
                let cued = self
                    .headphones()
                    .is_some_and(|headphones| headphones.is_cued(deck));
                self.engine_command(cx, |controller| controller.set_headphone_cue(deck, !cued));
            }
            Command::TapTempo(deck) => self.tap_tempo(deck, cx),
            Command::ToggleGridBar(deck) => self.toggle_grid_bar(deck),
            Command::EditDeckGrid(deck, edit) => self.edit_deck_grid(deck, edit, cx),
            Command::AuditionSeam => self.toggle_seam_audition(cx),
            Command::ToggleRecording => self.toggle_recording(cx),
            Command::ToggleSmartCfx => self.toggle_smart_cfx(cx),
            Command::NextSmartCfx => {
                let next = self.smart_cfx_preset().next();
                self.choose_smart_cfx(next, cx);
            }
            Command::ChooseSmartCfx(preset) => self.choose_smart_cfx(preset, cx),
            Command::ToggleCfxMenu => {
                self.cfx_menu = match self.cfx_menu {
                    Some(_) => None,
                    None => Some(CfxScope::default()),
                };
            }
            Command::SetCfxScope(scope) => self.cfx_menu = Some(scope),
            Command::ChooseCfxColour(colour) => {
                let scope = self.cfx_menu.take().unwrap_or_default();
                self.choose_cfx_colour(scope.decks(), colour, cx);
            }
            Command::ToggleSmartFader => self.toggle_smart_fader(cx),
            Command::ToggleMasterCue => {
                let cued = self
                    .headphones()
                    .is_some_and(|headphones| headphones.master_cue);
                self.engine_command(cx, |controller| controller.set_master_cue(!cued));
            }
            Command::PreviewSelected => self.toggle_track_preview(cx),
            Command::LoadSelected(deck) => {
                if let Some(track) = self.selected_track(cx) {
                    self.load(deck, LoadSource::Collection(track), cx);
                }
            }
            Command::RevealSelected => self.reveal_selected(cx),
            Command::CopySelectedPath => self.copy_selected_path(cx),
            Command::SeparateSelectedStems => self.separate_selected_stems(cx),
            Command::RelinkSelected => self.relink_selected(cx),
            Command::InstantDouble(deck) => self.instant_double(deck, cx),
            Command::NudgeTempo(deck, direction) => {
                let slider = nudged_tempo_slider(
                    self.control_fraction(Control::Tempo(deck)),
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
            Command::SetKeyLockOnNewTracks(on) => {
                self.settings.key_lock_on_new_tracks = on;
                self.save_settings(cx);
            }
            Command::SetFaderEcho(on) => {
                self.settings.fader_echo = on;
                self.save_settings(cx);
                self.engine_command(cx, |controller| controller.set_fader_echo(on));
            }
            Command::Stems(deck, action) => self.apply_stem_action(deck, action, cx),
            Command::ToggleKeyLock(deck) => {
                let locked = self.key_lock(deck);
                self.engine_command(cx, |controller| controller.set_key_lock(deck, !locked));
            }
            Command::KeySync(deck) => self.toggle_key_sync(deck, cx),
            Command::CycleTempoRange(deck) => {
                let range = self.tempo_range(deck).next();
                self.engine_command(cx, |controller| controller.set_tempo_range(deck, range));
            }
            Command::SetControl(control, fraction) => self.set_control(control, fraction, cx),
            Command::MoveCrossfader(direction) => {
                let position = self.mixer.crossfader + CROSSFADER_STEP * direction.sign() as f32;
                self.set_control(Control::Crossfader, position.clamp(0.0, 1.0), cx);
            }
            Command::ZoomLanes(direction) => {
                self.set_lane_zoom(self.lane_zoom.step(direction == Direction::Up), cx);
            }
            Command::ResetLaneZoom => self.set_lane_zoom(Zoom::DEFAULT, cx),
            Command::SetCrossfaderAssign(deck, assign) => {
                self.set_crossfader_assign(deck, assign, cx);
            }
            Command::CycleCrossfaderAssign(deck) => self.cycle_crossfader_assign(deck, cx),
            Command::ToggleCrossfader => self.toggle_crossfader(cx),
            Command::SetSmartFaderSource(source) => self.set_smart_fader_source(source, cx),
            Command::SetCrossfaderCurve(curve) => self.set_crossfader_curve(curve, cx),
            Command::SetChannelFaderCurve(curve) => self.set_channel_fader_curve(curve, cx),
            Command::MoveSelection(direction) => match (self.mode, self.prepare_focus) {
                (Mode::Prepare, PrepareFocus::Set) => {
                    self.move_set_selection(direction, cx);
                    self.ensure_track_detail(cx);
                }
                (Mode::Prepare, PrepareFocus::Next) => self.move_next_cursor(direction),
                _ => self
                    .browser
                    .update(cx, |browser, cx| browser.move_selection(direction, cx)),
            },
            Command::SwitchMode(mode) => {
                self.mode = mode;
                self.settings.set_screen(mode);
                self.save_settings(cx);
                self.browser
                    .update(cx, |browser, cx| browser.show_screen(mode, cx));
            }
            Command::EditSet(edit) => self.edit_set(edit, cx),
            Command::SwitchPrepareFocus(direction) => {
                if self.mode == Mode::Prepare {
                    self.prepare_focus = self.prepare_focus.step(direction);
                    self.next_cursor = 0;
                    window.focus(&self.focus_handle, cx);
                }
            }
            Command::LoadSet => self.load_set(cx),
            Command::ExportSet => self.export_set(cx),
            Command::ExportSetToUsb => self.export_set_to_usb(cx),
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
                self.follow_beat_fx_section(cx);
            }
            Command::FocusSearch => self
                .browser
                .update(cx, |browser, cx| browser.focus_search(window, cx)),
            Command::ToggleHelp => {
                self.help_open = !self.help_open;
                self.library_open = false;
            }
            Command::ChooseTheme(choice) => {
                theme::wear(choice, cx);
                self.settings.set_theme(choice);
                self.save_settings(cx);
            }
            Command::ToggleAutoCue(choice) => {
                let setting = &mut self.settings.auto_cue;
                match choice {
                    AutoCueChoice::SavedCuesFirst => {
                        setting.saved_cues_first = !setting.saved_cues_first;
                    }
                    AutoCueChoice::FirstSound => setting.first_sound = !setting.first_sound,
                }
                self.save_settings(cx);
                cx.notify();
            }
            Command::ImportRekordbox => self.set_rekordbox_import(true, cx),
            Command::RelinkMissingFiles => self.start_relink(None, cx),
            Command::ToggleLibrary => {
                self.library_open = !self.library_open;
                if self.library_open {
                    self.refresh_output_devices(cx);
                }
                self.help_open = false;
            }
            Command::AddMusicFolders => self.add_music_folders(cx),
            Command::ShiftHeld(deck, held) => self.shift_held[deck.index()] = held,
            Command::TogglePadLayer(deck) => {
                let shown = &mut self.shift_layer_shown[deck.index()];
                *shown = !*shown;
            }
            Command::PadFx(edit) => self.edit_pad_fx(edit, cx),
            Command::Dismiss => {
                if self.effect_menu.is_some() {
                    self.close_effect_menu(cx);
                } else if self.pad_fx_editor.is_some() {
                    self.pad_fx_editor = None;
                } else if self.relink_review.is_some() {
                    self.close_relink_review(cx);
                } else if self.help_open || self.library_open {
                    self.help_open = false;
                    self.library_open = false;
                } else if self.notices.is_showing() {
                    self.notices.dismiss();
                } else if self.browser.read(cx).search_has_focus(window, cx) {
                    self.browser
                        .update(cx, |browser, cx| browser.focus_table(window, cx));
                }
            }
        }
        self.refresh_filter_reference(cx);
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
    /// playing or not, and a hot loop starts its loop; an empty pad keeps the loop that is on
    /// as a hot loop, else takes the beat nearest to what the DJ hears now.
    fn press_hot_cue(&mut self, deck: DeckId, pad: HotCuePad, cx: &mut Context<'_, Self>) {
        if self.deck(deck).visuals().is_none() {
            return;
        }
        let called = self
            .deck(deck)
            .cues
            .hot_cue(pad)
            .map(|cue| (cue.start_seconds(), cue.loop_end_seconds()));
        match called {
            Some((start, loop_end)) => {
                self.decks[deck.index()].cue_preview = false;
                self.engine_command(cx, |controller| match loop_end {
                    Some(end) => controller.hot_loop(deck, start, end),
                    None => controller.hot_cue(deck, start),
                });
            }
            None => {
                let heard = self.quantized(deck, self.heard_seconds(deck));
                let (start_ms, loop_end_ms) = new_cue_span(self.active_loop(deck), heard);
                let cue = HotCue {
                    loop_end_ms,
                    ..HotCue::at(start_ms)
                };
                self.edit_cues(deck, |cues| cues.set_hot_cue(pad, cue), cx);
            }
        }
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

    /// `seconds` on the beat nearest to it while quantize is on and the track has a beat grid,
    /// as with rekordbox's quantize; `seconds` itself with quantize off.
    fn quantized(&self, deck: DeckId, seconds: f64) -> f64 {
        if !self.quantize() {
            return seconds;
        }
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
        let cues = Arc::clone(&self.deck(source).cues);
        let mut doubled = false;
        self.engine_command(cx, |controller| {
            doubled = transport::double(controller, source, deck)?;
            Ok(())
        });
        if !doubled {
            return;
        }
        let grid_source = self.deck(source).grid_source;
        let needs_waveform = visuals.zoomed.is_none();
        let side = &mut self.decks[deck.index()];
        let generation = side.begin_load(track, cues);
        side.commit_load(generation, visuals, grid_source);
        if needs_waveform {
            self.draw_waveform_from_audio(deck, generation, cx);
        }
        self.mixer.tempo_sliders[deck.index()] = self.mixer.tempo_sliders[source.index()];
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
                self.remember_levels(cx);
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
                self.remember_levels(cx);
                self.engine_command(cx, |controller| controller.set_headphone_mix(fraction));
            }
            Control::HeadphoneLevel => {
                self.mixer.headphone_level = fraction;
                self.remember_levels(cx);
                self.engine_command(cx, |controller| controller.set_headphone_level(fraction));
            }
            Control::StemLevel(deck, stem) => self.set_stem_level(deck, stem, fraction, cx),
            Control::SamplerVolume => {
                self.mixer.sampler_volume = fraction;
                self.settings.sampler.volume = fraction;
                self.engine_command(cx, |controller| controller.set_sampler_volume(fraction));
                self.save_settings_soon(cx);
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
                    if let Some(hardware) = self.hardware.as_mut()
                        && !hardware.is_connected()
                    {
                        hardware.dispatcher.forget_positions();
                    }
                    self.show_notice(NoticePlace::Window, format!("{name} disconnected"), cx);
                }
                LinkEvent::Problem(problem) => {
                    self.show_notice(NoticePlace::Window, problem, cx);
                }
                LinkEvent::Control { control, received } => {
                    let surface = Surface {
                        positions: self.shown_positions(),
                        playing: DeckId::ALL.map(|deck| self.deck_state(deck).playing),
                        focused_effect: Some(self.effect_focus),
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
        if let DeckContent::Loaded(visuals) = &mut deck_state.content {
            visuals.read_grid = None;
            if deck_state.grid_source == Some(GridSource::Analysis) {
                visuals.beats = Arc::default();
            }
        }
        deck_state.grid_source = Some(GridSource::Tapped);
        self.engine_command(cx, |controller| controller.set_beat_grid(deck, Some(grid)));
    }

    /// The DJ's fix of a track's grid.
    fn grid_correction(&self, track: &CollectionTrack) -> GridCorrection {
        track
            .track
            .file_path
            .as_deref()
            .map(|path| self.track_grid(path).correction)
            .unwrap_or_default()
    }

    /// The deck Space plays or pauses now.
    pub(super) fn space_deck(&self) -> DeckId {
        space_deck(
            DeckId::ALL.map(|deck| DeckOnAir {
                channel_fader: self.mixer.channel_faders[deck.index()],
                is_tempo_master: self.tempo_state(deck).is_some_and(|state| state.is_master),
            }),
            &self.crossfader(),
            self.channel_fader_curve(),
        )
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
            EffectEdit::Parameter(direction) => {
                match step_parameter(state.kind, state.parameter, direction) {
                    Some(value) => controller.set_effect_parameter(unit, value),
                    None => Ok(()),
                }
            }
            EffectEdit::Target(target) => controller.set_effect_target(unit, target),
            EffectEdit::Depth(depth) => controller.set_effect_depth(unit, depth.clamp(0.0, 1.0)),
            EffectEdit::Toggle => controller.toggle_effect(unit),
            EffectEdit::Release(held) => controller.hold_release_fx(unit, held),
            EffectEdit::NextRelease => controller.set_release_fx(unit, state.release.next()),
        });
        if !matches!(edit, EffectEdit::Toggle | EffectEdit::Release(_)) {
            self.remember_effect_units(cx);
        }
        if matches!(edit, EffectEdit::StepKind(_)) {
            self.glance_effect_menu(unit, cx);
        }
        cx.notify();
    }

    /// Gives the unit the controller's BEAT FX section drives the lever's target and the knob's
    /// level, as far as the controller has reported them.
    fn follow_beat_fx_section(&mut self, cx: &mut Context<'_, Self>) {
        let Some(section) = self
            .hardware
            .as_ref()
            .map(|hardware| hardware.dispatcher.beat_fx_section())
        else {
            return;
        };
        for command in section.commands_for(self.effect_focus) {
            if let Command::Effect(unit, edit) = command {
                self.edit_effect(unit, edit, cx);
            }
        }
    }

    /// Where the CH SELECT lever of the controller's BEAT FX section sits, when a controller is
    /// connected.
    pub(super) fn beat_fx_lever(&self) -> Option<Lever> {
        self.hardware
            .as_ref()
            .filter(|hardware| hardware.is_connected())
            .map(
                |hardware| match hardware.dispatcher.beat_fx_section().lever {
                    Some(target) => Lever::At(target),
                    None => Lever::Unreported,
                },
            )
    }

    /// Shows `unit`'s effect picker after FX SELECT, with the new effect highlighted, without
    /// taking the keyboard, and closes it a moment after the last press. A picker the DJ opened
    /// with the pointer stays as it is.
    fn glance_effect_menu(&mut self, unit: EffectUnitId, cx: &mut Context<'_, Self>) {
        if self
            .effect_menu
            .is_some_and(|menu| menu.opening == MenuOpening::Pointer)
        {
            return;
        }
        self.fx_select_presses += 1;
        let press = self.fx_select_presses;
        self.effect_menu = Some(EffectMenu {
            unit,
            opening: MenuOpening::FxSelect,
        });
        self.scroll_picker_to(
            self.effect_unit(unit).map(|state| state.kind),
            &picker_rows(&search("")),
        );
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(FX_SELECT_GLANCE).await;
            this.update(cx, |this, cx| {
                let still_glancing = this
                    .effect_menu
                    .is_some_and(|menu| menu.opening == MenuOpening::FxSelect);
                if still_glancing && this.fx_select_presses == press {
                    this.effect_menu = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Opens the effect picker of `unit` with the cursor in its search field, or closes it.
    pub(super) fn toggle_effect_menu(
        &mut self,
        unit: EffectUnitId,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if self
            .effect_menu
            .is_some_and(|menu| menu.unit == unit && menu.opening == MenuOpening::Pointer)
        {
            self.close_effect_menu(cx);
            return;
        }
        self.effect_menu = Some(EffectMenu {
            unit,
            opening: MenuOpening::Pointer,
        });
        self.effect_highlight = None;
        self.scroll_picker_to(
            self.effect_unit(unit).map(|state| state.kind),
            &picker_rows(&search("")),
        );
        self.effect_search.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    /// Closes the effect picker and gives the keys back to the window.
    /// The effect the picker's highlight is on among `rows`: the one the arrows moved to, else
    /// the unit's own, else the first shown.
    pub(super) fn effect_highlighted(&self, rows: &[PickerRow]) -> Option<EffectKind> {
        let shown = |kind: &EffectKind| picker_position(rows, *kind).is_some();
        let own = self
            .effect_menu
            .and_then(|menu| self.effect_unit(menu.unit))
            .map(|state| state.kind);
        self.effect_highlight
            .filter(shown)
            .or(own.filter(shown))
            .or_else(|| rows.first().and_then(|row| row.effects.first().copied()))
    }

    /// Moves the picker's highlight with an arrow key and scrolls it into view.
    pub(super) fn move_effect_highlight(
        &mut self,
        direction: PickerMove,
        cx: &mut Context<'_, Self>,
    ) {
        let query = self.effect_search.read(cx).value().to_string();
        let rows = picker_rows(&search(&query));
        if let Some(from) = self.effect_highlighted(&rows) {
            self.effect_highlight = step_highlight(&rows, from, direction);
            self.scroll_picker_to(self.effect_highlight, &rows);
            cx.notify();
        }
    }

    /// Scrolls the picker's list so `kind`'s row shows.
    fn scroll_picker_to(&self, kind: Option<EffectKind>, rows: &[PickerRow]) {
        if let Some((row, _)) = kind.and_then(|kind| picker_position(rows, kind)) {
            self.effect_picker_scroll.scroll_to_item(row);
        }
    }

    pub(super) fn close_effect_menu(&mut self, cx: &mut Context<'_, Self>) {
        if let Some(menu) = self.effect_menu.take() {
            if menu.opening == MenuOpening::Pointer {
                self.refocus_window = true;
            }
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
                if let Some(value) = setting.parameter() {
                    controller.set_effect_parameter(unit, value)?;
                }
                if let Some(release) = setting.release() {
                    controller.set_release_fx(unit, release)?;
                }
                match setting.depth() {
                    Some(depth) => controller.set_effect_depth(unit, depth),
                    None => Ok(()),
                }
            });
        }
    }

    /// Remembers the effect units' settings.
    fn remember_effect_units(&mut self, cx: &mut Context<'_, Self>) {
        let units: Vec<EffectUnitSetting> = EffectUnitId::ALL
            .into_iter()
            .filter_map(|unit| self.effect_unit(unit))
            .map(|state| EffectUnitSetting::of(&state))
            .collect();
        if units.len() == EFFECT_UNIT_COUNT {
            self.settings.effect_units = units;
        }
        self.save_settings_soon(cx);
    }

    /// Remembers the master and headphone levels, so the next launch opens no louder.
    fn remember_levels(&mut self, cx: &mut Context<'_, Self>) {
        self.settings.levels = LevelsSetting::of(&self.mixer);
        self.save_settings_soon(cx);
    }

    /// Saves the settings a moment after the last change, so turning a knob writes the file
    /// once rather than on every step.
    fn save_settings_soon(&mut self, cx: &mut Context<'_, Self>) {
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
            shift_pads: self.stem_pad_lights(deck),
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
            vinyl_mode: self.vinyl_mode(deck),
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
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if click_count == 2 {
            let control = Control::from(slider);
            self.set_control(control, control.resting(), cx);
            return;
        }
        self.dragging = Some(Drag::Slider(slider));
        self.drag_to(Drag::Slider(slider), position, window, cx);
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
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        self.dragging = Some(Drag::EffectLevel(unit));
        self.drag_to(Drag::EffectLevel(unit), position, window, cx);
    }

    fn drag_to(
        &mut self,
        drag: Drag,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        match drag {
            Drag::Lane(lane) => self.drag_lane(lane, position, window, cx),
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
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(drag) = self.dragging else {
            return;
        };
        if event.pressed_button == Some(MouseButton::Left) {
            self.drag_to(drag, event.position, window, cx);
        } else {
            self.end_drag(window, cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, window: &mut Window, cx: &mut Context<'_, Self>) {
        self.end_drag(window, cx);
    }

    fn end_drag(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        if let Some(Drag::Lane(lane)) = self.dragging.take() {
            self.end_lane_drag(lane, window, cx);
        }
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
        for (side, frame) in self.decks.iter_mut().zip(&frames) {
            notice_new_cue(side, frame.state.cue_point_seconds, now);
        }
        let pulsing = self.decks.iter().any(|side| side.marker_pulse.is_some())
            || self
                .hardware
                .as_ref()
                .is_some_and(|hardware| hardware.dispatcher.held.any_shown(now))
            || self.pad_notes.iter().flatten().any(|note| note.shows(now));
        let moving = pulsing
            || frames.iter().any(|frame| frame.state.playing)
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
}

/// Starts a pulse where a cue was just set on `side`, and ends a pulse that is over.
fn notice_new_cue(side: &mut Deck, cue_point: f64, now: Instant) {
    if side.visuals().is_none() {
        side.seen_marks = None;
        side.marker_pulse = None;
        return;
    }
    let marks = SeenMarks {
        load_generation: side.load_generation(),
        cue_point,
        cues: Arc::clone(&side.cues),
    };
    if let Some(seconds) = side
        .seen_marks
        .replace(marks.clone())
        .and_then(|seen| seen.newly_set(&marks))
    {
        side.marker_pulse = Some(MarkerPulse {
            seconds,
            started: now,
        });
    }
    if side
        .marker_pulse
        .is_some_and(|pulse| pulse.strength(now).is_none())
    {
        side.marker_pulse = None;
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
/// Where a connected controller's CH SELECT lever sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Lever {
    /// The controller has not said yet; it will when the lever moves.
    Unreported,
    /// The lever sends the effect to this target.
    At(EffectTarget),
}

/// An open effect picker and what opened it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct EffectMenu {
    /// The unit whose effect it picks.
    pub(super) unit: EffectUnitId,
    /// What opened it.
    pub(super) opening: MenuOpening,
}

/// What opened an effect picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MenuOpening {
    /// The pointer: the picker searches as the DJ types and stays until closed.
    Pointer,
    /// FX SELECT on the controller: the picker shows the effect it chose, leaves the keyboard
    /// alone and closes by itself.
    FxSelect,
}

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
        let modal_open = self.help_open
            || self.library_open
            || self.relink_review.is_some()
            || self.pad_fx_editor.is_some();
        if modal_open && !self.modal_active {
            self.modal_return_focus = window.focused(cx);
            window.focus(&self.modal_focus, cx);
        } else if !modal_open && self.modal_active {
            window.focus(
                self.modal_return_focus
                    .as_ref()
                    .unwrap_or(&self.focus_handle),
                cx,
            );
            self.modal_return_focus = None;
        } else if !modal_open && std::mem::take(&mut self.refocus_window) {
            window.focus(&self.focus_handle, cx);
        }
        if modal_open {
            self.refocus_window = false;
        }
        self.modal_active = modal_open;
        self.refresh_menus(cx);
        self.window_size = window.viewport_size();
        let frames = self.advance_frame(window);
        if self.glitches.observe(self.dropouts(), Instant::now()) {
            self.show_notice(NoticePlace::Window, GLITCH_NOTICE, cx);
        }
        self.follow_library_matches(cx);
        let root = div()
            .key_context(WINDOW_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &crate::Quit, _, cx| this.request_quit(cx)))
            .on_action(|_: &menus::Minimize, window, _| window.minimize_window())
            .on_action(|_: &menus::Zoom, window, _| window.zoom_window())
            .on_action(|_: &menus::ToggleFullScreen, window, _| window.toggle_fullscreen())
            .on_action(cx.listener(|this, action: &Perform, window, cx| {
                this.perform(action.0, window, cx);
            }))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme::active().background)
            .text_color(theme::active().text)
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
                            .border_color(theme::active().line)
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
            .children(notice::render_toast(self, cx))
            .when(
                self.help_open && !self.library_open && self.relink_review.is_none(),
                |root| root.child(help::render(self, cx)),
            )
            .when_some(
                self.relink_review.as_ref().filter(|_| !self.library_open),
                |root, review| root.child(relink::render(self, review, cx)),
            )
            .when_some(
                self.pad_fx_editor
                    .filter(|_| !self.library_open && !self.help_open),
                |root, editing| root.child(pad_fx_editor::render(self, editing, cx)),
            )
            .when(self.library_open, |root| {
                root.child(library_panel::render(self, cx))
            });
        if let Some(stats) = &self.frame_stats {
            stats.borrow_mut().record_render(render_started.elapsed());
        }
        root
    }
}
