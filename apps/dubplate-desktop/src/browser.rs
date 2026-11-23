//! The browser: the crates on the left and the tracks of the chosen crate on the right, filtered
//! by the search field and the quick filters, and sorted by the column the DJ clicked. It asks
//! the window to load tracks through [`BrowserEvent`].

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dubplate_engine::DeckId;
use dubplate_prep::{AudioAnalysis, Key};
use dubplate_stems::StemsStatus;
use gpui::component::input::{Input, InputEvent, InputState};
use gpui::component::menu::ContextMenuExt;
use gpui::prelude::*;
use gpui::{
    App, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable, MouseButton,
    MouseDownEvent, Pixels, ScrollStrategy, SharedString, Subscription, UniformListScrollHandle,
    Window, div, px, svg, uniform_list,
};

use crate::analysis::{AnalysisAnswer, AnalysisState, bpm_cell, energy_cell, key_cell};
use crate::app::NARROW_WINDOW_WIDTH;
use crate::assets::{
    ARROW_DOWN_PATH, ARROW_UP_PATH, CLOSE_ICON_PATH, PLAYED_ICON_PATH, SEARCH_ICON_PATH,
};
use crate::collection::{Collection, CollectionTrack, CrateIndex, CrateKind, TrackIndex};
use crate::commands::{Direction, Mode};
use crate::deck::deck_letter;
use crate::folder_tree::visible_crates;
use crate::fonts::MONO_FAMILY;
use crate::history::{HistoryPage, HistoryShelf};
use crate::library_match::{LibraryMatches, TrackMatch};
use crate::library_view::{
    BpmRange, Column, FilterReference, LibraryView, RangeEnd, Sort, SortDirection,
};
use crate::match_cell::MatchCell;
use crate::search::{SearchQuery, filter_tracks};
use crate::search_filter::{
    SuggestionContext, TrackFacts, complete, suggestions, toggle_hide_played,
};
use crate::selection::Selection;
use crate::stems::{StemStatuses, stems_cell};
use crate::theme;
use crate::track_menu::{self, RowFacts, RowPlace, TrackTags};
use crate::track_metadata::{TrackMetadataCache, colour_name, rating_description, rating_stars};
use crate::tracklist::TracklistFormat;
use crate::widgets::{Emphasis, InteractiveControl, button, tooltip};

const ROW_HEIGHT: Pixels = px(32.0);
const SIDEBAR_WIDTH: Pixels = px(200.0);
const BPM_WIDTH: Pixels = px(72.0);
const KEY_WIDTH: Pixels = px(56.0);
const ENERGY_WIDTH: Pixels = px(64.0);
/// Room for five stars at the table's text size, and for the header with its sort arrow.
const RATING_WIDTH: Pixels = px(88.0);
/// Room for the Colour header with its sort arrow.
const COLOUR_WIDTH: Pixels = px(76.0);
/// The side of the swatch in the Colour column.
const SWATCH_SIZE: Pixels = px(14.0);
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
const HISTORY_TIME_WIDTH: Pixels = px(72.0);
const HISTORY_DECK_WIDTH: Pixels = px(56.0);
const SIDEBAR_ROW_HEIGHT: Pixels = px(28.0);
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
    /// The DJ pressed a track row, so the keys act on the library's tracks.
    TookKeys,
    /// Ask for music folders to add to the library.
    AddMusicFolders,
    /// Make rekordbox's collection part of the library.
    ImportRekordbox,
    /// Find the moved files of every missing track.
    RelinkAll,
    /// Find the moved file of one missing track.
    Relink(TrackIndex),
    /// Export a playlist or music folder to a USB stick for CDJs.
    ExportCrateToUsb(CrateIndex),
    /// Save a page of the history as a tracklist.
    ExportHistory {
        /// The page.
        page: HistoryPage,
        /// The file's format.
        format: TracklistFormat,
    },
    /// Separate a track into stems, ahead of every other track waiting.
    SeparateStems(TrackIndex),
    /// Stop separating a track, or forget it if it waits.
    CancelStems(TrackIndex),
    /// The DJ opened or closed a folder of the sidebar's tree, or chose another crate.
    FolderTreeChanged,
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
            .border_color(theme::active().line)
            .bg(theme::active().raised)
            .text_color(theme::active().text)
            .text_size(theme::SMALL_TEXT_SIZE)
            .font_weight(gpui::FontWeight::MEDIUM)
            .child(self.title.clone())
    }
}

/// The crates and the track table.
pub struct Browser {
    library: LibraryStatus,
    crate_index: CrateIndex,
    expanded_folders: HashSet<PathBuf>,
    chosen_folder: Option<PathBuf>,
    visible_crates: Vec<CrateIndex>,
    query: SearchQuery,
    search_text: String,
    played: Arc<HashSet<PathBuf>>,
    rows: Vec<TrackIndex>,
    searched_count: usize,
    selection: Selection<TrackIndex>,
    pane: BrowserPane,
    in_set: Arc<HashSet<TrackIndex>>,
    screen: Mode,
    view: LibraryView,
    missing: Arc<HashSet<TrackIndex>>,
    relinking: bool,
    analyses: HashMap<PathBuf, Result<Arc<AudioAnalysis>, String>>,
    history: Arc<HistoryShelf>,
    history_page: Option<HistoryPage>,
    matches: Arc<LibraryMatches>,
    stems: StemStatuses,
    search_input: Entity<InputState>,
    table_focus: FocusHandle,
    scroll: UniformListScrollHandle,
    detail_scroll: gpui::ScrollHandle,
    /// Whether the window is narrow, so the table shows [`Column::NARROW`].
    narrow: bool,
    /// What Dubplate has stored for the library's tracks, for the search filters.
    track_metadata: Arc<TrackMetadataCache>,
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
            expanded_folders: HashSet::new(),
            chosen_folder: None,
            visible_crates: Vec::new(),
            query: SearchQuery::default(),
            search_text: String::new(),
            played: Arc::default(),
            rows: Vec::new(),
            searched_count: 0,
            selection: Selection::default(),
            pane: BrowserPane::default(),
            in_set: Arc::default(),
            screen: Mode::Perform,
            view: LibraryView::default(),
            missing: Arc::default(),
            relinking: false,
            analyses: HashMap::new(),
            history: Arc::default(),
            history_page: None,
            matches: Arc::default(),
            stems: StemStatuses::default(),
            search_input,
            table_focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
            detail_scroll: gpui::ScrollHandle::new(),
            narrow: false,
            track_metadata: Arc::default(),
            _subscriptions: vec![subscription],
        }
    }

    /// Shows the library as it now stands.
    /// The selected track stays selected when it is still in the library.
    pub fn set_library(&mut self, library: LibraryStatus, cx: &mut Context<'_, Self>) {
        let selected_identity = match (&self.library, self.focused()) {
            (LibraryStatus::Ready(collection), Some(track)) => {
                Some(collection.track(track).identity())
            }
            _ => None,
        };
        if let LibraryStatus::Ready(collection) = &library {
            self.selection
                .select_only(selected_identity.and_then(|identity| collection.find(&identity)));
            let chosen = self.chosen_folder.as_ref().and_then(|folder| {
                collection
                    .indexed_crates()
                    .find(|(_, entry)| entry.path.as_ref() == Some(folder))
                    .map(|(index, _)| index)
            });
            if let Some(chosen) = chosen {
                self.crate_index = chosen;
                if let Some(folder) = &self.chosen_folder {
                    self.expanded_folders
                        .extend(folder.ancestors().skip(1).map(Path::to_path_buf));
                }
            } else if collection
                .indexed_crates()
                .nth(self.crate_index.position())
                .is_none()
            {
                self.crate_index = CrateIndex::COLLECTION;
            }
            self.visible_crates = visible_crates(collection, &self.expanded_folders);
        } else {
            self.visible_crates.clear();
        }
        self.library = library;
        self.refilter(cx);
    }

    /// Opens the folders and chooses the folder the DJ left, once the library is there to
    /// show them.
    pub fn restore_folder_tree(
        &mut self,
        expanded: Vec<PathBuf>,
        selected: Option<PathBuf>,
        cx: &mut Context<'_, Self>,
    ) {
        self.expanded_folders = expanded.into_iter().collect();
        self.chosen_folder = selected;
        if let LibraryStatus::Ready(_) = &self.library {
            let library = self.library.clone();
            self.set_library(library, cx);
        }
    }

    /// The folders shown open, and the music folder or subfolder chosen, none when another
    /// crate is.
    pub fn folder_tree(&self) -> (Vec<PathBuf>, Option<PathBuf>) {
        let mut expanded: Vec<PathBuf> = self.expanded_folders.iter().cloned().collect();
        expanded.sort();
        (expanded, self.chosen_folder.clone())
    }

    fn folder_tree_changed(&self, cx: &mut Context<'_, Self>) {
        cx.emit(BrowserEvent::FolderTreeChanged);
    }

    fn refresh_visible_crates(&mut self) {
        if let LibraryStatus::Ready(collection) = &self.library {
            self.visible_crates = visible_crates(collection, &self.expanded_folders);
        }
    }

    /// Opens or closes the folder `crate_index`. Closing a folder that holds the chosen one
    /// chooses the closed folder, so the tracks shown stay in view.
    fn toggle_folder(&mut self, crate_index: CrateIndex, expand: bool, cx: &mut Context<'_, Self>) {
        let LibraryStatus::Ready(collection) = self.library.clone() else {
            return;
        };
        let entry = collection.crate_at(crate_index);
        let Some(path) = entry.path.clone() else {
            return;
        };
        if !entry.has_subfolders {
            return;
        }
        if expand {
            self.expanded_folders.insert(path.clone());
        } else {
            self.expanded_folders.remove(&path);
        }
        self.refresh_visible_crates();
        if !self.visible_crates.contains(&self.crate_index) {
            self.choose_crate(crate_index, cx);
            return;
        }
        self.folder_tree_changed(cx);
        cx.notify();
    }

    /// Left and Right in the folder tree, when the cursor is on a folder that has subfolders:
    /// Right opens it, or steps into it when open; Left closes it, or steps out to the folder
    /// holding it. Returns whether the key was used.
    pub fn tree_key(&mut self, direction: Direction, cx: &mut Context<'_, Self>) -> bool {
        if self.pane != BrowserPane::Crates || self.history_page.is_some() {
            return false;
        }
        let LibraryStatus::Ready(collection) = self.library.clone() else {
            return false;
        };
        let entry = collection.crate_at(self.crate_index);
        let Some(path) = entry.path.clone() else {
            return false;
        };
        let expanded = self.expanded_folders.contains(&path);
        match direction {
            Direction::Up if entry.has_subfolders => {
                if expanded {
                    self.move_crate_cursor(1, cx);
                } else {
                    self.toggle_folder(self.crate_index, true, cx);
                }
                true
            }
            Direction::Down if entry.has_subfolders && expanded => {
                self.toggle_folder(self.crate_index, false, cx);
                true
            }
            Direction::Down if entry.depth > 0 => {
                let parent = collection
                    .indexed_crates()
                    .take(self.crate_index.position())
                    .filter(|(_, other)| {
                        other.kind == CrateKind::MusicFolder && other.depth < entry.depth
                    })
                    .last()
                    .map(|(index, _)| index);
                if let Some(parent) = parent {
                    self.choose_crate(parent, cx);
                }
                parent.is_some()
            }
            _ => false,
        }
    }

    /// Gives the filters what Dubplate has stored for the library's tracks.
    pub fn set_track_metadata(
        &mut self,
        metadata: Arc<TrackMetadataCache>,
        cx: &mut Context<'_, Self>,
    ) {
        self.track_metadata = metadata;
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

    /// Shows the play history as it now stands.
    pub fn set_history(&mut self, history: Arc<HistoryShelf>, cx: &mut Context<'_, Self>) {
        if self
            .history_page
            .is_some_and(|page| history.page(page).is_none())
        {
            self.history_page = Some(HistoryPage::ThisSession);
        }
        self.history = history;
        cx.notify();
    }

    /// Shows a page of the history in place of the tracks.
    fn open_history(&mut self, page: HistoryPage, cx: &mut Context<'_, Self>) {
        self.history_page = Some(page);
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
        let track = self.focused()?;
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
            Column::Rating => self
                .track_metadata
                .for_track(&entry.identity())
                .map(|stored| rating_stars(stored.rating).into())
                .unwrap_or_default(),
            Column::Colour => self
                .track_metadata
                .for_track(&entry.identity())
                .and_then(|stored| stored.colour)
                .map(|colour| colour_name(colour).into())
                .unwrap_or_default(),
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
            .focused()
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

    /// The selected track, while the tracks are shown.
    pub fn selected(&self) -> Option<TrackIndex> {
        self.focused().filter(|_| self.history_page.is_none())
    }

    /// The selected tracks in the order of the table's rows, the focused one among them; none
    /// while a history page shows.
    pub fn selected_tracks(&self) -> Vec<TrackIndex> {
        if self.history_page.is_some() {
            return Vec::new();
        }
        self.selection.in_order(&self.rows)
    }

    fn focused(&self) -> Option<TrackIndex> {
        self.selection.focused()
    }

    /// Puts the cursor in the search field.
    pub fn focus_search(&self, window: &mut Window, cx: &mut Context<'_, Self>) {
        self.search_input
            .update(cx, |input, cx| input.focus(window, cx));
    }

    /// Moves the keyboard to the track table, leaving the search field.
    pub fn focus_table(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        if self.focused().is_none() {
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
        self.move_track_cursor(steps, false, cx);
    }

    /// Shift plus an arrow: moves the focus one row up or down and selects from where the
    /// selection started to it.
    pub fn extend_selection(&mut self, direction: Direction, cx: &mut Context<'_, Self>) {
        self.pane = BrowserPane::Tracks;
        let steps = match direction {
            Direction::Up => -1,
            Direction::Down => 1,
        };
        self.move_track_cursor(steps, true, cx);
    }

    /// Command+A: selects every track the table shows.
    pub fn select_all(&mut self, cx: &mut Context<'_, Self>) {
        if self.history_page.is_some() {
            return;
        }
        self.pane = BrowserPane::Tracks;
        self.selection.select_all(&self.rows);
        cx.notify();
    }

    /// Moves the cursor `steps` rows down, or up when negative, in the pane that has it: the
    /// crates, choosing each one passed, or the tracks.
    pub fn browse(&mut self, steps: i32, cx: &mut Context<'_, Self>) {
        match self.pane {
            BrowserPane::Crates => self.move_crate_cursor(steps, cx),
            BrowserPane::Tracks => self.move_track_cursor(steps, false, cx),
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

    fn move_track_cursor(&mut self, steps: i32, extend: bool, cx: &mut Context<'_, Self>) {
        if self.history_page.is_some() {
            return;
        }
        let current = self
            .focused()
            .and_then(|track| self.rows.iter().position(|row| *row == track));
        let next = match current {
            None => Some(0),
            Some(position) => offset(position, steps, self.rows.len()),
        };
        if let Some((next, track)) = next.and_then(|next| Some((next, *self.rows.get(next)?))) {
            if extend {
                self.selection.extend_to(track, &self.rows);
                cx.notify();
            } else {
                self.select(Some(track), cx);
            }
            self.scroll.scroll_to_item(next, ScrollStrategy::Nearest);
        }
    }

    /// Moves through the sidebar: the crates shown, then the pages of the history.
    fn move_crate_cursor(&mut self, steps: i32, cx: &mut Context<'_, Self>) {
        let crate_count = self.visible_crates.len();
        let current = match self.history_page {
            Some(page) => {
                crate_count
                    + self
                        .history
                        .pages
                        .iter()
                        .position(|view| view.page == page)
                        .unwrap_or(0)
            }
            None => self
                .visible_crates
                .iter()
                .position(|index| *index == self.crate_index)
                .unwrap_or(0),
        };
        let Some(next) = offset(current, steps, crate_count + self.history.pages.len()) else {
            return;
        };
        match next.checked_sub(crate_count) {
            Some(page_position) => {
                if let Some(view) = self.history.pages.get(page_position) {
                    self.open_history(view.page, cx);
                }
            }
            None => {
                if let Some(crate_index) = self.visible_crates.get(next).copied() {
                    self.choose_crate(crate_index, cx);
                }
            }
        }
    }

    fn select(&mut self, track: Option<TrackIndex>, cx: &mut Context<'_, Self>) {
        self.selection.select_only(track);
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

    /// Marks the tracks played this session, a minute or more on a deck as the history
    /// counts them, which the rows show and `played:` filters read.
    pub fn set_played(&mut self, played: Arc<HashSet<PathBuf>>, cx: &mut Context<'_, Self>) {
        self.played = played;
        if self.query.has_filters() {
            self.refilter(cx);
        }
        cx.notify();
    }

    /// Whether `entry` was played this session.
    fn was_played(&self, entry: &CollectionTrack) -> bool {
        entry
            .track
            .file_path
            .as_ref()
            .is_some_and(|path| self.played.contains(path))
    }

    /// Hide played: puts `played:no` in the search, or takes it out, so the chip and the
    /// typed filter are one filter.
    fn toggle_hide_played(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let text = toggle_hide_played(&self.search_text);
        self.search_input
            .update(cx, |input, cx| input.set_value(text.clone(), window, cx));
        self.set_search_text(text, cx);
        self.reveal_selection();
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
            played: self.was_played(entry),
            metadata: self.track_metadata.facts_for(&entry.identity()),
        }
    }

    /// The filters to offer for what is being typed, from the reference and selected tracks.
    fn search_suggestions(&self) -> Vec<String> {
        let reference = self.view.reference();
        let genre = match (&self.library, self.focused()) {
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
            tag_names: self.track_metadata.tag_names().to_vec(),
        };
        suggestions(&self.search_text, &context)
    }

    fn choose_crate(&mut self, crate_index: CrateIndex, cx: &mut Context<'_, Self>) {
        self.crate_index = crate_index;
        self.history_page = None;
        if let LibraryStatus::Ready(collection) = &self.library {
            self.chosen_folder = collection.crate_at(crate_index).path.clone();
        }
        self.folder_tree_changed(cx);
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
        self.selection.retain(&self.rows);
        cx.notify();
    }

    fn render_sidebar(
        &self,
        collection: &Collection,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        let crates_have_cursor = self.pane == BrowserPane::Crates;
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
            .border_color(theme::active().line)
            .bg(theme::active().background)
            .overflow_y_scroll()
            .children(self.visible_crates.iter().map(|&crate_index| {
                let entry = collection.crate_at(crate_index);
                let chosen = self.history_page.is_none() && crate_index == self.crate_index;
                let count: SharedString = match entry.kind {
                    CrateKind::Folder => "".into(),
                    CrateKind::Smart => "smart".into(),
                    CrateKind::Collection | CrateKind::Playlist | CrateKind::MusicFolder => {
                        entry.tracks.len().to_string().into()
                    }
                };
                let is_folder = entry.kind == CrateKind::MusicFolder;
                let expanded = entry
                    .path
                    .as_ref()
                    .is_some_and(|path| self.expanded_folders.contains(path));
                let leading = is_folder
                    .then(|| folder_disclosure(crate_index, entry.has_subfolders, expanded, cx));
                let hint = entry
                    .path
                    .as_ref()
                    .map(|path| SharedString::from(path.display().to_string()));
                sidebar_row(
                    ("crate", crate_index.position()),
                    entry.name.clone(),
                    count,
                    entry.depth,
                    chosen,
                    chosen && crates_have_cursor,
                    RowExtras {
                        action: (chosen
                            && matches!(entry.kind, CrateKind::Playlist | CrateKind::MusicFolder))
                        .then(|| export_crate_button(crate_index, cx).into_any_element()),
                        leading,
                        hint,
                    },
                )
                .when(is_folder && entry.has_subfolders, |row| {
                    row.aria_expanded(expanded)
                })
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.pane = BrowserPane::Crates;
                    this.choose_crate(crate_index, cx);
                }))
            }))
            .children(
                self.history
                    .pages
                    .iter()
                    .enumerate()
                    .map(|(position, view)| {
                        let page = view.page;
                        let chosen = self.history_page == Some(page);
                        let (name, depth): (SharedString, usize) = match page {
                            HistoryPage::ThisSession => ("History".into(), 0),
                            HistoryPage::Day(_) => (view.label.clone(), 1),
                        };
                        sidebar_row(
                            ("history-page", position),
                            name,
                            view.rows.len().to_string().into(),
                            depth,
                            chosen,
                            chosen && crates_have_cursor,
                            RowExtras::default(),
                        )
                        .when(page == HistoryPage::ThisSession, |row| row.mt(px(12.0)))
                        .on_click(cx.listener(
                            move |this, _: &ClickEvent, _, cx| {
                                this.pane = BrowserPane::Crates;
                                this.open_history(page, cx);
                            },
                        ))
                    }),
            )
    }

    /// A page of the history: when each track started, on which deck, and what it was, the
    /// latest first, with the tracklist exports above.
    fn render_history(&self, page: HistoryPage, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let view = self.history.page(page);
        let rows = view.map_or(&[][..], |view| view.rows.as_slice());
        let title: SharedString = match (page, view) {
            (HistoryPage::ThisSession, _) => "History, this session".into(),
            (HistoryPage::Day(_), Some(view)) => format!("History, {}", view.label).into(),
            (HistoryPage::Day(day), None) => format!("History, {day}").into(),
        };
        let count = rows.len();
        let bar = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.0))
            .h(FILTER_BAR_HEIGHT)
            .px(theme::GUTTER)
            .border_b_1()
            .border_color(theme::active().line)
            .child(div().font_weight(gpui::FontWeight::MEDIUM).child(title))
            .child(
                div()
                    .font_family(MONO_FAMILY)
                    .text_size(theme::TINY_TEXT_SIZE)
                    .text_color(theme::active().muted)
                    .child(format!(
                        "{count} track{}",
                        if count == 1 { "" } else { "s" }
                    )),
            )
            .child(div().flex_1())
            .when(count > 0, |bar| {
                bar.children(TracklistFormat::ALL.map(|format| {
                    button(
                        ("history-export", format as usize),
                        format!("Export {}", format.label()),
                        Emphasis::Secondary,
                    )
                    .tooltip(tooltip(match format {
                        TracklistFormat::Text => {
                            "Save these tracks as a text tracklist, each with its time into the mix"
                        }
                        TracklistFormat::Csv => {
                            "Save these tracks as a spreadsheet: start, deck, artist, title and \
                             how long each played"
                        }
                    }))
                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                        cx.emit(BrowserEvent::ExportHistory { page, format });
                    }))
                }))
            });
        let header = div()
            .flex()
            .flex_none()
            .items_center()
            .h(px(30.0))
            .text_size(theme::SMALL_TEXT_SIZE)
            .text_color(theme::active().muted)
            .font_weight(gpui::FontWeight::MEDIUM)
            .child(history_cell(HistoryColumn::Time).child("Time"))
            .child(history_cell(HistoryColumn::Deck).child("Deck"))
            .child(history_cell(HistoryColumn::Title).child("Title"))
            .child(history_cell(HistoryColumn::Artist).child("Artist"));
        let body = if rows.is_empty() {
            div()
                .px(theme::GUTTER)
                .py(px(12.0))
                .text_color(theme::active().muted)
                .child("A track shows here once a deck has played it for a minute.")
                .into_any_element()
        } else {
            div()
                .id("history-rows")
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .children(rows.iter().enumerate().map(|(position, row)| {
                    let deck_colour = row.deck.map_or(theme::active().muted, theme::deck_colour);
                    div()
                        .id(("history-row", position))
                        .role(gpui::Role::Row)
                        .aria_row_index(position + 2)
                        .aria_label(format!(
                            "{}, deck {}, {} by {}",
                            row.time, row.deck_letter, row.title, row.artist
                        ))
                        .flex()
                        .flex_none()
                        .items_center()
                        .h(ROW_HEIGHT)
                        .border_t_1()
                        .border_color(theme::active().row_line)
                        .child(
                            history_cell(HistoryColumn::Time)
                                .font_family(MONO_FAMILY)
                                .text_size(theme::SMALL_TEXT_SIZE)
                                .text_color(theme::active().muted)
                                .child(row.time.clone()),
                        )
                        .child(
                            history_cell(HistoryColumn::Deck)
                                .font_family(MONO_FAMILY)
                                .text_size(theme::TINY_TEXT_SIZE)
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(deck_colour)
                                .child(row.deck_letter.clone()),
                        )
                        .child(
                            history_cell(HistoryColumn::Title).child(
                                div()
                                    .truncate()
                                    .min_w_0()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .child(row.title.clone()),
                            ),
                        )
                        .child(
                            history_cell(HistoryColumn::Artist)
                                .text_color(theme::active().muted)
                                .child(div().truncate().min_w_0().child(row.artist.clone())),
                        )
                }))
                .into_any_element()
        };
        div()
            .id("history-table")
            .role(gpui::Role::Grid)
            .aria_row_count(count + 1)
            .aria_label("Play history")
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(theme::active().background)
            .child(bar)
            .child(header)
            .child(body)
    }

    /// The table's columns: all of them, or the ones a narrow window keeps.
    fn columns(&self) -> &'static [Column] {
        if self.narrow {
            &Column::NARROW
        } else {
            &Column::ALL
        }
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
            .aria_column_count(self.columns().len() + usize::from(self.actions() != Actions::None))
            .aria_label("Library tracks")
            .aria_description("Use Up and Down to select a track. Full title and artist appear below the list. Shift Left or Shift Right loads a deck.")
            .tab_index(0)
            .focus_visible(|style| style.border_1().border_color(theme::active().text))
            .track_focus(&self.table_focus)
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(theme::active().background)
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
                        .text_color(theme::active().muted)
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
            .when_some(self.focused(), |table, selected| {
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
                        .border_t_1().border_color(theme::active().line)
                        .child(div().font_weight(gpui::FontWeight::MEDIUM).child(entry.row.title.clone()))
                        .child(div().text_color(theme::active().muted).child(entry.row.artist.clone()))
                        .when_some(entry.track.file_path.as_ref().and_then(|path| self.analyses.get(path)).and_then(|answer| answer.as_ref().err()), |details, problem| {
                            details.child(div().text_color(theme::active().alert).child(format!("Analysis failed: {problem}. Rescan this music folder to retry.")))
                        })
                )
            })
    }

    /// The quick filters above the tracks, and how many tracks they leave.
    /// Hide played: only the tracks not played yet this session, as Engine DJ's match filter
    /// offers; the same filter as typing `played:no`.
    fn played_filter(&self, cx: &mut Context<'_, Self>) -> impl IntoElement + use<> {
        let on = self.query.hides_played();
        filter_chip("played-filter", "Hide played", on)
            .tooltip(tooltip(if on {
                "Show the tracks played this session again"
            } else {
                "Hide the tracks played for a minute or more this session (played:no)"
            }))
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                this.toggle_hide_played(window, cx);
            }))
    }

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
            .border_color(theme::active().line)
            .child(self.bpm_filter(cx))
            .child(self.key_filter(cx))
            .child(self.match_filter(cx))
            .child(self.played_filter(cx))
            .child(div().flex_1())
            .child(
                div()
                    .flex_none()
                    .font_family(MONO_FAMILY)
                    .text_size(theme::TINY_TEXT_SIZE)
                    .text_color(theme::active().muted)
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
                .border_color(theme::active().line)
                .overflow_x_hidden()
                .child(
                    div()
                        .flex_none()
                        .text_size(theme::TINY_TEXT_SIZE)
                        .text_color(theme::active().muted)
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
                        .border_color(theme::active().line)
                        .font_family(MONO_FAMILY)
                        .text_size(theme::TINY_TEXT_SIZE)
                        .text_color(theme::active().muted)
                        .cursor_pointer()
                        .hover(|chip| {
                            chip.text_color(theme::active().text)
                                .bg(theme::active().raised)
                        })
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
            .border_color(theme::active().line)
            .bg(theme::active().panel)
            .child(toggle.border_0())
            .child(bpm_stepper(RangeEnd::Lowest, range.lowest, cx))
            .child(div().text_color(theme::active().muted).child("\u{2013}"))
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
            .text_color(theme::active().muted)
            .font_weight(gpui::FontWeight::MEDIUM)
            .children(
                self.columns()
                    .iter()
                    .enumerate()
                    .map(|(position, &column)| {
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
                                .text_color(theme::active().text)
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
                            .aria_column_index(position + 1)
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .h_full()
                            .gap(px(4.0))
                            .when(column == Column::Title, |header| header.pl(px(22.0)))
                            .cursor_pointer()
                            .hover(|header| header.text_color(theme::active().text))
                            .when(direction.is_some(), |header| {
                                header.text_color(theme::active().text)
                            })
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
                    }),
            )
            .when(actions != Actions::None, |row| {
                row.child(
                    div()
                        .id("action-column-header")
                        .role(gpui::Role::ColumnHeader)
                        .aria_column_index(self.columns().len() + 1)
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
        let selected = self.selection.contains(track);
        let missing = self.missing.contains(&track);
        let in_set = self.in_set.contains(&track);
        let played = self.was_played(entry);
        let match_cell = entry
            .track
            .file_path
            .as_deref()
            .map(|path| MatchCell::of(&self.matches, path));
        let dragged = DraggedTrack {
            track,
            title: entry.row.title.clone(),
        };
        let has_path = entry.track.file_path.is_some();
        let menu_facts = RowFacts {
            place: RowPlace::Library,
            file_present: has_path && !missing,
            has_path,
            can_separate: matches!(
                self.stems_status(entry),
                StemsStatus::None | StemsStatus::Failed(_)
            ),
        };
        let keys = self.table_focus.clone();
        let menu_tags = TrackTags::new(
            self.track_metadata.for_track(&entry.identity()),
            Arc::clone(&self.track_metadata),
        );
        div()
            .id(("track", position))
            .role(gpui::Role::Row)
            .aria_row_index(position + 2)
            .aria_label(format!(
                "{} by {}{}",
                entry.row.title,
                entry.row.artist,
                if played { ", played" } else { "" }
            ))
            .aria_selected(selected)
            .aria_position_in_set(position + 1)
            .aria_size_of_set(self.rows.len())
            .when(selected, |row| row.aria_active_descendant())
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                let modifiers = event.modifiers();
                if !modifiers.platform && !modifiers.shift {
                    this.select(Some(track), cx);
                }
            }))
            .group(ROW_GROUP)
            .flex()
            .items_center()
            .h(ROW_HEIGHT)
            .w_full()
            .border_t_1()
            .border_color(theme::active().row_line)
            .when(selected, |row| row.bg(theme::active().selected_row))
            .hover(|row| row.bg(theme::active().selected_row))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.press_row(track, event.modifiers, window, cx);
                    if event.click_count == 2 {
                        cx.emit(BrowserEvent::LoadOnFreeDeck(track));
                    }
                    cx.stop_propagation();
                }),
            )
            .on_drag(dragged, |dragged, _, _, cx| cx.new(|_| dragged.clone()))
            .when(played, |row| row.opacity(PLAYED_ROW_OPACITY))
            .children(
                self.columns()
                    .iter()
                    .enumerate()
                    .map(|(position, &column)| {
                        let text = self.cell_text(entry, track, column);
                        let cell = column_box(column)
                            .id(("track-cell", column as usize))
                            .role(gpui::Role::Cell)
                            .aria_column_index(position + 1)
                            .aria_label(match (column, match_cell) {
                                (Column::Match, Some(found)) => found.description(played).into(),
                                (Column::Rating, _) => rating_description(
                                    u8::try_from(text.chars().count()).unwrap_or(0),
                                )
                                .into(),
                                (Column::Colour, _) if text.is_empty() => "No colour".into(),
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
                                        .when(in_set, |dot| dot.bg(theme::active().ok)),
                                )
                                .child(
                                    div()
                                        .truncate()
                                        .min_w_0()
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .when(played, |title| {
                                            title.text_color(theme::active().played_title)
                                        })
                                        .when(missing, |title| {
                                            title.text_color(theme::active().muted)
                                        })
                                        .child(text),
                                )
                                .when(played, |cell| {
                                    cell.child(
                                        svg()
                                            .path(PLAYED_ICON_PATH)
                                            .size(px(12.0))
                                            .flex_none()
                                            .text_color(theme::active().muted),
                                    )
                                }),
                            Column::Artist => cell
                                .text_color(theme::active().muted)
                                .child(div().truncate().min_w_0().child(text)),
                            Column::Genre => cell
                                .text_size(theme::SMALL_TEXT_SIZE)
                                .text_color(theme::active().muted)
                                .child(div().truncate().min_w_0().child(text)),
                            Column::Bpm | Column::Key | Column::Energy => cell
                                .font_family(MONO_FAMILY)
                                .text_size(theme::SMALL_TEXT_SIZE)
                                .child(text),
                            Column::Rating => cell
                                .text_size(theme::SMALL_TEXT_SIZE)
                                .text_color(theme::active().text)
                                .when(!text.is_empty(), |cell| {
                                    cell.child(div().whitespace_nowrap().child(text.clone()))
                                }),
                            Column::Colour => {
                                let swatch = self
                                    .track_metadata
                                    .for_track(&entry.identity())
                                    .and_then(|stored| stored.colour);
                                cell.children(swatch.map(|colour| {
                                    div()
                                        .size(SWATCH_SIZE)
                                        .flex_none()
                                        .rounded(px(3.0))
                                        .bg(theme::active().track_colour(colour))
                                }))
                            }
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
                                    .text_color(theme::active().muted)
                                    .when(separable, |cell| {
                                        cell.cursor_pointer()
                                            .child(
                                                div()
                                                    .opacity(0.0)
                                                    .group_hover(ROW_GROUP, |label| {
                                                        label.opacity(1.0)
                                                    })
                                                    .hover(|label| {
                                                        label.text_color(theme::active().text)
                                                    })
                                                    .child(SEPARATE_LABEL),
                                            )
                                            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                                cx.stop_propagation()
                                            })
                                            .on_click(cx.listener(
                                                move |_, _: &ClickEvent, _, cx| {
                                                    cx.emit(BrowserEvent::SeparateStems(track));
                                                },
                                            ))
                                    })
                                    .when(pending, |cell| {
                                        cell.cursor_pointer()
                                            .hover(|cell| cell.text_color(theme::active().text))
                                            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                                cx.stop_propagation()
                                            })
                                            .on_click(cx.listener(
                                                move |_, _: &ClickEvent, _, cx| {
                                                    cx.emit(BrowserEvent::CancelStems(track));
                                                },
                                            ))
                                    })
                                    .when(!separable, |cell| cell.child(text))
                            }
                            Column::Time => cell
                                .font_family(MONO_FAMILY)
                                .text_size(theme::SMALL_TEXT_SIZE)
                                .text_color(theme::active().muted)
                                .child(text),
                            Column::Source => cell
                                .text_size(theme::SMALL_TEXT_SIZE)
                                .text_color(if missing {
                                    theme::active().attention
                                } else {
                                    theme::active().muted
                                })
                                .child(div().truncate().min_w_0().child(text)),
                        }
                    }),
            )
            .when(actions != Actions::None, |row| {
                row.child(
                    div()
                        .id("actions-cell")
                        .role(gpui::Role::Cell)
                        .aria_column_index(self.columns().len() + 1)
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
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                    this.take_row_for_menu(track, window, cx);
                }),
            )
            .context_menu(move |menu, window, cx| {
                track_menu::build(menu, menu_facts, Some(&menu_tags), keys.clone(), window, cx)
            })
    }

    /// Pressing a row: Command adds it to the selection or takes it out, Shift selects from
    /// where the selection started, and a plain press selects it alone, unless it is part of
    /// the selection already, which a click on it then narrows to it.
    fn press_row(
        &mut self,
        track: TrackIndex,
        modifiers: gpui::Modifiers,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        window.focus(&self.table_focus, cx);
        self.pane = BrowserPane::Tracks;
        if modifiers.platform {
            self.selection.toggle(track, &self.rows);
            cx.notify();
        } else if modifiers.shift {
            self.selection.select_range(track, &self.rows);
            cx.notify();
        } else if !self.selection.contains(track) {
            self.select(Some(track), cx);
        } else {
            self.selection.focus(track);
            cx.notify();
        }
        cx.emit(BrowserEvent::TookKeys);
    }

    /// A right-click: the menu acts on the selection when the row is in it, else on the row
    /// alone.
    fn take_row_for_menu(
        &mut self,
        track: TrackIndex,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        window.focus(&self.table_focus, cx);
        self.pane = BrowserPane::Tracks;
        if self.selection.contains(track) {
            self.selection.focus(track);
            cx.notify();
        } else {
            self.select(Some(track), cx);
        }
        cx.emit(BrowserEvent::TookKeys);
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
        Column::Rating => fixed(cell, RATING_WIDTH),
        Column::Colour => fixed(cell, COLOUR_WIDTH),
        Column::Match => fixed(cell, MATCH_WIDTH),
        Column::Stems => fixed(cell, STEMS_WIDTH).justify_end(),
        Column::Time => fixed(cell, TIME_WIDTH).justify_end(),
        Column::Source => fixed(cell, SOURCE_WIDTH),
    }
}

/// A column of the history table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HistoryColumn {
    Time,
    Deck,
    Title,
    Artist,
}

/// The box a history column's cells sit in, the same in the header and every row.
fn history_cell(column: HistoryColumn) -> gpui::Div {
    let cell = div().flex().items_center().min_w_0().whitespace_nowrap();
    match column {
        HistoryColumn::Time => cell.flex_none().w(HISTORY_TIME_WIDTH).pl(theme::GUTTER),
        HistoryColumn::Deck => cell.flex_none().w(HISTORY_DECK_WIDTH).px(px(8.0)),
        HistoryColumn::Title => cell.flex_basis(px(0.0)).flex_grow(TITLE_SHARE).px(px(8.0)),
        HistoryColumn::Artist => cell
            .flex_basis(px(0.0))
            .flex_grow(ARTIST_SHARE)
            .pl(px(8.0))
            .pr(theme::GUTTER),
    }
}

/// An entry of the sidebar: a crate, or a page of the history.
/// What a sidebar row can carry besides its name and count.
#[derive(Default)]
struct RowExtras {
    /// A button before the count.
    action: Option<gpui::AnyElement>,
    /// What comes before the name, such as a disclosure triangle.
    leading: Option<gpui::AnyElement>,
    /// What hovering the name shows, such as the folder's whole path.
    hint: Option<SharedString>,
}

fn sidebar_row(
    id: impl Into<gpui::ElementId>,
    name: SharedString,
    count: SharedString,
    depth: usize,
    chosen: bool,
    has_cursor: bool,
    extras: RowExtras,
) -> gpui::Stateful<gpui::Div> {
    let RowExtras {
        action,
        leading,
        hint,
    } = extras;
    div()
        .id(id)
        .accessible_toggle(name.clone(), chosen)
        .flex()
        .flex_none()
        .items_center()
        .justify_between()
        .h(SIDEBAR_ROW_HEIGHT)
        .pl(px(8.0) + px(12.0) * depth as f32)
        .pr(px(8.0))
        .rounded(theme::SMALL_RADIUS)
        .cursor_pointer()
        .text_color(if chosen {
            theme::active().text
        } else {
            theme::active().muted
        })
        .when(chosen, |row| {
            row.bg(theme::active().raised)
                .font_weight(gpui::FontWeight::MEDIUM)
        })
        .hover(|row| row.text_color(theme::active().text))
        .border_1()
        .border_color(if has_cursor {
            theme::active().muted
        } else {
            theme::faded(theme::active().muted, 0.0)
        })
        .children(leading)
        .child(
            div()
                .id("crate-name")
                .flex_1()
                .min_w_0()
                .truncate()
                .child(name)
                .when_some(hint, |label, hint| label.tooltip(tooltip(hint))),
        )
        .children(action)
        .child(
            div()
                .font_family(MONO_FAMILY)
                .text_size(theme::TINY_TEXT_SIZE)
                .text_color(theme::active().muted)
                .child(count),
        )
}

/// The disclosure triangle of a music folder's row, a blank of the same width when it has no
/// subfolders so the names line up.
fn folder_disclosure(
    crate_index: CrateIndex,
    has_subfolders: bool,
    expanded: bool,
    cx: &mut Context<'_, Browser>,
) -> gpui::AnyElement {
    let box_ = div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(16.0))
        .mr(px(2.0));
    if !has_subfolders {
        return box_.into_any_element();
    }
    box_.id(("folder-disclosure", crate_index.position()))
        .accessible_button(if expanded {
            "Collapse folder"
        } else {
            "Expand folder"
        })
        .cursor_pointer()
        .text_size(theme::TINY_TEXT_SIZE)
        .text_color(theme::active().muted)
        .hover(|arrow| arrow.text_color(theme::active().text))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.toggle_folder(crate_index, !expanded, cx);
        }))
        .child(if expanded { "\u{25BE}" } else { "\u{25B8}" })
        .into_any_element()
}

/// Exports the chosen playlist or music folder to a USB stick for CDJs.
fn export_crate_button(
    crate_index: CrateIndex,
    cx: &mut Context<'_, Browser>,
) -> impl IntoElement + use<> {
    div()
        .id(("export-crate", crate_index.position()))
        .accessible_button("Export to USB")
        .flex_none()
        .px(px(4.0))
        .mr(px(4.0))
        .rounded(theme::SMALL_RADIUS)
        .text_size(theme::TINY_TEXT_SIZE)
        .text_color(theme::active().muted)
        .hover(|button| {
            button
                .bg(theme::active().panel)
                .text_color(theme::active().text)
        })
        .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
            cx.stop_propagation();
            cx.emit(BrowserEvent::ExportCrateToUsb(crate_index));
        }))
        .child("USB\u{2026}")
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
                chip.border_color(theme::active().line)
                    .bg(theme::active().raised)
                    .text_color(theme::active().text)
                    .font_weight(gpui::FontWeight::MEDIUM)
            } else {
                chip.border_color(theme::active().line)
                    .text_color(theme::active().muted)
                    .hover(|chip| {
                        chip.text_color(theme::active().text)
                            .bg(theme::active().panel)
                    })
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
            .text_color(theme::active().muted)
            .cursor_pointer()
            .hover(|arrow| {
                arrow
                    .bg(theme::active().raised)
                    .text_color(theme::active().text)
            })
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
        .text_color(theme::active().muted)
        .cursor_pointer()
        .hover(|button| {
            button
                .bg(theme::active().raised)
                .text_color(theme::active().text)
        })
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(on_click)
        .tooltip(tooltip("Show every tempo"))
        .child(
            svg()
                .path(CLOSE_ICON_PATH)
                .size(px(12.0))
                .text_color(theme::active().muted),
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
        .border_color(theme::active().line)
        .font_family(MONO_FAMILY)
        .text_size(theme::TINY_TEXT_SIZE)
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(colour)
        .cursor_pointer()
        .hover(move |button| button.bg(colour).text_color(theme::active().on_deck))
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
        .border_color(theme::active().line)
        .bg(theme::active().panel)
        .text_size(theme::SMALL_TEXT_SIZE)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .size(px(6.0))
                        .rounded_full()
                        .bg(theme::active().attention),
                )
                .child(format!(
                    "{count} file{} moved or missing since rekordbox last saw {}",
                    if count == 1 { "" } else { "s" },
                    if count == 1 { "it" } else { "them" }
                )),
        )
        .child(if relinking {
            div()
                .text_color(theme::active().muted)
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
        .border_color(theme::active().line)
        .text_size(theme::TINY_TEXT_SIZE)
        .text_color(theme::active().attention)
        .cursor_pointer()
        .hover(|button| button.bg(theme::active().raised))
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
        .border_color(theme::active().line)
        .bg(theme::active().panel)
        .text_color(theme::active().muted)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            svg()
                .path(SEARCH_ICON_PATH)
                .size(px(14.0))
                .flex_none()
                .text_color(theme::active().muted),
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
        self.narrow = window.viewport_size().width < NARROW_WINDOW_WIDTH;
        let body = div()
            .flex()
            .size_full()
            .min_h_0()
            .bg(theme::active().background);
        let searching = self.search_has_focus(window, cx);
        match self.library.clone() {
            LibraryStatus::Ready(collection) => {
                let body = body.child(self.render_sidebar(&collection, cx));
                match self.history_page {
                    Some(page) => body.child(self.render_history(page, cx)),
                    None => body.child(self.render_table(collection, searching, cx)),
                }
            }
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
            .child(div().text_color(theme::active().muted).child(
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
        .child(div().text_color(theme::active().text).child(title))
        .children(detail.map(|detail| {
            div()
                .max_w(px(640.0))
                .text_color(theme::active().muted)
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
