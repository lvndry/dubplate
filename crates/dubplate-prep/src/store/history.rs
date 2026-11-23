//! The play history: every track a deck played long enough to count, when it started, how
//! long it played and on which deck, whether or not a set was being played.

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};

use super::{SetId, SetStore, StoreError};
use crate::sql::execute_cached;

/// Dubplate's identifier for a recorded play.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlayId(i64);

impl std::fmt::Display for PlayId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

/// The deck a track played on, by the letter the decks carry, A to D as on rekordbox's
/// four-deck layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlayDeck(char);

impl PlayDeck {
    /// The deck with `letter`, or `None` for a letter no deck carries.
    pub fn new(letter: char) -> Option<Self> {
        let letter = letter.to_ascii_uppercase();
        ('A'..='D').contains(&letter).then_some(Self(letter))
    }

    /// The deck's letter.
    pub fn letter(self) -> char {
        self.0
    }
}

impl ToSql for PlayDeck {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.0.to_string()))
    }
}

impl FromSql for PlayDeck {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let text = value.as_str()?;
        let mut letters = text.chars();
        match (letters.next().and_then(Self::new), letters.next()) {
            (Some(deck), None) => Ok(deck),
            _ => Err(FromSqlError::Other(
                format!("{text:?} is not a deck letter").into(),
            )),
        }
    }
}

/// One track played on a deck.
#[derive(Debug, Clone, PartialEq)]
pub struct Play {
    /// The set the track belongs to, when it was a set track.
    pub set: Option<SetId>,
    /// The audio file.
    pub track_path: PathBuf,
    /// rekordbox's id for the track, when it comes from rekordbox.
    pub rekordbox_id: Option<String>,
    /// The title as the library showed it, kept so a tracklist survives the file moving.
    pub title: String,
    /// The artist as the library showed it.
    pub artist: String,
    /// When the deck first played this load.
    pub started_at: SystemTime,
    /// How much of the track played, in track time, so far. Zero for plays recorded before
    /// lengths were kept.
    pub played: Duration,
    /// The deck it played on.
    pub deck: PlayDeck,
}

/// A play as the store keeps it.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordedPlay {
    /// Its identifier, to update its length.
    pub id: PlayId,
    /// The play.
    pub play: Play,
}

impl SetStore {
    /// Records a play and returns its identifier, for its length to be updated as the track
    /// plays on.
    pub fn record_play(&mut self, play: &Play) -> Result<PlayId, StoreError> {
        let transaction = self.transaction()?;
        execute_cached(
            &transaction,
            "INSERT INTO play_history
                (set_id, track_path, rekordbox_id, title, artist, started_at, seconds_played, deck)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            (
                play.set.map(SetId::as_i64),
                play.track_path.to_string_lossy(),
                &play.rekordbox_id,
                &play.title,
                &play.artist,
                unix_millis(play.started_at),
                play.played.as_secs_f64(),
                play.deck,
            ),
        )?;
        let id = PlayId(transaction.last_insert_rowid());
        transaction.commit()?;
        Ok(id)
    }

    /// Sets how long a recorded play has played in all.
    pub fn set_play_length(&mut self, id: PlayId, played: Duration) -> Result<(), StoreError> {
        let transaction = self.transaction()?;
        let updated = execute_cached(
            &transaction,
            "UPDATE play_history SET seconds_played = ?2 WHERE id = ?1",
            (id.0, played.as_secs_f64()),
        )?;
        if updated == 0 {
            return Err(StoreError::PlayNotFound(id));
        }
        transaction.commit()?;
        Ok(())
    }

    /// Every recorded play, newest first.
    pub fn play_history(&self) -> Result<Vec<RecordedPlay>, StoreError> {
        let mut statement = self.connection.prepare_cached(
            "SELECT id, set_id, track_path, rekordbox_id, title, artist, started_at,
                seconds_played, deck
             FROM play_history
             ORDER BY started_at DESC, id DESC",
        )?;
        let plays = statement
            .query_map([], |row| {
                Ok(RecordedPlay {
                    id: PlayId(row.get(0)?),
                    play: Play {
                        set: row.get::<_, Option<i64>>(1)?.map(SetId),
                        track_path: PathBuf::from(row.get::<_, String>(2)?),
                        rekordbox_id: row.get(3)?,
                        title: row.get(4)?,
                        artist: row.get(5)?,
                        started_at: from_unix_millis(row.get(6)?),
                        played: Duration::try_from_secs_f64(row.get(7)?).unwrap_or_default(),
                        deck: row.get(8)?,
                    },
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(plays)
    }
}

/// `time` as Unix milliseconds, 0 before 1970.
fn unix_millis(time: SystemTime) -> i64 {
    i64::try_from(dubplate_library::unix_millis(time)).unwrap_or(i64::MAX)
}

/// Unix milliseconds as a time, 1970 for a negative count.
fn from_unix_millis(millis: i64) -> SystemTime {
    UNIX_EPOCH + Duration::from_millis(u64::try_from(millis).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deck_letters_are_a_to_d_whatever_the_case() {
        assert_eq!(PlayDeck::new('b').map(PlayDeck::letter), Some('B'));
        assert_eq!(PlayDeck::new('E'), None);
        assert_eq!(PlayDeck::new('1'), None);
    }

    #[test]
    fn times_round_trip_through_unix_milliseconds() {
        let time = UNIX_EPOCH + Duration::from_millis(1_795_000_000_123);
        assert_eq!(from_unix_millis(unix_millis(time)), time);
        assert_eq!(from_unix_millis(-5), UNIX_EPOCH);
    }
}
