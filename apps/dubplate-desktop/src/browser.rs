//! The browser: the crates on the left and the tracks of the chosen crate on the right, filtered
//! by the search field and the quick filters, and sorted by the column the DJ clicked. It asks
//! the window to load tracks through [`BrowserEvent`].

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use dubplate_engine::DeckId;
use dubplate_prep::{AudioAnalysis, Key};
use dubplate_stems::StemsStatus;
use gpui::component::input::{Input, InputEvent, InputState};
use gpui::prelude::*;
use gpui::{
    App, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable, MouseButton,
    MouseDownEvent, Pixels, ScrollStrategy, SharedString, Subscription, UniformListScrollHandle,
    Window, div, px, svg, uniform_list,
};

use crate::analysis::{AnalysisAnswer, AnalysisState, bpm_cell, energy_cell, key_cell};
use crate::assets::{ARROW_DOWN_PATH, ARROW_UP_PATH, CLOSE_ICON_PATH, SEARCH_ICON_PATH};
use crate::collection::{Collection, CollectionTrack, CrateIndex, CrateKind, TrackIndex};
use crate::commands::{Direction, Mode};
use crate::deck::deck_letter;
use crate::fonts::MONO_FAMILY;
use crate::library_match::{LibraryMatches, TrackMatch};
use crate::library_view::{
    BpmRange, Column, FilterReference, LibraryView, RangeEnd, Sort, SortDirection,
};
use crate::match_cell::MatchCell;
use crate::search::{SearchQuery, filter_tracks};
use crate::search_filter::{SuggestionContext, TrackFacts, complete, suggestions};
use crate::stems::{StemStatuses, stems_cell};
use crate::theme;
use crate::widgets::{Emphasis, InteractiveControl, button, tooltip};

const ROW_HEIGHT: Pixels = px(32.0);
const SIDEBAR_WIDTH: Pixels = px(200.0);
const BPM_WIDTH: Pixels = px(72.0);
const KEY_WIDTH: Pixels = px(56.0);
const ENERGY_WIDTH: Pixels = px(64.0);
const STEMS_WIDTH: Pixels = px(72.0);
/// What a track without stems offers in the Stems column on hover.
const SEPARATE_LABEL: &str = "Separate";
const TIME_WIDTH: Pixels = px(64.0);
const SOURCE_WIDTH: Pixels = px(88.0);
const MATCH_WIDTH: Pixels = px(232.0);
/// How faint a row is once its track played this session.
const PLAYED_ROW_OPACITY: f32 = 0.55;
/// How the spare width is shared: titles are the longest text in a row, so the title takes the
/// most; the genre gets room for a folder and its subfolder, such as `afro house / warmup`.
const TITLE_SHARE: f32 = 3.0;
const ARTIST_SHARE: f32 = 1.5;
const GENRE_SHARE: f32 = 2.0;
const FILTER_BAR_HEIGHT: Pixels = px(40.0);
const FILTER_HEIGHT: Pixels = px(28.0);
const LOAD_WIDTH: Pixels = px(84.0);
/// What the source column says of a track whose file is not where rekordbox says it is.
const MISSING_SOURCE: &str = "File missing";
const ROW_GROUP: &str = "track-row";

/// Where the library stands.
#[derive(Clone)]
pub enum LibraryStatus {
    /// Being opened.
    Reading,
    /// Open, with every track from the sources the DJ chose.
    Ready(Arc<Collection>),
    /// The DJ has not chosen any music folder nor rekordbox yet.
    Unchosen,
}

/// What the browser asks the window to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserEvent {
    /// Load a track on a deck.
    Load {
        /// The deck.
        deck: DeckId,
        /// The track.
        track: TrackIndex,
    },
    /// Load a track on whichever deck is free.
    LoadOnFreeDeck(TrackIndex),
    /// Ask for music folders to add to the library.
    AddMusicFolders,
    /// Make rekordbox's collection part of the library.
    ImportRekordbox,
    /// Find the moved files of every missing track.
    RelinkAll,
    /// Find the moved file of one missing track.
    Relink(TrackIndex),
    /// Separate a track into stems, ahead of every other track waiting.
    SeparateStems(TrackIndex),
    /// Stop separating a track, or forget it if it waits.
    CancelStems(TrackIndex),
    /// The DJ sorted a screen's tracks by another column, or went back to the crate's order.
    Sorted {
        /// The screen.
        screen: Mode,
        /// The new sort.
        sort: Option<Sort>,
    },
}

/// Which side of the browser a controller's browse knob moves through, as rekordbox's tree view
/// and track list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BrowserPane {
    /// The crates on the left.
    Crates,
    /// The tracks of the chosen crate.
    #[default]
    Tracks,
}

/// A track being dragged from the browser to a deck.
#[derive(Debug, Clone)]
pub struct DraggedTrack {
    /// The track.
    pub track: TrackIndex,
    title: SharedString,
}

impl Render for DraggedTrack {
    fn render(&mut self, _: &mut Window, _: &mut Context<'_, Self>) -> impl IntoElement {
        div()
            .px(px(10.0))
            .py(px(5.0))
            .rounded(theme::SMALL_RADIUS)
            .border_1()
            .border_color(theme::LINE)
            .bg(theme::RAISED)
            .text_color(theme::TEXT)
            .text_size(theme::SMALL_TEXT_SIZE)
            .font_weight(gpui::FontWeight::MEDIUM)
            .child(self.title.clone())
    }
}

/// The crates and the track table.
pub struct Browser {
    library: LibraryStatus,
    crate_index: CrateIndex,
    query: SearchQuery,
    search_text: String,
    played: Arc<HashSet<PathBuf>>,
    rows: Vec<TrackIndex>,
    searched_count: usize,
    selected: Option<TrackIndex>,
    pane: BrowserPane,
    in_set: Arc<HashSet<TrackIndex>>,
    screen: Mode,
    view: LibraryView,
    missing: Arc<HashSet<TrackIndex>>,
    relinking: bool,
    analyses: HashMap<PathBuf, Result<Arc<AudioAnalysis>, String>>,
    matches: Arc<LibraryMatches>,
    stems: StemStatuses,
    search_input: Entity<InputState>,
    table_focus: FocusHandle,
    scroll: UniformListScrollHandle,
    detail_scroll: gpui::ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<BrowserEvent> for Browser {}

impl Browser {
    /// An empty browser, waiting for the library.
    pub fn new(window: &mut Window, cx: &mut Context<'_, Self>) -> Self {
        let search_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search, or bpm:120-124 key:fits"));
        let subscription = cx.subscribe_in(
            &search_input,
            window,
            |this: &mut Self, input, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    let text = input.read(cx).value();
                    this.set_search_text(text.to_string(), cx);
                }
                InputEvent::PressEnter { .. } => this.focus_table(window, cx),
                InputEvent::Focus | InputEvent::Blur => cx.notify(),
            },
        );
        Self {
            library: LibraryStatus::Reading,
            crate_index: CrateIndex::COLLECTION,
            query: SearchQuery::default(),
            search_text: String::new(),
            played: Arc::default(),
            rows: Vec::new(),
            searched_count: 0,
            selected: None,
            pane: BrowserPane::default(),
            in_set: Arc::default(),
            screen: Mode::Perform,
            view: LibraryView::default(),
            missing: Arc::default(),
            relinking: false,
            analyses: HashMap::new(),
            matches: Arc::default(),
            stems: StemStatuses::default(),
            search_input,
            table_focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
            detail_scroll: gpui::ScrollHandle::new(),
            _subscriptions: vec![subscription],
        }
    }

    /// Shows the library as it now stands.
    /// The selected track stays selected when it is still in the library.
    pub fn set_library(&mut self, library: LibraryStatus, cx: &mut Context<'_, Self>) {
        let selected_identity = match (&self.library, self.selected) {
            (LibraryStatus::Ready(collection), Some(track)) => {
                Some(collection.track(track).identity())
            }
            _ => None,
        };
        if let LibraryStatus::Ready(collection) = &library {
            self.selected = selected_identity.and_then(|identity| collection.find(&identity));
            if collection
                .indexed_crates()
                .nth(self.crate_index.position())
                .is_none()
            {
                self.crate_index = CrateIndex::COLLECTION;
            }
        }
        self.library = library;
        self.refilter(cx);
    }

    /// The search field, which sits in the window's header.
    pub fn search_input(&self) -> &Entity<InputState> {
        &self.search_input
    }

    /// Marks the tracks that are in the set being prepared.
    pub fn set_marked(&mut self, in_set: Arc<HashSet<TrackIndex>>, cx: &mut Context<'_, Self>) {
        self.in_set = in_set;
        cx.notify();
    }

    /// Shows completed analyses, retaining the error when a track could not be analysed.
    pub fn add_analyses(&mut self, analyses: Vec<AnalysisAnswer>, cx: &mut Context<'_, Self>) {
        self.analyses.extend(analyses);
        let reads_analysis = |column| matches!(column, Column::Bpm | Column::Key | Column::Energy);
        if self.sort().is_some_and(|sort| reads_analysis(sort.column))
            || self.view.is_filtered()
            || self.query.has_filters()
        {
            self.refilter(cx);
        }
        cx.notify();
    }

    /// Shows how every track mixes after the track the room follows.
    pub fn set_matches(&mut self, matches: Arc<LibraryMatches>, cx: &mut Context<'_, Self>) {
        let narrows = self
            .view
            .set_match_reference(matches.reference_title().is_some());
        self.matches = matches;
        if narrows
            || self.view.match_filter_on()
            || self.sort().is_some_and(|sort| sort.column == Column::Match)
        {
            self.refilter(cx);
        }
        cx.notify();
    }

    /// Shows where each track's stems stand.
    pub fn set_stems(&mut self, stems: StemStatuses, cx: &mut Context<'_, Self>) {
        self.stems = stems;
        if self.sort().is_some_and(|sort| sort.column == Column::Stems) {
            self.refilter(cx);
        }
        cx.notify();
    }

    /// How `entry` mixes after the track the room follows.
    fn track_match(&self, entry: &CollectionTrack) -> Option<&TrackMatch> {
        self.matches.get(entry.track.file_path.as_deref()?)
    }

    fn toggle_match_filter(&mut self, cx: &mut Context<'_, Self>) {
        self.view.toggle_match_filter();
        self.refilter(cx);
        self.reveal_selection();
    }

    /// Where the stems of `entry` stand.
    fn stems_status(&self, entry: &CollectionTrack) -> &StemsStatus {
        entry
            .track
            .file_path
            .as_ref()
            .and_then(|path| self.stems.get(path))
            .unwrap_or(&StemsStatus::None)
    }

    /// What the analyser has said about `entry` so far.
    fn analysis_state(&self, entry: &CollectionTrack) -> AnalysisState<'_> {
        match entry
            .track
            .file_path
            .as_ref()
            .and_then(|path| self.analyses.get(path))
        {
            Some(Ok(answer)) => AnalysisState::Found(answer),
            Some(Err(_)) => AnalysisState::Failed,
            None => AnalysisState::Waiting,
        }
    }

    /// Marks the rekordbox tracks whose files are not where rekordbox says, and whether their
    /// new places are being looked for.
    pub fn set_missing(
        &mut self,
        missing: Arc<HashSet<TrackIndex>>,
        relinking: bool,
        cx: &mut Context<'_, Self>,
    ) {
        self.missing = missing;
        self.relinking = relinking;
        if self
            .sort()
            .is_some_and(|sort| sort.column == Column::Source)
        {
            self.refilter(cx);
        }
        cx.notify();
    }

    /// Shows the tracks as `screen` has them: sorted its way, with the load buttons only where
    /// the decks are.
    pub fn show_screen(&mut self, screen: Mode, cx: &mut Context<'_, Self>) {
        if self.screen == screen {
            return;
        }
        let resort = self.view.sort(self.screen) != self.view.sort(screen);
        self.screen = screen;
        if resort {
            self.refilter(cx);
            self.reveal_selection();
        }
        cx.notify();
    }

    /// Sorts `screen`'s tracks by `sort`, as the DJ left them.
    pub fn restore_sort(&mut self, screen: Mode, sort: Option<Sort>, cx: &mut Context<'_, Self>) {
        self.view.set_sort(screen, sort);
        if screen == self.screen {
            self.refilter(cx);
        }
    }

    /// The track the quick filters start from, when there is one.
    pub fn set_filter_reference(
        &mut self,
        reference: Option<FilterReference>,
        cx: &mut Context<'_, Self>,
    ) {
        if reference.is_none() || self.view.reference() == reference.as_ref() {
            return;
        }
        if self.view.set_reference(reference) || self.query.has_filters() {
            self.refilter(cx);
        }
        cx.notify();
    }

    fn sort(&self) -> Option<Sort> {
        self.view.sort(self.screen)
    }

    fn click_header(&mut self, column: Column, cx: &mut Context<'_, Self>) {
        let sort = Sort::after_click(self.sort(), column);
        self.view.set_sort(self.screen, sort);
        cx.emit(BrowserEvent::Sorted {
            screen: self.screen,
            sort,
        });
        self.refilter(cx);
        self.reveal_selection();
    }

    fn toggle_bpm_filter(&mut self, cx: &mut Context<'_, Self>) {
        let selected_bpm = self.selected_bpm();
        self.view.toggle_bpm_range(selected_bpm);
        self.refilter(cx);
        self.reveal_selection();
    }

    /// The selected track's tempo, where a BPM range starts when there is no reference.
    fn selected_bpm(&self) -> Option<f64> {
        let LibraryStatus::Ready(collection) = &self.library else {
            return None;
        };
        let track = self.selected?;
        BpmRange::tempo_of(&self.cell_text(collection.track(track), track, Column::Bpm))
    }

    fn step_bpm_range(&mut self, end: RangeEnd, direction: Direction, cx: &mut Context<'_, Self>) {
        self.view.step_bpm_range(end, direction);
        self.refilter(cx);
        self.reveal_selection();
    }

    fn toggle_key_filter(&mut self, cx: &mut Context<'_, Self>) {
        self.view.toggle_key_filter();
        self.refilter(cx);
        self.reveal_selection();
    }

    /// What a track's cell in `column` says. Sorting and the quick filters read the same text,
    /// so they always agree with what the table shows.
    fn cell_text(
        &self,
        entry: &CollectionTrack,
        track: TrackIndex,
        column: Column,
    ) -> SharedString {
        let row = &entry.row;
        match column {
            Column::Title => row.title.clone(),
            Column::Artist => row.artist.clone(),
            Column::Genre => row.genre.clone(),
            Column::Bpm => bpm_cell(entry, self.analysis_state(entry)),
            Column::Key => key_cell(entry, self.analysis_state(entry)),
            Column::Energy => energy_cell(self.analysis_state(entry)),
            Column::Match => self
                .track_match(entry)
                .and_then(|found| found.rank)
                .map(|rank| rank.to_string().into())
                .unwrap_or_default(),
            Column::Stems => stems_cell(self.stems_status(entry)),
            Column::Time => row.time.clone(),
            Column::Source => {
                if self.missing.contains(&track) {
                    MISSING_SOURCE.into()
                } else {
                    row.source.clone()
                }
            }
        }
    }

    /// Scrolls the selected track into the middle of the table.
    fn reveal_selection(&self) {
        if let Some(position) = self
            .selected
            .and_then(|track| self.rows.iter().position(|row| *row == track))
        {
            self.scroll.scroll_to_item(position, ScrollStrategy::Center);
        }
    }

    /// What the last column holds: the load buttons where the decks are, otherwise only Relink
    /// on tracks whose files are missing, or nothing.
    fn actions(&self) -> Actions {
        if self.screen == Mode::Perform {
            Actions::Load
        } else if self.missing.is_empty() {
            Actions::None
        } else {
            Actions::RelinkOnly
        }
    }

    /// The selected track.
    pub fn selected(&self) -> Option<TrackIndex> {
        self.selected
    }

    /// Puts the cursor in the search field.
    pub fn focus_search(&self, window: &mut Window, cx: &mut Context<'_, Self>) {
        self.search_input
            .update(cx, |input, cx| input.focus(window, cx));
    }

    /// Moves the keyboard to the track table, leaving the search field.
    pub fn focus_table(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        if self.selected.is_none() {
            self.select(self.rows.first().copied(), cx);
        }
        window.focus(&self.table_focus, cx);
    }

    /// Whether the search field has the keyboard.
    pub fn search_has_focus(&self, window: &Window, cx: &App) -> bool {
        self.search_input
            .read(cx)
            .focus_handle(cx)
            .is_focused(window)
    }

    /// Selects the row above or below the selected one.
    pub fn move_selection(&mut self, direction: Direction, cx: &mut Context<'_, Self>) {
        self.pane = BrowserPane::Tracks;
        let steps = match direction {
            Direction::Up => -1,
            Direction::Down => 1,
        };
        self.move_track_cursor(steps, cx);
    }

    /// Moves the cursor `steps` rows down, or up when negative, in the pane that has it: the
    /// crates, choosing each one passed, or the tracks.
    pub fn browse(&mut self, steps: i32, cx: &mut Context<'_, Self>) {
        match self.pane {
            BrowserPane::Crates => self.move_crate_cursor(steps, cx),
            BrowserPane::Tracks => self.move_track_cursor(steps, cx),
        }
    }

    /// Moves the browse cursor between the crates and the tracks.
    pub fn switch_pane(&mut self, cx: &mut Context<'_, Self>) {
        self.pane = match self.pane {
            BrowserPane::Crates => BrowserPane::Tracks,
            BrowserPane::Tracks => BrowserPane::Crates,
        };
        cx.notify();
    }

    fn move_track_cursor(&mut self, steps: i32, cx: &mut Context<'_, Self>) {
        let current = self
            .selected
            .and_then(|track| self.rows.iter().position(|row| *row == track));
        let next = match current {
            None => Some(0),
            Some(position) => offset(position, steps, self.rows.len()),
        };
        if let Some((next, track)) = next.and_then(|next| Some((next, *self.rows.get(next)?))) {
            self.select(Some(track), cx);
            self.scroll.scroll_to_item(next, ScrollStrategy::Nearest);
        }
    }

    fn move_crate_cursor(&mut self, steps: i32, cx: &mut Context<'_, Self>) {
        let LibraryStatus::Ready(collection) = &self.library else {
            return;
        };
        let count = collection.indexed_crates().count();
        let next = offset(self.crate_index.position(), steps, count)
            .and_then(|next| collection.indexed_crates().nth(next))
            .map(|(crate_index, _)| crate_index);
        if let Some(crate_index) = next {
            self.choose_crate(crate_index, cx);
        }
    }

    fn select(&mut self, track: Option<TrackIndex>, cx: &mut Context<'_, Self>) {
        self.selected = track;
        self.detail_scroll.set_offset(gpui::point(px(0.0), px(0.0)));
        cx.notify();
    }

    fn set_search_text(&mut self, text: String, cx: &mut Context<'_, Self>) {
        let query = SearchQuery::parse(&text);
        self.search_text = text;
        if query == self.query {
            cx.notify();
            return;
        }
        self.query = query;
        self.refilter(cx);
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
    }

    /// Marks the tracks a deck played this session: `played:` filters read them, and their
    /// rows are dimmed.
    pub fn set_played(&mut self, played: Arc<HashSet<PathBuf>>, cx: &mut Context<'_, Self>) {
        self.played = played;
        if self.query.has_filters() {
            self.refilter(cx);
        }
        cx.notify();
    }

    /// Puts a suggested filter in place of the word being typed, and keeps typing there.
    fn take_suggestion(&mut self, word: &str, window: &mut Window, cx: &mut Context<'_, Self>) {
        let text = complete(&self.search_text, word);
        self.search_input.update(cx, |input, cx| {
            input.set_value(text.clone(), window, cx);
            input.focus(window, cx);
        });
        self.set_search_text(text, cx);
    }

    /// What the search filters read of `entry`: the same cells the table shows.
    fn track_facts<'entry>(
        &self,
        entry: &'entry CollectionTrack,
        track: TrackIndex,
    ) -> TrackFacts<'entry> {
        let energy = self.cell_text(entry, track, Column::Energy);
        TrackFacts {
            artist: &entry.row.artist,
            genre: &entry.row.genre,
            bpm: BpmRange::tempo_of(&self.cell_text(entry, track, Column::Bpm)),
            key: Key::parse_file_key(&self.cell_text(entry, track, Column::Key)),
            energy: energy.trim().parse().ok(),
            played: entry
                .track
                .file_path
                .as_ref()
                .is_some_and(|path| self.played.contains(path)),
            rating: entry.track.rating,
        }
    }

    /// The filters to offer for what is being typed, from the reference and selected tracks.
    fn search_suggestions(&self) -> Vec<String> {
        let reference = self.view.reference();
        let genre = match (&self.library, self.selected) {
            (LibraryStatus::Ready(collection), Some(track)) => collection
                .track(track)
                .row
                .genre
                .split(|character: char| character == '/' || character.is_whitespace())
                .find(|word| !word.is_empty())
                .map(str::to_lowercase),
            _ => None,
        };
        let context = SuggestionContext {
            bpm: reference.and_then(|reference| reference.bpm),
            key: reference
                .and_then(|reference| reference.key)
                .map(|fit| fit.reference()),
            genre,
        };
        suggestions(&self.search_text, &context)
    }

    fn choose_crate(&mut self, crate_index: CrateIndex, cx: &mut Context<'_, Self>) {
        self.crate_index = crate_index;
        self.refilter(cx);
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
    }

    fn refilter(&mut self, cx: &mut Context<'_, Self>) {
        let LibraryStatus::Ready(collection) = self.library.clone() else {
            self.rows = Vec::new();
            self.searched_count = 0;
            cx.notify();
            return;
        };
        let mut rows = filter_tracks(&collection, self.crate_index, &self.query);
        if self.query.has_filters() {
            let fits = self.view.reference().and_then(|reference| reference.key);
            rows.retain(|track| {
                let facts = self.track_facts(collection.track(*track), *track);
                self.query.filters_keep(&facts, fits)
            });
        }
        self.searched_count = rows.len();
        self.view.apply(self.screen, &mut rows, |track, column| {
            self.cell_text(collection.track(track), track, column)
        });
        self.rows = rows;
        if self
            .selected
            .is_none_or(|selected| !self.rows.contains(&selected))
        {
            self.selected = self.rows.first().copied();
        }
        cx.notify();
    }

    fn render_sidebar(
        &self,
        collection: &Collection,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        div()
            .id("crates")
            .flex()
            .flex_col()
            .flex_none()
            .w(SIDEBAR_WIDTH)
            .h_full()
            .p(px(8.0))
            .gap(px(1.0))
            .border_r_1()
            .border_color(theme::LINE)
            .bg(theme::BACKGROUND)
            .overflow_y_scroll()
            .children(collection.indexed_crates().map(|(crate_index, entry)| {
                let chosen = crate_index == self.crate_index;
                let count: SharedString = match entry.kind {
                    CrateKind::Folder => "".into(),
                    CrateKind::Smart => "smart".into(),
                    CrateKind::Collection | CrateKind::Playlist | CrateKind::MusicFolder => {
                        entry.tracks.len().to_string().into()
                    }
                };
                div()
                    .id(("crate", crate_index.position()))
                    .accessible_toggle(entry.name.clone(), chosen)
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_between()
                    .h(px(28.0))
                    .pl(px(8.0) + px(12.0) * entry.depth as f32)
                    .pr(px(8.0))
                    .rounded(theme::SMALL_RADIUS)
                    .cursor_pointer()
                    .text_color(if chosen { theme::TEXT } else { theme::MUTED })
                    .when(chosen, |row| {
                        row.bg(theme::RAISED).font_weight(gpui::FontWeight::MEDIUM)
                    })
                    .hover(|row| row.text_color(theme::TEXT))
                    .border_1()
                    .border_color(if chosen && self.pane == BrowserPane::Crates {
                        theme::MUTED
                    } else {
                        theme::faded(theme::MUTED, 0.0)
                    })
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.pane = BrowserPane::Crates;
                        this.choose_crate(crate_index, cx);
                    }))
                    .child(div().truncate().child(entry.name.clone()))
                    .child(
                        div()
                            .font_family(MONO_FAMILY)
                            .text_size(theme::TINY_TEXT_SIZE)
                            .text_color(theme::MUTED)
                            .child(count),
                    )
            }))
    }

    fn render_table(
        &self,
        collection: Arc<Collection>,
        searching: bool,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        let empty_message: Option<SharedString> = if !self.rows.is_empty() {
            None
        } else if self.searched_count > 0 {
            Some("No track matches the filters.".into())
        } else if !self.query.is_empty() {
            Some("No track matches the search.".into())
        } else {
            Some("This crate is empty.".into())
        };
        let list_collection = Arc::clone(&collection);
        div()
            .id("track-table")
            .role(gpui::Role::Grid)
            .aria_row_count(self.rows.len() + 1)
            .aria_column_count(Column::ALL.len() + usize::from(self.actions() != Actions::None))
            .aria_label("Library tracks")
            .aria_description("Use Up and Down to select a track. Full title and artist appear below the list. Shift Left or Shift Right loads a deck.")
            .tab_index(0)
            .focus_visible(|style| style.border_1().border_color(theme::TEXT))
            .track_focus(&self.table_focus)
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(theme::BACKGROUND)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, window, cx| {
                    window.focus(&this.table_focus, cx);
                }),
            )
            .child(self.render_filters(cx))
            .when(searching, |table| table.children(self.render_suggestions(cx)))
            .when(!self.missing.is_empty(), |table| {
                table.child(missing_bar(self.missing.len(), self.relinking, cx))
            })
            .child(self.render_header(cx))
            .map(|table| match empty_message {
                Some(message) => table.child(
                    div()
                        .px(theme::GUTTER)
                        .py(px(12.0))
                        .text_color(theme::MUTED)
                        .child(message),
                ),
                None => table.child(
                    uniform_list(
                        "tracks",
                        self.rows.len(),
                        cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                            range
                                .map(|position| {
                                    let track = this.rows[position];
                                    this.render_row(&list_collection, position, track, cx)
                                })
                                .collect::<Vec<_>>()
                        }),
                    )
                    .flex_1()
                    .track_scroll(&self.scroll),
                ),
            })
            .when_some(self.selected, |table, selected| {
                let entry = collection.track(selected);
                table.child(
                    div().id("selected-track-details")
                        .accessible_button(format!("Selected track: {} by {}", entry.row.title, entry.row.artist))
                        .role(gpui::Role::Group)
                        .aria_description("Full track details. Use Up, Down, Page Up or Page Down to scroll, and Home or End to reach either end.")
                        .track_scroll(&self.detail_scroll)
                        .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                            let stroke = &event.keystroke;
                            if stroke.modifiers.platform || stroke.modifiers.control || stroke.modifiers.alt { return; }
                            let scroll = &this.detail_scroll;
                            let offset = scroll.offset();
                            let page = scroll.bounds().size.height.max(px(24.0));
                            let y = match stroke.key.as_str() {
                                "up" => offset.y + px(24.0),
                                "down" => offset.y - px(24.0),
                                "pageup" => offset.y + page,
                                "pagedown" => offset.y - page,
                                "home" => px(0.0),
                                "end" => -scroll.max_offset().y,
                                _ => return,
                            };
                            scroll.set_offset(gpui::point(offset.x, y.clamp(-scroll.max_offset().y, px(0.0))));
                            cx.stop_propagation();
                            cx.notify();
                        }))
                        .flex_none().min_w_0().max_h(px(120.0)).overflow_y_scroll()
                        .px(theme::GUTTER).py(px(8.0))
                        .border_t_1().border_color(theme::LINE)
                        .child(div().font_weight(gpui::FontWeight::MEDIUM).child(entry.row.title.clone()))
                        .child(div().text_color(theme::MUTED).child(entry.row.artist.clone()))
                        .when_some(entry.track.file_path.as_ref().and_then(|path| self.analyses.get(path)).and_then(|answer| answer.as_ref().err()), |details, problem| {
                            details.child(div().text_color(theme::ALERT).child(format!("Analysis failed: {problem}. Rescan this music folder to retry.")))
                        })
                )
            })
    }

    /// The quick filters above the tracks, and how many tracks they leave.
    fn render_filters(&self, cx: &mut Context<'_, Self>) -> impl IntoElement + use<> {
        let count = if self.view.is_filtered() {
            format!("{} of {}", self.rows.len(), self.searched_count)
        } else {
            let count = self.rows.len();
            format!("{count} track{}", if count == 1 { "" } else { "s" })
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.0))
            .h(FILTER_BAR_HEIGHT)
            .px(theme::GUTTER)
            .border_b_1()
            .border_color(theme::LINE)
            .child(self.bpm_filter(cx))
            .child(self.key_filter(cx))
            .child(self.match_filter(cx))
            .child(div().flex_1())
            .child(
                div()
                    .flex_none()
                    .font_family(MONO_FAMILY)
                    .text_size(theme::TINY_TEXT_SIZE)
                    .text_color(theme::MUTED)
                    .child(count),
            )
    }

    /// The filters the DJ can type, as chips under the filter bar while the search field has
    /// the keyboard: a click puts one in the search in place of the word being typed.
    fn render_suggestions(&self, cx: &mut Context<'_, Self>) -> Option<impl IntoElement + use<>> {
        let words = self.search_suggestions();
        if words.is_empty() {
            return None;
        }
        Some(
            div()
                .id("search-suggestions")
                .role(gpui::Role::Group)
                .aria_label("Search filters")
                .flex()
                .flex_none()
                .items_center()
                .gap(px(6.0))
                .h(FILTER_BAR_HEIGHT)
                .px(theme::GUTTER)
                .border_b_1()
                .border_color(theme::LINE)
                .overflow_x_hidden()
                .child(
                    div()
                        .flex_none()
                        .text_size(theme::TINY_TEXT_SIZE)
                        .text_color(theme::MUTED)
                        .child("Filters"),
                )
                .children(words.into_iter().enumerate().map(|(position, word)| {
                    let shared: SharedString = word.into();
                    div()
                        .id(("search-suggestion", position))
                        .accessible_button(format!("Search with {shared}"))
                        .flex_none()
                        .px(px(8.0))
                        .py(px(3.0))
                        .rounded(theme::CONTROL_RADIUS)
                        .border_1()
                        .border_color(theme::LINE)
                        .font_family(MONO_FAMILY)
                        .text_size(theme::TINY_TEXT_SIZE)
                        .text_color(theme::MUTED)
                        .cursor_pointer()
                        .hover(|chip| chip.text_color(theme::TEXT).bg(theme::RAISED))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener({
                                let shared = shared.clone();
                                move |this, _: &MouseDownEvent, window, cx| {
                                    cx.stop_propagation();
                                    window.prevent_default();
                                    this.take_suggestion(&shared, window, cx);
                                }
                            }),
                        )
                        .child(shared)
                })),
        )
    }

    /// BPM, which turns on a range around the reference track, then its two ends to step.
    fn bpm_filter(&self, cx: &mut Context<'_, Self>) -> impl IntoElement + use<> {
        let range = self.view.bpm_range();
        let toggle = filter_chip("bpm-filter", "BPM", range.is_some())
            .tooltip(tooltip(if range.is_some() {
                "Show every tempo"
            } else {
                "Only tracks in a tempo range, a few BPM either side of the selected track"
            }))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_bpm_filter(cx)));
        let Some(range) = range else {
            return toggle.into_any_element();
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(2.0))
            .h(FILTER_HEIGHT)
            .pr(px(2.0))
            .rounded(theme::CONTROL_RADIUS)
            .border_1()
            .border_color(theme::LINE)
            .bg(theme::PANEL)
            .child(toggle.border_0())
            .child(bpm_stepper(RangeEnd::Lowest, range.lowest, cx))
            .child(div().text_color(theme::MUTED).child("\u{2013}"))
            .child(bpm_stepper(RangeEnd::Highest, range.highest, cx))
            .child(clear_button(
                "bpm-filter-clear",
                cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_bpm_filter(cx)),
            ))
            .into_any_element()
    }

    /// Key fits: only the tracks whose key follows the reference track's as the Next
    /// suggestions would have it.
    fn key_filter(&self, cx: &mut Context<'_, Self>) -> impl IntoElement + use<> {
        let reference_key = self
            .view
            .reference()
            .and_then(|reference| Some((reference, reference.key?.reference())));
        let on = self.view.key_filter_on() && reference_key.is_some();
        let explanation: SharedString = match reference_key {
            Some((reference, key)) => format!(
                "Only tracks whose key mixes well after {} ({}): the same key, one step on \
                 the wheel, or the relative key",
                reference.title,
                key.camelot()
            )
            .into(),
            None => "Select a set track with a key, or play a deck, to see what fits it".into(),
        };
        filter_chip("key-filter", "Key fits", on)
            .when_some(reference_key, |chip, (_, key)| {
                chip.child(
                    div()
                        .font_family(MONO_FAMILY)
                        .text_size(theme::TINY_TEXT_SIZE)
                        .child(key.camelot()),
                )
            })
            .when(reference_key.is_none(), |chip| {
                chip.opacity(0.5).cursor_default().tab_stop(false)
            })
            .tooltip(tooltip(explanation))
            .when(reference_key.is_some(), |chip| {
                chip.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.toggle_key_filter(cx);
                }))
            })
    }

    /// Matches master: only the tracks the Next suggestions would offer after the track the
    /// room follows.
    fn match_filter(&self, cx: &mut Context<'_, Self>) -> impl IntoElement + use<> {
        let reference = self.matches.reference_title().map(str::to_owned);
        let on = self.view.match_filter_on();
        let explanation: SharedString = match &reference {
            Some(title) => format!(
                "Only tracks that mix well after {title}: a key that fits and a tempo within reach"
            )
            .into(),
            None => "Play a deck to see what mixes after it".into(),
        };
        filter_chip("match-filter", "Matches master", on)
            .when(reference.is_none(), |chip| {
                chip.opacity(0.5).cursor_default().tab_stop(false)
            })
            .tooltip(tooltip(explanation))
            .when(reference.is_some(), |chip| {
                chip.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.toggle_match_filter(cx);
                }))
            })
    }

    fn render_header(&self, cx: &mut Context<'_, Self>) -> impl IntoElement + use<> {
        let actions = self.actions();
        let sort = self.sort();
        div()
            .id("library-column-headers")
            .role(gpui::Role::Row)
            .aria_row_index(1)
            .flex()
            .flex_none()
            .items_center()
            .h(px(30.0))
            .text_size(theme::SMALL_TEXT_SIZE)
            .text_color(theme::MUTED)
            .font_weight(gpui::FontWeight::MEDIUM)
            .children(Column::ALL.map(|column| {
                let direction = sort
                    .filter(|sort| sort.column == column)
                    .map(|sort| sort.direction);
                let arrow = direction.map(|direction| {
                    svg()
                        .path(match direction {
                            SortDirection::Ascending => ARROW_UP_PATH,
                            SortDirection::Descending => ARROW_DOWN_PATH,
                        })
                        .size(px(11.0))
                        .flex_none()
                        .text_color(theme::TEXT)
                });
                let heading: SharedString = match (column, self.matches.heading()) {
                    (Column::Match, Some(heading)) => heading.to_owned().into(),
                    _ => column.label().into(),
                };
                let label = div().truncate().child(heading.clone());
                column_box(column)
                    .id(("sort", column as usize))
                    .accessible_button(match direction {
                        Some(SortDirection::Ascending) => {
                            format!("Sort by {heading}, ascending")
                        }
                        Some(SortDirection::Descending) => {
                            format!("Sort by {heading}, descending")
                        }
                        None => format!("Sort by {heading}"),
                    })
                    .role(gpui::Role::ColumnHeader)
                    .aria_column_index(column as usize + 1)
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .h_full()
                    .gap(px(4.0))
                    .when(column == Column::Title, |header| header.pl(px(22.0)))
                    .cursor_pointer()
                    .hover(|header| header.text_color(theme::TEXT))
                    .when(direction.is_some(), |header| header.text_color(theme::TEXT))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.click_header(column, cx);
                    }))
                    .map(|header| {
                        if column.is_numeric() {
                            header.children(arrow).child(label)
                        } else {
                            header.child(label).children(arrow)
                        }
                    })
            }))
            .when(actions != Actions::None, |row| {
                row.child(
                    div()
                        .id("action-column-header")
                        .role(gpui::Role::ColumnHeader)
                        .aria_column_index(Column::ALL.len() + 1)
                        .aria_label("Track actions")
                        .flex()
                        .items_center()
                        .justify_end()
                        .h_full()
                        .w(LOAD_WIDTH)
                        .flex_none()
                        .pr(theme::GUTTER)
                        .child(if actions == Actions::Load { "Load" } else { "" }),
                )
            })
    }

    fn render_row(
        &self,
        collection: &Collection,
        position: usize,
        track: TrackIndex,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement + use<> {
        let entry = collection.track(track);
        let actions = self.actions();
        let selected = self.selected == Some(track);
        let missing = self.missing.contains(&track);
        let in_set = self.in_set.contains(&track);
        let path = entry.track.file_path.as_deref();
        let played = path.is_some_and(|path| self.played.contains(path));
        let match_cell = path.map(|path| MatchCell::of(&self.matches, path));
        let dragged = DraggedTrack {
            track,
            title: entry.row.title.clone(),
        };
        div()
            .id(("track", position))
            .role(gpui::Role::Row)
            .aria_row_index(position + 2)
            .aria_label(format!("{} by {}", entry.row.title, entry.row.artist))
            .aria_selected(selected)
            .aria_position_in_set(position + 1)
            .aria_size_of_set(self.rows.len())
            .when(selected, |row| row.aria_active_descendant())
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.select(Some(track), cx);
            }))
            .group(ROW_GROUP)
            .flex()
            .items_center()
            .h(ROW_HEIGHT)
            .w_full()
            .border_t_1()
            .border_color(theme::ROW_LINE)
            .when(selected, |row| row.bg(theme::SELECTED_ROW))
            .hover(|row| row.bg(theme::SELECTED_ROW))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    window.focus(&this.table_focus, cx);
                    this.pane = BrowserPane::Tracks;
                    this.select(Some(track), cx);
                    if event.click_count == 2 {
                        cx.emit(BrowserEvent::LoadOnFreeDeck(track));
                    }
                    cx.stop_propagation();
                }),
            )
            .on_drag(dragged, |dragged, _, _, cx| cx.new(|_| dragged.clone()))
            .when(played, |row| row.opacity(PLAYED_ROW_OPACITY))
            .children(Column::ALL.map(|column| {
                let text = self.cell_text(entry, track, column);
                let cell = column_box(column)
                    .id(("track-cell", column as usize))
                    .role(gpui::Role::Cell)
                    .aria_column_index(column as usize + 1)
                    .aria_label(match (column, match_cell) {
                        (Column::Match, Some(found)) => found.description(played).into(),
                        _ => text.clone(),
                    });
                match column {
                    Column::Title => cell
                        .gap(px(8.0))
                        .pl(px(8.0))
                        .child(
                            div()
                                .size(px(6.0))
                                .flex_none()
                                .rounded_full()
                                .when(in_set, |dot| dot.bg(theme::OK)),
                        )
                        .child(
                            div()
                                .truncate()
                                .min_w_0()
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .when(missing, |title| title.text_color(theme::MUTED))
                                .child(text),
                        ),
                    Column::Artist => cell
                        .text_color(theme::MUTED)
                        .child(div().truncate().min_w_0().child(text)),
                    Column::Genre => cell
                        .text_size(theme::SMALL_TEXT_SIZE)
                        .text_color(theme::MUTED)
                        .child(div().truncate().min_w_0().child(text)),
                    Column::Bpm | Column::Key | Column::Energy => cell
                        .font_family(MONO_FAMILY)
                        .text_size(theme::SMALL_TEXT_SIZE)
                        .child(text),
                    Column::Match => cell.children(match_cell.map(MatchCell::render)),
                    Column::Stems => {
                        let status = self.stems_status(entry);
                        let separable =
                            matches!(status, StemsStatus::None | StemsStatus::Failed(_))
                                && entry.track.file_path.is_some()
                                && !missing;
                        let pending = status.is_pending();
                        cell.font_family(MONO_FAMILY)
                            .text_size(theme::SMALL_TEXT_SIZE)
                            .text_color(theme::MUTED)
                            .when(separable, |cell| {
                                cell.cursor_pointer()
                                    .child(
                                        div()
                                            .opacity(0.0)
                                            .group_hover(ROW_GROUP, |label| label.opacity(1.0))
                                            .hover(|label| label.text_color(theme::TEXT))
                                            .child(SEPARATE_LABEL),
                                    )
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation()
                                    })
                                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                                        cx.emit(BrowserEvent::SeparateStems(track));
                                    }))
                            })
                            .when(pending, |cell| {
                                cell.cursor_pointer()
                                    .hover(|cell| cell.text_color(theme::TEXT))
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation()
                                    })
                                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                                        cx.emit(BrowserEvent::CancelStems(track));
                                    }))
                            })
                            .when(!separable, |cell| cell.child(text))
                    }
                    Column::Time => cell
                        .font_family(MONO_FAMILY)
                        .text_size(theme::SMALL_TEXT_SIZE)
                        .text_color(theme::MUTED)
                        .child(text),
                    Column::Source => cell
                        .text_size(theme::SMALL_TEXT_SIZE)
                        .text_color(if missing {
                            theme::ATTENTION
                        } else {
                            theme::MUTED
                        })
                        .child(div().truncate().min_w_0().child(text)),
                }
            }))
            .when(actions != Actions::None, |row| {
                row.child(
                    div()
                        .id("actions-cell")
                        .role(gpui::Role::Cell)
                        .aria_column_index(Column::ALL.len() + 1)
                        .aria_label("Track actions")
                        .flex()
                        .flex_none()
                        .justify_end()
                        .gap(px(4.0))
                        .w(LOAD_WIDTH)
                        .pr(theme::GUTTER)
                        .map(|buttons| {
                            if missing {
                                buttons.child(relink_button(position, track, cx))
                            } else if actions == Actions::Load {
                                buttons.children(
                                    DeckId::ALL.map(|deck| load_button(position, track, deck, cx)),
                                )
                            } else {
                                buttons
                            }
                        }),
                )
            })
    }
}

/// What a track row's last column holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Actions {
    /// The load buttons, and Relink on a missing track.
    Load,
    /// Relink on a missing track only.
    RelinkOnly,
    /// No column.
    None,
}

/// `position` moved by `steps` and kept inside `count` rows, or `None` when there are none.
fn offset(position: usize, steps: i32, count: usize) -> Option<usize> {
    let last = count.checked_sub(1)?;
    let moved = i64::try_from(position).ok()? + i64::from(steps);
    Some(usize::try_from(moved.max(0)).ok()?.min(last))
}

/// The box a column's cells sit in, the same in the header and every row, so they line up.
fn column_box(column: Column) -> gpui::Div {
    let cell = div().flex().items_center().min_w_0().whitespace_nowrap();
    let shared = |cell: gpui::Div, share: f32| cell.flex_basis(px(0.0)).flex_grow(share);
    let fixed = |cell: gpui::Div, width: Pixels| cell.flex_none().w(width).px(px(8.0));
    match column {
        Column::Title => shared(cell, TITLE_SHARE).pr(theme::GUTTER),
        Column::Artist => shared(cell, ARTIST_SHARE).px(px(8.0)),
        Column::Genre => shared(cell, GENRE_SHARE).px(px(8.0)),
        Column::Bpm => fixed(cell, BPM_WIDTH).justify_end(),
        Column::Key => fixed(cell, KEY_WIDTH),
        Column::Energy => fixed(cell, ENERGY_WIDTH).justify_end(),
        Column::Match => fixed(cell, MATCH_WIDTH),
        Column::Stems => fixed(cell, STEMS_WIDTH).justify_end(),
        Column::Time => fixed(cell, TIME_WIDTH).justify_end(),
        Column::Source => fixed(cell, SOURCE_WIDTH),
    }
}

/// A quick filter that is on or off, in the compact style of the effects strip.
fn filter_chip(id: &'static str, label: &'static str, on: bool) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .accessible_toggle(label, on)
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.0))
        .h(FILTER_HEIGHT)
        .px(px(10.0))
        .rounded(theme::CONTROL_RADIUS)
        .border_1()
        .text_size(theme::SMALL_TEXT_SIZE)
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .map(|chip| {
            if on {
                chip.border_color(theme::LINE)
                    .bg(theme::RAISED)
                    .text_color(theme::TEXT)
                    .font_weight(gpui::FontWeight::MEDIUM)
            } else {
                chip.border_color(theme::LINE)
                    .text_color(theme::MUTED)
                    .hover(|chip| chip.text_color(theme::TEXT).bg(theme::PANEL))
            }
        })
        .child(label)
}

/// One end of the BPM range with its arrows, as the effects strip steps a beat length.
fn bpm_stepper(end: RangeEnd, bpm: u16, cx: &mut Context<'_, Browser>) -> impl IntoElement + use<> {
    let arrow = |glyph: &'static str, direction: Direction| {
        let index = match (end, direction) {
            (RangeEnd::Lowest, Direction::Down) => 0_usize,
            (RangeEnd::Lowest, Direction::Up) => 1,
            (RangeEnd::Highest, Direction::Down) => 2,
            (RangeEnd::Highest, Direction::Up) => 3,
        };
        div()
            .id(("bpm-step", index))
            .accessible_button(match (end, direction) {
                (RangeEnd::Lowest, Direction::Down) => "Decrease minimum BPM",
                (RangeEnd::Lowest, Direction::Up) => "Increase minimum BPM",
                (RangeEnd::Highest, Direction::Down) => "Decrease maximum BPM",
                (RangeEnd::Highest, Direction::Up) => "Increase maximum BPM",
            })
            .flex()
            .items_center()
            .justify_center()
            .w(px(18.0))
            .h(px(22.0))
            .rounded(px(5.0))
            .text_color(theme::MUTED)
            .cursor_pointer()
            .hover(|arrow| arrow.bg(theme::RAISED).text_color(theme::TEXT))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.step_bpm_range(end, direction, cx);
            }))
            .child(glyph)
    };
    div()
        .flex()
        .items_center()
        .child(arrow("\u{2039}", Direction::Down))
        .child(
            div()
                .min_w(px(28.0))
                .flex()
                .justify_center()
                .font_family(MONO_FAMILY)
                .text_size(theme::SMALL_TEXT_SIZE)
                .child(bpm.to_string()),
        )
        .child(arrow("\u{203a}", Direction::Up))
}

/// The cross at the end of a filter that is on.
fn clear_button(
    id: &'static str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .accessible_button("Clear BPM filter")
        .flex()
        .items_center()
        .justify_center()
        .size(px(22.0))
        .rounded(px(5.0))
        .text_color(theme::MUTED)
        .cursor_pointer()
        .hover(|button| button.bg(theme::RAISED).text_color(theme::TEXT))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(on_click)
        .tooltip(tooltip("Show every tempo"))
        .child(
            svg()
                .path(CLOSE_ICON_PATH)
                .size(px(12.0))
                .text_color(theme::MUTED),
        )
}

fn load_button(
    position: usize,
    track: TrackIndex,
    deck: DeckId,
    cx: &mut Context<'_, Browser>,
) -> impl IntoElement + use<> {
    let colour = theme::deck_colour(deck);
    div()
        .id(("load", position * 2 + deck.index()))
        .accessible_button(format!("Load track on deck {}", deck_letter(deck)))
        .flex()
        .items_center()
        .justify_center()
        .size(px(22.0))
        .rounded(px(5.0))
        .border_1()
        .border_color(theme::LINE)
        .font_family(MONO_FAMILY)
        .text_size(theme::TINY_TEXT_SIZE)
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(colour)
        .cursor_pointer()
        .hover(move |button| button.bg(colour).text_color(theme::ON_DECK))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.select(Some(track), cx);
            cx.emit(BrowserEvent::Load { deck, track });
        }))
        .child(deck_letter(deck))
}

/// The quiet line above the tracks when rekordbox tracks have lost their files.
fn missing_bar(count: usize, relinking: bool, cx: &mut Context<'_, Browser>) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .h(px(34.0))
        .px(theme::GUTTER)
        .border_b_1()
        .border_color(theme::LINE)
        .bg(theme::PANEL)
        .text_size(theme::SMALL_TEXT_SIZE)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(div().size(px(6.0)).rounded_full().bg(theme::ATTENTION))
                .child(format!(
                    "{count} file{} moved or missing since rekordbox last saw {}",
                    if count == 1 { "" } else { "s" },
                    if count == 1 { "it" } else { "them" }
                )),
        )
        .child(if relinking {
            div()
                .text_color(theme::MUTED)
                .child("Looking\u{2026}")
                .into_any_element()
        } else {
            button("relink-all", "Relink", Emphasis::Secondary)
                .on_click(cx.listener(|_, _: &ClickEvent, _, cx| {
                    cx.emit(BrowserEvent::RelinkAll);
                }))
                .into_any_element()
        })
}

/// Relink, in place of the load buttons on a track whose file is missing.
fn relink_button(
    position: usize,
    track: TrackIndex,
    cx: &mut Context<'_, Browser>,
) -> impl IntoElement + use<> {
    div()
        .id(("relink", position))
        .accessible_button("Relink track")
        .flex()
        .items_center()
        .h(px(22.0))
        .px(px(6.0))
        .rounded(px(5.0))
        .border_1()
        .border_color(theme::LINE)
        .text_size(theme::TINY_TEXT_SIZE)
        .text_color(theme::ATTENTION)
        .cursor_pointer()
        .hover(|button| button.bg(theme::RAISED))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.select(Some(track), cx);
            cx.emit(BrowserEvent::Relink(track));
        }))
        .child("Relink")
}

/// The search field as drawn in the header.
pub fn search_field(browser: &Browser) -> impl IntoElement + use<> {
    div()
        .flex()
        .items_center()
        .gap(px(8.0))
        .h(px(28.0))
        .w(px(320.0))
        .flex_shrink(1.0)
        .px(px(10.0))
        .rounded(theme::CONTROL_RADIUS)
        .border_1()
        .border_color(theme::LINE)
        .bg(theme::PANEL)
        .text_color(theme::MUTED)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            svg()
                .path(SEARCH_ICON_PATH)
                .size(px(14.0))
                .flex_none()
                .text_color(theme::MUTED),
        )
        .child(
            div().flex_1().min_w_0().child(
                Input::new(browser.search_input())
                    .appearance(false)
                    .cleanable(true),
            ),
        )
        .child(
            div()
                .font_family(MONO_FAMILY)
                .text_size(theme::TINY_TEXT_SIZE)
                .child("/"),
        )
}

impl Render for Browser {
    fn render(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let body = div().flex().size_full().min_h_0().bg(theme::BACKGROUND);
        let searching = self.search_has_focus(window, cx);
        match self.library.clone() {
            LibraryStatus::Ready(collection) => body
                .child(self.render_sidebar(&collection, cx))
                .child(self.render_table(collection, searching, cx)),
            LibraryStatus::Reading => {
                body.child(message_panel("Opening your library\u{2026}".into(), None))
            }
            LibraryStatus::Unchosen => body.child(empty_library(cx)),
        }
    }
}

/// What the library shows before the DJ has chosen where their music is: a folder picker, and
/// rekordbox as a separate choice.
fn empty_library(cx: &mut Context<'_, Browser>) -> impl IntoElement {
    div().flex().flex_1().items_center().justify_center().child(
        div()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .w(px(420.0))
            .child(
                div()
                    .text_size(px(15.0))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child("Add your music"),
            )
            .child(div().text_color(theme::MUTED).child(
                "Choose the folders that hold your tracks. Dubplate reads them, \
                 and their subfolders, and never changes your files.",
            ))
            .child(
                div()
                    .flex()
                    .gap(px(8.0))
                    .pt(px(6.0))
                    .child(
                        button(
                            "add-music-folders",
                            "Add music folders\u{2026}",
                            Emphasis::Primary,
                        )
                        .on_click(cx.listener(
                            |_, _: &ClickEvent, _, cx| {
                                cx.emit(BrowserEvent::AddMusicFolders);
                            },
                        )),
                    )
                    .child(
                        button(
                            "import-rekordbox",
                            "Import rekordbox collection",
                            Emphasis::Secondary,
                        )
                        .on_click(cx.listener(
                            |_, _: &ClickEvent, _, cx| {
                                cx.emit(BrowserEvent::ImportRekordbox);
                            },
                        )),
                    ),
            ),
    )
}

fn message_panel(title: SharedString, detail: Option<SharedString>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .p(px(24.0))
        .child(div().text_color(theme::TEXT).child(title))
        .children(detail.map(|detail| {
            div()
                .max_w(px(640.0))
                .text_color(theme::MUTED)
                .child(detail)
        }))
}

#[cfg(test)]
mod tests {
    use super::offset;

    #[test]
    fn the_cursor_stops_at_the_first_and_last_rows() {
        assert_eq!(offset(3, 2, 10), Some(5));
        assert_eq!(offset(3, -5, 10), Some(0));
        assert_eq!(offset(8, 4, 10), Some(9));
        assert_eq!(offset(0, 1, 0), None);
    }
}
