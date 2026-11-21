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
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dubplate_engine::{
    AudioOutput, Controller, DECK_COUNT, DeckId, DeckState, EngineError, OutputConfig,
};
use dubplate_rekordbox::HotCuePad;
use gpui::prelude::*;
use gpui::{
    Bounds, Context, Entity, FocusHandle, MouseButton, MouseMoveEvent, MouseUpEvent, Pixels, Point,
    SharedString, StyleRefinement, Subscription, Window, div, px,
};

use crate::browser::{Browser, BrowserEvent, LibraryStatus};
use crate::collection::{Collection, CollectionTrack};
use crate::controls::{
    CROSSFADER_STEP, Slider, SliderPositions, fraction_at, master_gain_at, nudged_tempo, tempo_at,
    tempo_fraction,
};
use crate::deck::{Deck, DeckContent, LoadError, PreparedTrack, TrackVisuals, prepare_track};
use crate::fonts::UI_FAMILY;
use crate::frame_stats::FrameStats;
use crate::loading::{LoadRefusal, LoadSource, check_deck_free, check_file};
use crate::meter::PeakMeter;
use crate::shortcuts::{Command, Perform, WINDOW_CONTEXT};
use crate::theme;
use crate::waveform::Zoom;

/// Frames per device buffer: about 5 ms at 48 kHz.
const BUFFER_FRAMES: u32 = 256;
/// How long a notice stays on screen.
const NOTICE_DURATION: Duration = Duration::from_secs(6);

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
    mixer: SliderPositions,
    dragging: Option<Slider>,
    header_drag_armed: bool,
    lock_playing_deck: bool,
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
        let mixer = SliderPositions::default();
        let engine = start_engine(&mixer);
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        let app = Self {
            engine,
            library: LibraryStatus::Reading,
            decks: Default::default(),
            deck_meters: [PeakMeter::default(); DECK_COUNT],
            master_meter: PeakMeter::default(),
            mixer,
            dragging: None,
            header_drag_armed: false,
            lock_playing_deck: true,
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
        app
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
        let generation = self.decks[deck.index()].begin_load(Arc::clone(&track));
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
            Ok(visuals) => DeckContent::Loaded(visuals),
            Err(error) => DeckContent::Failed(error_chain(&error)),
        };
        self.decks[deck.index()].content = content;
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

    fn perform(&mut self, command: Command, window: &mut Window, cx: &mut Context<'_, Self>) {
        match command {
            Command::TogglePlay(deck) => {
                self.engine_command(cx, |controller| controller.toggle_play(deck));
            }
            Command::Cue(deck) => self.engine_command(cx, |controller| controller.cue(deck)),
            Command::HotCue(deck, pad) => self.jump_to_hot_cue(deck, pad, cx),
            Command::LoadSelected(deck) => {
                if let Some(track) = self.browser.read(cx).selected() {
                    self.load(deck, LoadSource::Collection(track), cx);
                }
            }
            Command::NudgeTempo(deck, direction) => {
                let tempo = nudged_tempo(self.mixer.tempos[deck.index()], direction.sign());
                self.set_slider(Slider::Tempo(deck), tempo_fraction(tempo), cx);
            }
            Command::MoveCrossfader(direction) => {
                let position = self.mixer.crossfader + CROSSFADER_STEP * direction.sign() as f32;
                self.set_slider(Slider::Crossfader, position.clamp(0.0, 1.0), cx);
            }
            Command::MoveSelection(direction) => self
                .browser
                .update(cx, |browser, cx| browser.move_selection(direction, cx)),
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
        cx.notify();
    }

    fn jump_to_hot_cue(&mut self, deck: DeckId, pad: HotCuePad, cx: &mut Context<'_, Self>) {
        let deck_side = self.deck(deck);
        if deck_side.visuals().is_none() {
            return;
        }
        let Some(seconds) = deck_side
            .track
            .as_ref()
            .and_then(|track| track.hot_cue(pad))
            .map(|cue| cue.seconds)
        else {
            return;
        };
        self.jump(deck, seconds, cx);
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
        let playing = self.deck_state(deck).playing;
        self.engine_command(cx, |controller| {
            if !playing {
                controller.set_cue_point(deck, seconds)?;
            }
            controller.seek(deck, seconds)
        });
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

    fn slider_fraction(&self, slider: Slider) -> f32 {
        match slider {
            Slider::ChannelFader(deck) => self.mixer.channel_faders[deck.index()],
            Slider::Crossfader => self.mixer.crossfader,
            Slider::Tempo(deck) => tempo_fraction(self.mixer.tempos[deck.index()]),
            Slider::MasterLevel => self.mixer.master_level,
        }
    }

    fn set_slider(&mut self, slider: Slider, fraction: f32, cx: &mut Context<'_, Self>) {
        match slider {
            Slider::ChannelFader(deck) => {
                self.mixer.channel_faders[deck.index()] = fraction;
                self.engine_command(cx, |controller| {
                    controller.set_channel_fader(deck, fraction)
                });
            }
            Slider::Crossfader => {
                self.mixer.crossfader = fraction;
                self.engine_command(cx, |controller| controller.set_crossfader(fraction));
            }
            Slider::Tempo(deck) => {
                let tempo = tempo_at(fraction);
                self.mixer.tempos[deck.index()] = tempo;
                self.engine_command(cx, |controller| controller.set_tempo(deck, tempo));
            }
            Slider::MasterLevel => {
                self.mixer.master_level = fraction;
                self.engine_command(cx, |controller| {
                    controller.set_master_gain(master_gain_at(fraction))
                });
            }
        }
        cx.notify();
    }

    fn begin_drag(
        &mut self,
        slider: Slider,
        position: Point<Pixels>,
        click_count: usize,
        cx: &mut Context<'_, Self>,
    ) {
        if click_count == 2 {
            let resting = match slider {
                Slider::ChannelFader(_) | Slider::MasterLevel => 1.0,
                Slider::Crossfader | Slider::Tempo(_) => 0.5,
            };
            self.set_slider(slider, resting, cx);
            return;
        }
        self.dragging = Some(slider);
        self.drag_to(slider, position, cx);
    }

    fn drag_to(&mut self, slider: Slider, position: Point<Pixels>, cx: &mut Context<'_, Self>) {
        if let Some(bounds) = self.hit_areas.sliders[slider.index()].get() {
            self.set_slider(
                slider,
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

fn start_engine(mixer: &SliderPositions) -> Result<Engine, SharedString> {
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
