//! The play history: every track a deck played long enough to count, when it started, how
//! long it played and on which deck, whether or not a set was being played.

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dubplate_rekordbox::TrackCues;
use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};
use rusqlite::{Connection, Transaction};

use super::{SetId, SetStore, StoreError, cues::write_track_cues};
use crate::TrackIdentity;
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

/// A CDJ history. The player stores the order and track ids, without play times.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportedHistory {
    /// The exporting library id.
    pub library: u32,
    /// The history id within that library.
    pub history: u32,
    /// The stick and history names shown in Dubplate.
    pub label: String,
    /// Artist and title in play order.
    pub tracks: Vec<(String, String)>,
}

impl SetStore {
    /// Saves the selected CDJ histories and track cues as one change.
    pub(crate) fn import_usb_selection(
        &mut self,
        histories: &[ImportedHistory],
        tracks: &[(TrackIdentity, Option<PathBuf>, TrackCues)],
    ) -> Result<Vec<(i64, ImportedHistory)>, StoreError> {
        let transaction = self.transaction()?;
        for history in histories {
            write_usb_history(&transaction, history)?;
        }
        for (identity, path, cues) in tracks {
            write_track_cues(&transaction, identity, path.as_deref(), cues)?;
        }
        let histories = read_usb_histories(&transaction)?;
        transaction.commit()?;
        Ok(histories)
    }

    /// Imported histories, newest first.
    pub fn usb_histories(&self) -> Result<Vec<(i64, ImportedHistory)>, StoreError> {
        read_usb_histories(&self.connection)
    }

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

/// Imports or refreshes a history, keeping its database id on repeat imports.
fn write_usb_history(
    transaction: &Transaction<'_>,
    history: &ImportedHistory,
) -> Result<(), StoreError> {
    let tracks = serde_json::to_string(&history.tracks)?;
    execute_cached(
        transaction,
        "INSERT INTO usb_history (library, history, label, tracks) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(library, history) DO UPDATE SET label = excluded.label, tracks = excluded.tracks",
        (history.library, history.history, &history.label, tracks),
    )?;
    Ok(())
}

fn read_usb_histories(connection: &Connection) -> Result<Vec<(i64, ImportedHistory)>, StoreError> {
    let mut statement = connection.prepare_cached(
        "SELECT id, library, history, label, tracks FROM usb_history ORDER BY id DESC",
    )?;
    let mut rows = statement.query([])?;
    let mut result = Vec::new();
    while let Some(row) = rows.next()? {
        result.push((
            row.get(0)?,
            ImportedHistory {
                library: row.get(1)?,
                history: row.get(2)?,
                label: row.get(3)?,
                tracks: serde_json::from_str(&row.get::<_, String>(4)?)?,
            },
        ));
    }
    Ok(result)
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
    use dubplate_rekordbox::{HotCuePad, MemoryCue};

    use super::*;
    use crate::store::MIGRATIONS;

    #[test]
    fn version_seven_upgrades_and_retains_existing_history() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("dubplate.db");
        let connection = Connection::open(&path).unwrap();
        for migration in &MIGRATIONS[..7] {
            connection.execute_batch(migration).unwrap();
        }
        connection
            .execute_batch(
                "INSERT INTO sets (id, name, target_minutes, created_at, updated_at)
                 VALUES (1, 'Friday', 90, 0, 0);
                 INSERT INTO play_history (id, set_id, track_path, title, started_at, deck)
                 VALUES (1, 1, '/sets/local.flac', 'Local track', 1000, 'A');",
            )
            .unwrap();
        drop(connection);

        let store = SetStore::open(&path).unwrap();
        assert_eq!(store.load_set(SetId(1)).unwrap().name, "Friday");
        assert!(store.usb_histories().unwrap().is_empty());
        let plays = store.play_history().unwrap();
        assert_eq!(plays.len(), 1);
        assert_eq!(plays[0].play.title, "Local track");
        assert_eq!(plays[0].play.set, Some(SetId(1)));
        assert!(
            directory
                .path()
                .join("backups")
                .read_dir()
                .unwrap()
                .filter_map(Result::ok)
                .any(|backup| backup.file_name().to_string_lossy().starts_with("v7-"))
        );
    }

    #[test]
    fn importing_a_history_again_updates_only_that_librarys_history() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("dubplate.db");
        let mut store = SetStore::open(&path).unwrap();
        let mut first = ImportedHistory {
            library: u32::MAX,
            history: 1,
            label: "First".into(),
            tracks: vec![("Band".into(), "Song".into())],
        };
        let first_id = store
            .import_usb_selection(std::slice::from_ref(&first), &[])
            .unwrap()[0]
            .0;
        let second = ImportedHistory {
            library: 42,
            ..first.clone()
        };
        let second_id = store
            .import_usb_selection(std::slice::from_ref(&second), &[])
            .unwrap()[0]
            .0;
        assert_ne!(first_id, second_id);
        first.label = "Updated".into();
        first.tracks = vec![
            ("Band".into(), "Song".into()),
            ("DJ".into(), "Next".into()),
            ("Band".into(), "Song".into()),
        ];
        for _ in 0..2 {
            store
                .import_usb_selection(std::slice::from_ref(&first), &[])
                .unwrap();
        }
        drop(store);
        let reopened = SetStore::open(&path).unwrap();
        assert_eq!(
            reopened.usb_histories().unwrap(),
            vec![(second_id, second), (first_id, first)]
        );
        assert!(reopened.play_history().unwrap().is_empty());
    }

    #[test]
    fn a_failed_cue_write_rolls_back_the_entire_usb_selection() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("dubplate.db");
        let mut store = SetStore::open(&path).unwrap();
        let history = ImportedHistory {
            library: 42,
            history: 1,
            label: "First".into(),
            tracks: vec![("Band".into(), "Song".into())],
        };
        let existing = store
            .import_usb_selection(std::slice::from_ref(&history), &[])
            .unwrap();
        store
            .connection
            .execute_batch(
                "CREATE TRIGGER reject_second_track BEFORE INSERT ON track_cues
                 WHEN NEW.track_path = '/sets/second.flac'
                 BEGIN
                     SELECT RAISE(ABORT, 'cue write failed');
                 END;",
            )
            .unwrap();
        let changed = ImportedHistory {
            label: "Updated".into(),
            ..history
        };
        let cues = TrackCues::new(
            [const { None }; HotCuePad::ALL.len()],
            vec![MemoryCue {
                start_ms: 1000,
                loop_end_ms: None,
                color: None,
                name: "Drop".into(),
            }],
        );
        let first = PathBuf::from("/sets/first.flac");
        let second = PathBuf::from("/sets/second.flac");
        let changes = [
            (
                TrackIdentity::File(first.clone()),
                Some(first),
                cues.clone(),
            ),
            (TrackIdentity::File(second.clone()), Some(second), cues),
        ];
        assert!(
            store
                .import_usb_selection(std::slice::from_ref(&changed), &changes)
                .is_err()
        );
        assert_eq!(store.usb_histories().unwrap(), existing);
        assert!(store.track_cues().unwrap().is_empty());

        store
            .connection
            .execute_batch("DROP TRIGGER reject_second_track")
            .unwrap();
        let histories = store
            .import_usb_selection(std::slice::from_ref(&changed), &changes)
            .unwrap();
        assert_eq!(histories, vec![(existing[0].0, changed)]);
        assert_eq!(store.track_cues().unwrap().len(), 2);
    }

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
