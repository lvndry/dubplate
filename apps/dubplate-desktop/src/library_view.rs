//! How the DJ orders and narrows the library's tracks: the column the table is sorted by, a
//! BPM range, and the key filter that keeps the tracks the Next suggestions would accept. Each
//! reads a column's value as the table shows it, so a column sorts and filters by whatever
//! fills it.

use std::cmp::Ordering;

use dubplate_prep::{Key, KeyFit};
use gpui::SharedString;
use serde::{Deserialize, Serialize};

use crate::commands::{Direction, Mode};
use crate::track_metadata::{colour_named, colour_rank};

/// A column of the library's track table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Column {
    /// The title.
    Title,
    /// The artist.
    Artist,
    /// The genre, or the folders the track is filed in.
    Genre,
    /// The tempo.
    Bpm,
    /// The key.
    Key,
    /// The energy, from 1 to 10.
    Energy,
    /// The rating, from no stars to five.
    Rating,
    /// The colour label.
    Colour,
    /// How the track mixes after the tempo master's, ranked as the Next suggestions are.
    Match,
    /// Whether the track's stems are separated, being separated or not asked for.
    Stems,
    /// The length.
    Time,
    /// rekordbox or the music folder.
    Source,
}

impl Column {
    /// Every column, in the table's order.
    pub const ALL: [Self; 12] = [
        Self::Title,
        Self::Artist,
        Self::Genre,
        Self::Bpm,
        Self::Key,
        Self::Energy,
        Self::Rating,
        Self::Colour,
        Self::Match,
        Self::Stems,
        Self::Time,
        Self::Source,
    ];

    /// The columns a narrow window keeps, what a DJ reads while mixing: without the genre, the
    /// rating, the colour, the stems and the source, so the title and artist keep room to read and every header stays
    /// a whole word.
    pub const NARROW: [Self; 7] = [
        Self::Title,
        Self::Artist,
        Self::Bpm,
        Self::Key,
        Self::Energy,
        Self::Match,
        Self::Time,
    ];

    /// The column's header.
    pub fn label(self) -> &'static str {
        match self {
            Self::Title => "Title",
            Self::Artist => "Artist",
            Self::Genre => "Genre",
            Self::Bpm => "BPM",
            Self::Key => "Key",
            Self::Energy => "Energy",
            Self::Rating => "Rating",
            Self::Colour => "Colour",
            Self::Match => "Match",
            Self::Stems => "Stems",
            Self::Time => "Time",
            Self::Source => "Source",
        }
    }

    /// The name the settings keep the column by.
    fn name(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Artist => "artist",
            Self::Genre => "genre",
            Self::Bpm => "bpm",
            Self::Key => "key",
            Self::Energy => "energy",
            Self::Rating => "rating",
            Self::Colour => "colour",
            Self::Match => "match",
            Self::Stems => "stems",
            Self::Time => "time",
            Self::Source => "source",
        }
    }

    /// Whether the column holds numbers, which line up on the right.
    pub fn is_numeric(self) -> bool {
        matches!(self, Self::Bpm | Self::Energy | Self::Time)
    }

    /// A cell's text as a value to sort by, or `None` for an empty cell.
    fn value(self, text: &str) -> Option<CellValue> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        match self {
            Self::Title | Self::Artist | Self::Genre | Self::Stems | Self::Source => {
                Some(CellValue::Text(text.to_lowercase()))
            }
            Self::Bpm | Self::Energy | Self::Match => text
                .parse::<f64>()
                .ok()
                .filter(|number| number.is_finite())
                .map(CellValue::Number),
            Self::Key => {
                Key::parse_file_key(text).map(|key| CellValue::Number(wheel_position(key)))
            }
            Self::Time => clock_seconds(text).map(CellValue::Number),
            Self::Rating => Some(CellValue::Number(
                text.chars().filter(|star| *star == '\u{2605}').count() as f64,
            )),
            Self::Colour => {
                colour_named(text).map(|colour| CellValue::Number(colour_rank(colour) as f64))
            }
        }
    }

    /// Which way the first click on the header sorts: the best rating first, as rekordbox
    /// does, and every other column from its start.
    fn first_direction(self) -> SortDirection {
        match self {
            Self::Rating => SortDirection::Descending,
            _ => SortDirection::Ascending,
        }
    }
}

/// A key's place on the Camelot wheel, 1A, 1B, 2A and on to 12B, so keys sort as a DJ reads
/// them.
fn wheel_position(key: Key) -> f64 {
    let minor_first = if key.camelot().ends_with('A') {
        0.0
    } else {
        0.5
    };
    f64::from(key.camelot_number()) + minor_first
}

/// `m:ss` or `h:mm:ss` as seconds.
fn clock_seconds(text: &str) -> Option<f64> {
    text.split(':').try_fold(0.0, |total: f64, part| {
        let number = part.parse::<u32>().ok()?;
        Some(total * 60.0 + f64::from(number))
    })
}

/// A cell's value as the sort compares it.
#[derive(Debug, Clone, PartialEq)]
enum CellValue {
    Number(f64),
    Text(String),
}

impl CellValue {
    fn compare(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Number(first), Self::Number(second)) => first.total_cmp(second),
            (Self::Text(first), Self::Text(second)) => first.cmp(second),
            (Self::Number(_), Self::Text(_)) => Ordering::Less,
            (Self::Text(_), Self::Number(_)) => Ordering::Greater,
        }
    }
}

/// Which way a sorted column runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    /// A to Z, slowest first, 1A first.
    Ascending,
    /// The other way.
    Descending,
}

/// The column the table is sorted by, and which way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sort {
    /// The column.
    pub column: Column,
    /// Which way.
    pub direction: SortDirection,
}

impl Sort {
    /// What a click on `column`'s header makes of `current`: sort by it, then reverse it, then
    /// go back to the crate's own order.
    pub fn after_click(current: Option<Self>, column: Column) -> Option<Self> {
        match current {
            Some(sort) if sort.column == column => {
                if sort.direction == column.first_direction() {
                    Some(Self {
                        column,
                        direction: match sort.direction {
                            SortDirection::Ascending => SortDirection::Descending,
                            SortDirection::Descending => SortDirection::Ascending,
                        },
                    })
                } else {
                    None
                }
            }
            _ => Some(Self {
                column,
                direction: column.first_direction(),
            }),
        }
    }

    /// Orders `rows` by this column, reading each row's cell with `cell`. Rows with an empty
    /// cell go last whichever way, and rows that tie keep the crate's order.
    pub fn apply<Row: Copy>(self, rows: &mut Vec<Row>, cell: impl Fn(Row) -> SharedString) {
        let mut keyed: Vec<(Option<CellValue>, Row)> = rows
            .iter()
            .map(|row| (self.column.value(&cell(*row)), *row))
            .collect();
        keyed.sort_by(|(first, _), (second, _)| match (first, second) {
            (Some(first), Some(second)) => {
                let order = first.compare(second);
                match self.direction {
                    SortDirection::Ascending => order,
                    SortDirection::Descending => order.reverse(),
                }
            }
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        });
        *rows = keyed.into_iter().map(|(_, row)| row).collect();
    }
}

/// A screen's sort as the settings keep it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SortSetting {
    /// The column's name, such as `bpm`.
    pub column: String,
    /// Whether it runs the other way.
    #[serde(default)]
    pub descending: bool,
}

impl SortSetting {
    /// The setting for `sort`.
    pub fn of(sort: Sort) -> Self {
        Self {
            column: sort.column.name().to_owned(),
            descending: sort.direction == SortDirection::Descending,
        }
    }

    /// The sort, or `None` for a column this version does not have.
    pub fn sort(&self) -> Option<Sort> {
        let column = Column::ALL
            .into_iter()
            .find(|column| column.name() == self.column)?;
        Some(Sort {
            column,
            direction: if self.descending {
                SortDirection::Descending
            } else {
                SortDirection::Ascending
            },
        })
    }
}

/// The lowest and highest tempos a BPM filter can reach.
const BPM_LIMITS: (u16, u16) = (40, 220);
/// How far either side of a track's tempo a new BPM range reaches.
const BPM_SPAN: u16 = 3;
/// The tempo a new BPM range centres on when no track has one.
const DEFAULT_BPM: f64 = 120.0;

/// Which end of a BPM range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeEnd {
    /// The slowest tempo kept.
    Lowest,
    /// The fastest tempo kept.
    Highest,
}

/// The tempos a BPM filter keeps, in whole BPM, both ends included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BpmRange {
    /// The slowest tempo kept.
    pub lowest: u16,
    /// The fastest tempo kept.
    pub highest: u16,
}

impl BpmRange {
    /// A few BPM either side of `bpm`, or of 120 when it is unknown.
    pub fn around(bpm: Option<f64>) -> Self {
        let (slowest, fastest) = BPM_LIMITS;
        let centre = bpm
            .filter(|bpm| bpm.is_finite())
            .unwrap_or(DEFAULT_BPM)
            .round()
            .clamp(f64::from(slowest + BPM_SPAN), f64::from(fastest - BPM_SPAN))
            as u16;
        Self {
            lowest: centre - BPM_SPAN,
            highest: centre + BPM_SPAN,
        }
    }

    /// The range with one end a BPM slower or faster, the ends never crossing.
    pub fn step(self, end: RangeEnd, direction: Direction) -> Self {
        let (slowest, fastest) = BPM_LIMITS;
        let moved = |value: u16| match direction {
            Direction::Down => value.saturating_sub(1),
            Direction::Up => value.saturating_add(1),
        };
        match end {
            RangeEnd::Lowest => Self {
                lowest: moved(self.lowest).clamp(slowest, self.highest),
                ..self
            },
            RangeEnd::Highest => Self {
                highest: moved(self.highest).clamp(self.lowest, fastest),
                ..self
            },
        }
    }

    /// Whether a BPM cell's tempo, to the nearest BPM, is in the range. An empty cell is not.
    pub fn keeps(self, text: &str) -> bool {
        match Column::Bpm.value(text) {
            Some(CellValue::Number(bpm)) => {
                let rounded = bpm.round();
                rounded >= f64::from(self.lowest) && rounded <= f64::from(self.highest)
            }
            _ => false,
        }
    }

    /// The tempo of a BPM cell, for a new range around it.
    pub fn tempo_of(text: &str) -> Option<f64> {
        match Column::Bpm.value(text) {
            Some(CellValue::Number(bpm)) => Some(bpm),
            _ => None,
        }
    }
}

/// The track the quick filters start from: the selected set track, or the deck playing.
#[derive(Debug, Clone, PartialEq)]
pub struct FilterReference {
    /// Its title.
    pub title: SharedString,
    /// The key moves from its key, when it has one.
    pub key: Option<KeyFit>,
    /// Its tempo, when known.
    pub bpm: Option<f64>,
}

impl FilterReference {
    /// Whether a Key cell's key follows well from the reference's. An empty cell does not, nor
    /// any cell when the reference has no key.
    pub fn keeps(&self, text: &str) -> bool {
        self.key.is_some_and(|fit| {
            Key::parse_file_key(text).is_some_and(|key| fit.key_move(&key).is_some())
        })
    }
}

/// How the library's table is ordered and narrowed: each screen's sort, the BPM range, and
/// the key filter with the track it starts from.
#[derive(Debug, Clone, Default)]
pub struct LibraryView {
    prepare_sort: Option<Sort>,
    perform_sort: Option<Sort>,
    bpm_range: Option<BpmRange>,
    key_filter: bool,
    reference: Option<FilterReference>,
    match_filter: bool,
    has_match_reference: bool,
}

impl LibraryView {
    /// How `screen` sorts the tracks, or `None` for the crate's own order.
    pub fn sort(&self, screen: Mode) -> Option<Sort> {
        match screen {
            Mode::Prepare => self.prepare_sort,
            Mode::Perform => self.perform_sort,
        }
    }

    /// Sorts `screen`'s tracks by `sort`.
    pub fn set_sort(&mut self, screen: Mode, sort: Option<Sort>) {
        match screen {
            Mode::Prepare => self.prepare_sort = sort,
            Mode::Perform => self.perform_sort = sort,
        }
    }

    /// The tempos kept, when the BPM filter is on.
    pub fn bpm_range(&self) -> Option<BpmRange> {
        self.bpm_range
    }

    /// Turns the BPM filter off, or on around the reference's tempo, else `fallback_bpm`.
    pub fn toggle_bpm_range(&mut self, fallback_bpm: Option<f64>) {
        self.bpm_range = match self.bpm_range {
            Some(_) => None,
            None => Some(BpmRange::around(
                self.reference
                    .as_ref()
                    .and_then(|reference| reference.bpm)
                    .or(fallback_bpm),
            )),
        };
    }

    /// Moves one end of the BPM range a BPM, when the filter is on.
    pub fn step_bpm_range(&mut self, end: RangeEnd, direction: Direction) {
        self.bpm_range = self.bpm_range.map(|range| range.step(end, direction));
    }

    /// Turns the key filter on or off. It only narrows once there is a key to start from.
    pub fn toggle_key_filter(&mut self) {
        self.key_filter = !self.key_filter;
    }

    /// Whether the DJ turned the key filter on.
    pub fn key_filter_on(&self) -> bool {
        self.key_filter
    }

    /// The track the filters start from.
    pub fn reference(&self) -> Option<&FilterReference> {
        self.reference.as_ref()
    }

    /// Takes a new track to start from. Without one the last stays, so the tracks shown do
    /// not jump when a deck stops. Returns whether the tracks kept may change.
    pub fn set_reference(&mut self, reference: Option<FilterReference>) -> bool {
        let Some(reference) = reference else {
            return false;
        };
        let key_changed = self.reference.as_ref().map(|current| current.key) != Some(reference.key);
        self.reference = Some(reference);
        self.key_filter && key_changed
    }

    /// Turns the Matches master filter on or off. It only narrows once there is a track to
    /// match.
    pub fn toggle_match_filter(&mut self) {
        self.match_filter = !self.match_filter;
    }

    /// Whether the Matches master filter narrows the tracks.
    pub fn match_filter_on(&self) -> bool {
        self.match_filter && self.has_match_reference
    }

    /// Says whether the Match column has a track to match against. Returns whether the tracks
    /// kept may change.
    pub fn set_match_reference(&mut self, has_reference: bool) -> bool {
        let changed = self.has_match_reference != has_reference;
        self.has_match_reference = has_reference;
        changed && self.match_filter
    }

    /// Whether a filter narrows the tracks.
    pub fn is_filtered(&self) -> bool {
        self.bpm_range.is_some() || self.active_key_filter().is_some() || self.match_filter_on()
    }

    fn active_key_filter(&self) -> Option<&FilterReference> {
        self.reference
            .as_ref()
            .filter(|reference| self.key_filter && reference.key.is_some())
    }

    /// Keeps the rows the filters let through, then orders them as `screen` sorts, reading
    /// each row's cells with `cell`.
    pub fn apply<Row: Copy>(
        &self,
        screen: Mode,
        rows: &mut Vec<Row>,
        cell: impl Fn(Row, Column) -> SharedString,
    ) {
        let key_filter = self.active_key_filter();
        let match_filter = self.match_filter_on();
        if self.is_filtered() {
            rows.retain(|row| {
                self.bpm_range
                    .is_none_or(|range| range.keeps(&cell(*row, Column::Bpm)))
                    && key_filter.is_none_or(|reference| reference.keeps(&cell(*row, Column::Key)))
                    && (!match_filter || !cell(*row, Column::Match).is_empty())
            });
        }
        if let Some(sort) = self.sort(screen) {
            sort.apply(rows, |row| cell(row, sort.column));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(column: Column, direction: SortDirection, cells: &[&'static str]) -> Vec<usize> {
        let mut rows: Vec<usize> = (0..cells.len()).collect();
        Sort { column, direction }.apply(&mut rows, |row| cells[row].into());
        rows
    }

    #[test]
    fn numbers_sort_as_numbers_and_empty_cells_go_last_both_ways() {
        let tempos = ["122.00", "", "98.50", "113.00"];
        assert_eq!(
            sorted(Column::Bpm, SortDirection::Ascending, &tempos),
            [2, 3, 0, 1]
        );
        assert_eq!(
            sorted(Column::Bpm, SortDirection::Descending, &tempos),
            [0, 3, 2, 1]
        );
        let energies = ["7", "\u{2013}", "10", "3"];
        assert_eq!(
            sorted(Column::Energy, SortDirection::Descending, &energies),
            [2, 0, 3, 1]
        );
    }

    #[test]
    fn keys_sort_round_the_wheel_whatever_notation() {
        let keys = ["9A", "Am", "C", "1B", "12A", ""];
        assert_eq!(
            sorted(Column::Key, SortDirection::Ascending, &keys),
            [3, 1, 2, 0, 4, 5]
        );
    }

    #[test]
    fn times_sort_by_length_and_text_ignores_case() {
        let times = ["10:02", "6:48", "1:02:00", "0:59"];
        assert_eq!(
            sorted(Column::Time, SortDirection::Ascending, &times),
            [3, 1, 0, 2]
        );
        let titles = ["uthando", "Abalele", "bambelela", "Amanxeba"];
        assert_eq!(
            sorted(Column::Title, SortDirection::Ascending, &titles),
            [1, 3, 2, 0]
        );
    }

    #[test]
    fn ties_keep_the_crates_order() {
        let tempos = ["120.00", "118.00", "120.00", "120.00"];
        assert_eq!(
            sorted(Column::Bpm, SortDirection::Descending, &tempos),
            [0, 2, 3, 1]
        );
    }

    #[test]
    fn a_header_click_sorts_then_reverses_then_goes_back_to_the_crate() {
        let ascending = Sort::after_click(None, Column::Bpm);
        assert_eq!(
            ascending,
            Some(Sort {
                column: Column::Bpm,
                direction: SortDirection::Ascending
            })
        );
        let descending = Sort::after_click(ascending, Column::Bpm);
        assert_eq!(
            descending.map(|sort| sort.direction),
            Some(SortDirection::Descending)
        );
        assert_eq!(Sort::after_click(descending, Column::Bpm), None);
        assert_eq!(
            Sort::after_click(descending, Column::Key).map(|sort| sort.column),
            Some(Column::Key)
        );
    }

    #[test]
    fn rating_sorts_the_best_first_and_unrated_tracks_last() {
        let ratings = [
            "\u{2605}\u{2605}",
            "",
            "\u{2605}\u{2605}\u{2605}\u{2605}\u{2605}",
            "\u{2605}",
        ];
        assert_eq!(
            sorted(Column::Rating, SortDirection::Descending, &ratings),
            [2, 0, 3, 1]
        );
        assert_eq!(
            sorted(Column::Rating, SortDirection::Ascending, &ratings),
            [3, 0, 2, 1]
        );
        let first = Sort::after_click(None, Column::Rating);
        assert_eq!(
            first.map(|sort| sort.direction),
            Some(SortDirection::Descending)
        );
        let second = Sort::after_click(first, Column::Rating);
        assert_eq!(
            second.map(|sort| sort.direction),
            Some(SortDirection::Ascending)
        );
        assert_eq!(Sort::after_click(second, Column::Rating), None);
    }

    #[test]
    fn colour_sorts_in_rekordbox_palette_order_and_none_goes_last() {
        let colours = ["Blue", "", "Pink", "Purple", "Orange", "Red"];
        assert_eq!(
            sorted(Column::Colour, SortDirection::Ascending, &colours),
            [2, 5, 4, 0, 3, 1]
        );
        assert_eq!(
            sorted(Column::Colour, SortDirection::Descending, &colours),
            [3, 0, 4, 5, 2, 1]
        );
    }

    #[test]
    fn a_sort_survives_the_settings_and_an_unknown_column_is_dropped() {
        for column in Column::ALL {
            let sort = Sort {
                column,
                direction: SortDirection::Descending,
            };
            assert_eq!(SortSetting::of(sort).sort(), Some(sort));
        }
        let future = SortSetting {
            column: "mood".to_owned(),
            descending: false,
        };
        assert_eq!(future.sort(), None);
    }

    #[test]
    fn a_bpm_range_keeps_its_ends_and_never_crosses() {
        let range = BpmRange::around(Some(112.6));
        assert_eq!(
            range,
            BpmRange {
                lowest: 110,
                highest: 116
            }
        );
        assert!(range.keeps("110.00"));
        assert!(range.keeps("116.40"));
        assert!(!range.keeps("116.60"));
        assert!(!range.keeps(""));
        let narrow = BpmRange {
            lowest: 118,
            highest: 118,
        };
        assert_eq!(
            narrow.step(RangeEnd::Lowest, Direction::Up),
            narrow,
            "the slowest end stops at the fastest"
        );
        assert_eq!(narrow.step(RangeEnd::Highest, Direction::Up).highest, 119);
        assert_eq!(
            BpmRange::around(None),
            BpmRange {
                lowest: 117,
                highest: 123
            }
        );
        assert_eq!(BpmRange::around(Some(300.0)).highest, 220);
    }

    #[test]
    fn sorting_by_match_ranks_the_suggestions_and_the_filter_keeps_only_them() {
        let ranks = ["", "2", "1", ""];
        let cell = |row: usize, column: Column| -> SharedString {
            match column {
                Column::Match => ranks[row].into(),
                _ => SharedString::default(),
            }
        };
        let mut view = LibraryView::default();
        view.set_sort(
            Mode::Perform,
            Some(Sort {
                column: Column::Match,
                direction: SortDirection::Ascending,
            }),
        );
        let mut ranked: Vec<usize> = (0..ranks.len()).collect();
        view.apply(Mode::Perform, &mut ranked, cell);
        assert_eq!(
            ranked,
            [2, 1, 0, 3],
            "best match first, the rest in the crate's order"
        );
        view.toggle_match_filter();
        assert!(!view.is_filtered(), "nothing to match against yet");
        assert!(view.set_match_reference(true));
        let mut matching: Vec<usize> = (0..ranks.len()).collect();
        view.apply(Mode::Perform, &mut matching, cell);
        assert_eq!(matching, [2, 1]);
    }

    #[test]
    fn the_view_narrows_then_sorts_each_screen_its_own_way() {
        let rows: [(&str, &str); 4] = [
            ("122.00", "8A"),
            ("113.00", "8A"),
            ("121.00", "3B"),
            ("", "9A"),
        ];
        let cell = |row: usize, column: Column| -> SharedString {
            match column {
                Column::Bpm => rows[row].0.into(),
                Column::Key => rows[row].1.into(),
                _ => SharedString::default(),
            }
        };
        let mut view = LibraryView::default();
        view.set_sort(
            Mode::Prepare,
            Some(Sort {
                column: Column::Bpm,
                direction: SortDirection::Ascending,
            }),
        );
        let mut prepared: Vec<usize> = (0..rows.len()).collect();
        view.apply(Mode::Prepare, &mut prepared, cell);
        assert_eq!(prepared, [1, 2, 0, 3]);
        let mut performed: Vec<usize> = (0..rows.len()).collect();
        view.apply(Mode::Perform, &mut performed, cell);
        assert_eq!(performed, [0, 1, 2, 3], "Perform keeps the crate's order");

        view.toggle_key_filter();
        assert!(!view.is_filtered(), "no key to start from yet");
        let reference = FilterReference {
            title: "Uthando".into(),
            key: Key::parse_file_key("8A").map(KeyFit::new),
            bpm: Some(120.0),
        };
        assert!(view.set_reference(Some(reference.clone())));
        assert!(
            !view.set_reference(None),
            "a stopped deck keeps the last reference"
        );
        assert_eq!(view.reference(), Some(&reference));
        view.toggle_bpm_range(None);
        assert_eq!(
            view.bpm_range(),
            Some(BpmRange {
                lowest: 117,
                highest: 123
            })
        );
        let mut narrowed: Vec<usize> = (0..rows.len()).collect();
        view.apply(Mode::Perform, &mut narrowed, cell);
        assert_eq!(narrowed, [0]);
    }

    #[test]
    fn the_key_filter_keeps_the_moves_the_suggestions_make() {
        let mut reference = FilterReference {
            title: "Uthando".into(),
            key: Key::parse_file_key("8A").map(KeyFit::new),
            bpm: Some(112.0),
        };
        assert!(reference.keeps("8A"));
        assert!(reference.keeps("9A"));
        assert!(reference.keeps("C"));
        assert!(!reference.keeps("3B"));
        assert!(!reference.keeps(""));
        reference.key = None;
        assert!(
            !reference.keeps("8A"),
            "a reference without a key keeps nothing"
        );
    }
}
