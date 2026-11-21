//! The browser: the crates on the left and the tracks of the chosen crate on the right, filtered
//! by the search field. It asks the window to load tracks through [`BrowserEvent`].

use std::sync::Arc;

use dubplate_engine::DeckId;
use gpui::component::input::{Input, InputEvent, InputState};
use gpui::prelude::*;
use gpui::{
    App, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable, MouseButton,
    MouseDownEvent, Pixels, ScrollStrategy, SharedString, Subscription, UniformListScrollHandle,
    Window, div, px, relative, svg, uniform_list,
};

use crate::assets::SEARCH_ICON_PATH;
use crate::collection::{Collection, CrateIndex, CrateKind, TrackIndex};
use crate::commands::Direction;
use crate::deck::deck_letter;
use crate::fonts::MONO_FAMILY;
use crate::search::{SearchQuery, filter_tracks};
use crate::theme;

const ROW_HEIGHT: Pixels = px(32.0);
const SIDEBAR_WIDTH: Pixels = px(200.0);
const BPM_WIDTH: Pixels = px(72.0);
const KEY_WIDTH: Pixels = px(56.0);
const TIME_WIDTH: Pixels = px(64.0);
const LOAD_WIDTH: Pixels = px(84.0);
const ROW_GROUP: &str = "track-row";

/// Where the rekordbox library stands.
#[derive(Clone)]
pub enum LibraryStatus {
    /// Being copied and read.
    Reading,
    /// Read.
    Ready(Arc<Collection>),
    /// Could not be read, and why.
    Failed(SharedString),
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
    rows: Vec<TrackIndex>,
    selected: Option<TrackIndex>,
    pane: BrowserPane,
    search_input: Entity<InputState>,
    table_focus: FocusHandle,
    scroll: UniformListScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<BrowserEvent> for Browser {}

impl Browser {
    /// An empty browser, waiting for the library.
    pub fn new(window: &mut Window, cx: &mut Context<'_, Self>) -> Self {
        let search_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search title or artist"));
        let subscription = cx.subscribe_in(
            &search_input,
            window,
            |this: &mut Self, input, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    let text = input.read(cx).value();
                    this.set_query(SearchQuery::parse(&text), cx);
                }
                InputEvent::PressEnter { .. } => this.focus_table(window, cx),
                InputEvent::Focus | InputEvent::Blur => {}
            },
        );
        Self {
            library: LibraryStatus::Reading,
            crate_index: CrateIndex::COLLECTION,
            query: SearchQuery::default(),
            rows: Vec::new(),
            selected: None,
            pane: BrowserPane::default(),
            search_input,
            table_focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
            _subscriptions: vec![subscription],
        }
    }

    /// Shows the library as it now stands.
    pub fn set_library(&mut self, library: LibraryStatus, cx: &mut Context<'_, Self>) {
        self.library = library;
        self.refilter(cx);
    }

    /// The search field, which sits in the window's header.
    pub fn search_input(&self) -> &Entity<InputState> {
        &self.search_input
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
        cx.notify();
    }

    fn set_query(&mut self, query: SearchQuery, cx: &mut Context<'_, Self>) {
        self.query = query;
        self.refilter(cx);
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
    }

    fn choose_crate(&mut self, crate_index: CrateIndex, cx: &mut Context<'_, Self>) {
        self.crate_index = crate_index;
        self.refilter(cx);
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
    }

    fn refilter(&mut self, cx: &mut Context<'_, Self>) {
        self.rows = match &self.library {
            LibraryStatus::Ready(collection) => {
                filter_tracks(collection, self.crate_index, &self.query)
            }
            LibraryStatus::Reading | LibraryStatus::Failed(_) => Vec::new(),
        };
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
                    CrateKind::Collection | CrateKind::Playlist => {
                        entry.tracks.len().to_string().into()
                    }
                };
                div()
                    .id(("crate", crate_index.position()))
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
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        let empty_message: Option<SharedString> = if !self.rows.is_empty() {
            None
        } else if !self.query.is_empty() {
            Some("No track matches the search.".into())
        } else {
            Some("This crate is empty.".into())
        };
        div()
            .id("track-table")
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
            .child(header_row())
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
                                    this.render_row(&collection, position, track, cx)
                                })
                                .collect::<Vec<_>>()
                        }),
                    )
                    .flex_1()
                    .track_scroll(&self.scroll),
                ),
            })
    }

    fn render_row(
        &self,
        collection: &Collection,
        position: usize,
        track: TrackIndex,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement + use<> {
        let row = &collection.track(track).row;
        let selected = self.selected == Some(track);
        let dragged = DraggedTrack {
            track,
            title: row.title.clone(),
        };
        div()
            .id(("track", position))
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
            .child(
                text_cell(row.title.clone())
                    .flex_1()
                    .min_w_0()
                    .px(theme::GUTTER)
                    .font_weight(gpui::FontWeight::MEDIUM),
            )
            .child(
                text_cell(row.artist.clone())
                    .w(relative(0.3))
                    .flex_none()
                    .px(px(8.0))
                    .text_color(theme::MUTED),
            )
            .child(number_cell(row.bpm.clone(), BPM_WIDTH).justify_end())
            .child(number_cell(row.key.clone(), KEY_WIDTH))
            .child(
                number_cell(row.time.clone(), TIME_WIDTH)
                    .justify_end()
                    .text_color(theme::MUTED),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .justify_end()
                    .gap(px(4.0))
                    .w(LOAD_WIDTH)
                    .pr(theme::GUTTER)
                    .when(!selected, |buttons| {
                        buttons
                            .opacity(0.0)
                            .group_hover(ROW_GROUP, |style| style.opacity(1.0))
                    })
                    .children(DeckId::ALL.map(|deck| load_button(position, track, deck, cx))),
            )
    }
}

/// `position` moved by `steps` and kept inside `count` rows, or `None` when there are none.
fn offset(position: usize, steps: i32, count: usize) -> Option<usize> {
    let last = count.checked_sub(1)?;
    let moved = i64::try_from(position).ok()? + i64::from(steps);
    Some(usize::try_from(moved.max(0)).ok()?.min(last))
}

fn header_row() -> impl IntoElement {
    let header = |label: &'static str| {
        div()
            .flex()
            .items_center()
            .h_full()
            .whitespace_nowrap()
            .child(label)
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .h(px(30.0))
        .text_size(theme::SMALL_TEXT_SIZE)
        .text_color(theme::MUTED)
        .font_weight(gpui::FontWeight::MEDIUM)
        .child(header("Title").flex_1().px(theme::GUTTER))
        .child(header("Artist").w(relative(0.3)).flex_none().px(px(8.0)))
        .child(
            header("BPM")
                .w(BPM_WIDTH)
                .flex_none()
                .px(px(8.0))
                .justify_end(),
        )
        .child(header("Key").w(KEY_WIDTH).flex_none().px(px(8.0)))
        .child(
            header("Time")
                .w(TIME_WIDTH)
                .flex_none()
                .px(px(8.0))
                .justify_end(),
        )
        .child(
            header("Load")
                .w(LOAD_WIDTH)
                .flex_none()
                .pr(theme::GUTTER)
                .justify_end(),
        )
}

fn text_cell(text: SharedString) -> gpui::Div {
    div().truncate().child(text)
}

fn number_cell(text: SharedString, width: Pixels) -> gpui::Div {
    div()
        .flex()
        .flex_none()
        .w(width)
        .px(px(8.0))
        .font_family(MONO_FAMILY)
        .text_size(theme::SMALL_TEXT_SIZE)
        .child(text)
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let body = div().flex().size_full().min_h_0().bg(theme::BACKGROUND);
        match self.library.clone() {
            LibraryStatus::Ready(collection) => body
                .child(self.render_sidebar(&collection, cx))
                .child(self.render_table(collection, cx)),
            LibraryStatus::Reading => body.child(message_panel(
                "Reading your rekordbox library\u{2026}".into(),
                None,
            )),
            LibraryStatus::Failed(problem) => body.child(message_panel(
                "Dubplate could not read your rekordbox library.".into(),
                Some(problem),
            )),
        }
    }
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
