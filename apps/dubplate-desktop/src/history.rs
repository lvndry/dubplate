//! The play history. A track counts as played once its deck has played that load for a minute
//! of track time, as in rekordbox's History, and only once per load. The browser shows this
//! session's plays and the earlier ones by day.

use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use dubplate_engine::DeckId;
use dubplate_prep::{Play, PlayDeck, PlayId};
use gpui::SharedString;
use jiff::Timestamp;
use jiff::civil::{Date, DateTime};
use jiff::tz::TimeZone;

use crate::deck::deck_letter;

/// How much of a load a deck plays before the track counts as played.
pub const PLAYED_AFTER: Duration = Duration::from_secs(60);

/// A deck as the history watches it at one moment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeckSample {
    /// The load on the deck, or `None` while no track is loaded.
    pub load: Option<u64>,
    /// Whether the deck plays.
    pub playing: bool,
    /// Its speed, 1.0 at the track's own tempo.
    pub tempo: f64,
    /// When the sample was taken.
    pub at: Instant,
    /// The time of day it was taken, for when a play started.
    pub wall_clock: SystemTime,
}

/// A load that has just played long enough to count.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Counted {
    /// When the deck first played the load.
    pub started_at: SystemTime,
    /// How much it has played.
    pub played: Duration,
}

/// A counted load that left the deck, or is being left as Dubplate quits.
#[derive(Debug, Clone, PartialEq)]
pub struct Finished<Record> {
    /// What the play was recorded as.
    pub record: Record,
    /// How much it played in all.
    pub played: Duration,
}

/// Whether the load on a deck has counted yet, and as what.
#[derive(Debug, Clone, PartialEq)]
enum Count<Record> {
    Counting,
    Counted(Option<Record>),
}

/// The previous sample's part in adding up how long a load played.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Previous {
    at: Instant,
    playing: bool,
    tempo: f64,
}

/// One deck's current load, adding up how much of it plays.
///
/// Time counts from one sample to the next when the deck played at the first of them, at the
/// speed it played, so a minute at +8% is reached sooner in the room's time than at 0%.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayWatch<Record> {
    load: Option<u64>,
    started_at: Option<SystemTime>,
    played_seconds: f64,
    previous: Option<Previous>,
    count: Count<Record>,
}

impl<Record> Default for PlayWatch<Record> {
    fn default() -> Self {
        Self {
            load: None,
            started_at: None,
            played_seconds: 0.0,
            previous: None,
            count: Count::Counting,
        }
    }
}

/// What a sample changed.
#[derive(Debug, Clone, PartialEq)]
pub enum PlayEvent<Record> {
    /// The load has just counted as played: record it, then tell the watch what it was
    /// recorded as with [`PlayWatch::counted_as`].
    Counted(Counted),
    /// A counted load left the deck.
    Finished(Finished<Record>),
}

impl<Record> PlayWatch<Record> {
    /// Takes in a sample of the deck and says what it changed.
    pub fn observe(&mut self, sample: DeckSample) -> Option<PlayEvent<Record>> {
        if sample.load != self.load {
            let finished = self.finish().map(PlayEvent::Finished);
            *self = Self {
                load: sample.load,
                ..Self::default()
            };
            self.take(sample);
            return finished;
        }
        self.load?;
        if let Some(previous) = self.previous.filter(|previous| previous.playing) {
            let elapsed = sample.at.saturating_duration_since(previous.at);
            self.played_seconds += elapsed.as_secs_f64() * previous.tempo.max(0.0);
        }
        self.take(sample);
        let counts = matches!(self.count, Count::Counting) && self.played() >= PLAYED_AFTER;
        if !counts {
            return None;
        }
        self.count = Count::Counted(None);
        Some(PlayEvent::Counted(Counted {
            started_at: self.started_at.unwrap_or(sample.wall_clock),
            played: self.played(),
        }))
    }

    fn take(&mut self, sample: DeckSample) {
        if self.load.is_some() && sample.playing && self.started_at.is_none() {
            self.started_at = Some(sample.wall_clock);
        }
        self.previous = Some(Previous {
            at: sample.at,
            playing: sample.playing,
            tempo: sample.tempo,
        });
    }

    /// Keeps what the counted load was recorded as, to hand back when it finishes.
    pub fn counted_as(&mut self, record: Record) {
        if let Count::Counted(slot) = &mut self.count {
            *slot = Some(record);
        }
    }

    /// How much of the load has played so far.
    pub fn played(&self) -> Duration {
        Duration::try_from_secs_f64(self.played_seconds).unwrap_or_default()
    }

    /// What the counted load was recorded as, while it is on the deck.
    pub fn record(&self) -> Option<&Record> {
        match &self.count {
            Count::Counted(record) => record.as_ref(),
            Count::Counting => None,
        }
    }

    /// Hands back the counted load's record with how much it played, once.
    pub fn finish(&mut self) -> Option<Finished<Record>> {
        let Count::Counted(record) = &mut self.count else {
            return None;
        };
        Some(Finished {
            record: record.take()?,
            played: self.played(),
        })
    }
}

/// Where a play of this session sits in the history.
#[derive(Debug, Clone)]
pub struct SessionRecord {
    /// Its place among this session's plays.
    pub index: usize,
    /// The store's id for it, once written.
    pub stored: Arc<OnceLock<PlayId>>,
}

/// A page of the history: this session's plays, or an earlier day's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HistoryPage {
    /// The plays since Dubplate opened.
    ThisSession,
    /// The plays of earlier sessions on a day.
    Day(Date),
}

/// One play as the history table shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryRow {
    /// When it started, `21:04`.
    pub time: SharedString,
    /// The deck it played on, for its colour, when this layout has that deck.
    pub deck: Option<DeckId>,
    /// The deck's letter.
    pub deck_letter: SharedString,
    /// The title.
    pub title: SharedString,
    /// The artist.
    pub artist: SharedString,
}

/// A page with its plays, newest first.
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryPageView {
    /// Which page.
    pub page: HistoryPage,
    /// Its name in the sidebar, `This session` or `2026-11-21`.
    pub label: SharedString,
    /// Its plays, newest first.
    pub rows: Vec<HistoryRow>,
}

/// Every page of the history as the browser shows them: this session first, then each earlier
/// day, the latest first.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HistoryShelf {
    /// The pages.
    pub pages: Vec<HistoryPageView>,
}

impl HistoryShelf {
    /// The page `page`, when it has one.
    pub fn page(&self, page: HistoryPage) -> Option<&HistoryPageView> {
        self.pages.iter().find(|view| view.page == page)
    }
}

/// The plays of this session and of earlier ones.
#[derive(Debug, Clone)]
pub struct PlayHistory {
    session_started: SystemTime,
    session: Vec<Play>,
    earlier: Vec<Play>,
}

impl PlayHistory {
    /// An empty history for a session that started at `session_started`.
    pub fn new(session_started: SystemTime) -> Self {
        Self {
            session_started,
            session: Vec::new(),
            earlier: Vec::new(),
        }
    }

    /// Adds a play of this session and returns where it sits.
    pub fn add(&mut self, play: Play) -> SessionRecord {
        self.session.push(play);
        SessionRecord {
            index: self.session.len() - 1,
            stored: Arc::new(OnceLock::new()),
        }
    }

    /// Sets how long a play of this session has played.
    pub fn set_length(&mut self, record: &SessionRecord, played: Duration) {
        if let Some(play) = self.session.get_mut(record.index) {
            play.played = played;
        }
    }

    /// Takes the store's plays, newest first, keeping those of earlier sessions.
    pub fn restore_earlier(&mut self, plays: impl IntoIterator<Item = Play>) {
        self.earlier = plays
            .into_iter()
            .filter(|play| play.started_at < self.session_started)
            .collect();
    }

    /// This session's plays, oldest first.
    pub fn session(&self) -> impl DoubleEndedIterator<Item = &Play> {
        self.session.iter()
    }

    /// A page's plays, oldest first.
    pub fn plays(&self, page: HistoryPage, time_zone: &TimeZone) -> Vec<&Play> {
        match page {
            HistoryPage::ThisSession => self.session().collect(),
            HistoryPage::Day(day) => self
                .earlier
                .iter()
                .rev()
                .filter(|play| local_time(play.started_at, time_zone).date() == day)
                .collect(),
        }
    }

    /// The pages as the browser shows them.
    pub fn shelf(&self, time_zone: &TimeZone) -> HistoryShelf {
        let mut pages = vec![HistoryPageView {
            page: HistoryPage::ThisSession,
            label: "This session".into(),
            rows: self
                .session()
                .rev()
                .map(|play| history_row(play, time_zone))
                .collect(),
        }];
        for play in &self.earlier {
            let day = local_time(play.started_at, time_zone).date();
            let row = history_row(play, time_zone);
            match pages.last_mut() {
                Some(view) if view.page == HistoryPage::Day(day) => view.rows.push(row),
                _ => pages.push(HistoryPageView {
                    page: HistoryPage::Day(day),
                    label: day.to_string().into(),
                    rows: vec![row],
                }),
            }
        }
        HistoryShelf { pages }
    }
}

/// `time` on the clock of `time_zone`.
pub fn local_time(time: SystemTime, time_zone: &TimeZone) -> DateTime {
    let timestamp = Timestamp::try_from(time).unwrap_or(Timestamp::UNIX_EPOCH);
    time_zone.to_datetime(timestamp)
}

/// The deck of this layout a play happened on.
pub fn deck_of(play_deck: PlayDeck) -> Option<DeckId> {
    DeckId::ALL
        .into_iter()
        .find(|deck| deck_letter(*deck).starts_with(play_deck.letter()))
}

/// How the history keeps `deck`.
pub fn play_deck(deck: DeckId) -> Option<PlayDeck> {
    deck_letter(deck).chars().next().and_then(PlayDeck::new)
}

fn history_row(play: &Play, time_zone: &TimeZone) -> HistoryRow {
    HistoryRow {
        time: local_time(play.started_at, time_zone)
            .strftime("%H:%M")
            .to_string()
            .into(),
        deck: deck_of(play.deck),
        deck_letter: play.deck.letter().to_string().into(),
        title: play.title.clone().into(),
        artist: play.artist.clone().into(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::UNIX_EPOCH;

    use super::*;

    const LOAD: Option<u64> = Some(1);

    /// Samples a deck every half second from `start`, playing at `tempo`, for `seconds`.
    fn play_for(
        watch: &mut PlayWatch<u32>,
        start: Instant,
        tempo: f64,
        seconds: f64,
    ) -> Vec<PlayEvent<u32>> {
        let steps = (seconds * 2.0).round() as u32;
        (0..=steps)
            .filter_map(|step| {
                let offset = Duration::from_millis(u64::from(step) * 500);
                watch.observe(DeckSample {
                    load: LOAD,
                    playing: true,
                    tempo,
                    at: start + offset,
                    wall_clock: UNIX_EPOCH + Duration::from_secs(1_000) + offset,
                })
            })
            .collect()
    }

    #[test]
    fn fifty_nine_seconds_do_not_count_and_sixty_do() {
        let start = Instant::now();
        let mut short = PlayWatch::<u32>::default();
        assert!(play_for(&mut short, start, 1.0, 59.0).is_empty());

        let mut full = PlayWatch::<u32>::default();
        let events = play_for(&mut full, start, 1.0, 60.0);
        assert_eq!(
            events,
            [PlayEvent::Counted(Counted {
                started_at: UNIX_EPOCH + Duration::from_secs(1_000),
                played: PLAYED_AFTER,
            })]
        );
    }

    #[test]
    fn a_load_counts_once_however_long_it_plays() {
        let start = Instant::now();
        let mut watch = PlayWatch::<u32>::default();
        let events = play_for(&mut watch, start, 1.0, 300.0);
        assert_eq!(events.len(), 1);
        assert!(watch.finish().is_none(), "nothing recorded it yet");
    }

    #[test]
    fn track_time_follows_the_tempo_and_stops_while_paused() {
        let start = Instant::now();
        let mut watch = PlayWatch::<u32>::default();
        assert!(play_for(&mut watch, start, 1.5, 39.5).is_empty());
        assert_eq!(watch.played(), Duration::from_secs_f64(59.25));

        let paused = |seconds: u64| DeckSample {
            load: LOAD,
            playing: false,
            tempo: 1.5,
            at: start + Duration::from_secs(seconds),
            wall_clock: UNIX_EPOCH,
        };
        assert!(
            matches!(watch.observe(paused(40)), Some(PlayEvent::Counted(_))),
            "the last half second played at 1.5 makes the minute"
        );
        assert_eq!(watch.observe(paused(100)), None);
        assert_eq!(watch.observe(paused(400)), None);
        assert_eq!(watch.played(), PLAYED_AFTER);
    }

    #[test]
    fn a_new_load_finishes_the_counted_one_and_counts_again() {
        let start = Instant::now();
        let mut watch = PlayWatch::<u32>::default();
        play_for(&mut watch, start, 1.0, 90.0);
        watch.counted_as(7);
        assert_eq!(watch.record(), Some(&7));

        let next = watch.observe(DeckSample {
            load: Some(2),
            playing: false,
            tempo: 1.0,
            at: start + Duration::from_secs(91),
            wall_clock: UNIX_EPOCH,
        });
        assert_eq!(
            next,
            Some(PlayEvent::Finished(Finished {
                record: 7,
                played: Duration::from_secs(90),
            }))
        );
        assert_eq!(watch.played(), Duration::ZERO);
        assert_eq!(watch.record(), None);
    }

    #[test]
    fn an_empty_deck_never_counts() {
        let start = Instant::now();
        let mut watch = PlayWatch::<u32>::default();
        for second in 0..200 {
            let event = watch.observe(DeckSample {
                load: None,
                playing: true,
                tempo: 1.0,
                at: start + Duration::from_secs(second),
                wall_clock: UNIX_EPOCH,
            });
            assert_eq!(event, None);
        }
    }

    fn play(title: &str, deck: char, started_at: u64) -> Play {
        Play {
            set: None,
            track_path: PathBuf::from(format!("/Music/{title}.mp3")),
            rekordbox_id: None,
            title: title.to_owned(),
            artist: "Artist".to_owned(),
            started_at: UNIX_EPOCH + Duration::from_secs(started_at),
            played: PLAYED_AFTER,
            deck: PlayDeck::new(deck).unwrap(),
        }
    }

    #[test]
    fn the_shelf_lists_this_session_newest_first_then_earlier_days() {
        let day_one = 1_794_873_600;
        let day_two = day_one + 86_400;
        let session_started = UNIX_EPOCH + Duration::from_secs(day_two + 7_200);
        let mut history = PlayHistory::new(session_started);
        history.restore_earlier([
            play("Late", 'B', day_two + 600),
            play("Early", 'A', day_two),
            play("Before", 'A', day_one),
            play("Now", 'A', day_two + 7_300),
        ]);
        history.add(play("First", 'A', day_two + 7_300));
        history.add(play("Second", 'B', day_two + 7_600));

        let utc = TimeZone::UTC;
        let shelf = history.shelf(&utc);
        let labels: Vec<&str> = shelf.pages.iter().map(|page| page.label.as_ref()).collect();
        assert_eq!(labels, ["This session", "2026-11-18", "2026-11-17"]);
        let session_titles: Vec<&str> = shelf.pages[0]
            .rows
            .iter()
            .map(|row| row.title.as_ref())
            .collect();
        assert_eq!(session_titles, ["Second", "First"]);
        assert_eq!(shelf.pages[0].rows[0].deck, Some(DeckId::B));
        assert_eq!(shelf.pages[0].rows[1].time.as_ref(), "02:01");
        let day = shelf.pages[1].page;
        let plays: Vec<&str> = history
            .plays(day, &utc)
            .iter()
            .map(|play| play.title.as_str())
            .collect();
        assert_eq!(plays, ["Early", "Late"], "an export runs oldest first");
    }

    #[test]
    fn deck_letters_go_both_ways() {
        for deck in DeckId::ALL {
            assert_eq!(play_deck(deck).and_then(deck_of), Some(deck));
        }
    }
}
