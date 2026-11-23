//! The metadata the DJ edits on a track, as rekordbox has it: the rating from 0 to 5 stars,
//! the colour label, the comment, and the My Tag system of categories of tags, kept in
//! Dubplate's database and never in rekordbox's. A track with nothing stored for it has none,
//! and the caller seeds it from what rekordbox has for it.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::{OptionalExtension, Transaction, params_from_iter};

use super::{SetStore, StoreError, now_unix_millis};
use crate::identity::TrackIdentity;
use crate::sql::{execute_cached, query_row_cached};

/// How many tracks one metadata read asks about, two variables each, well under SQLite's
/// limit on variables in a statement.
const METADATA_READ_CHUNK: usize = 4_000;

/// Dubplate's identifier for one of the DJ's My Tag categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TagCategoryId(pub i64);

impl std::fmt::Display for TagCategoryId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

/// Dubplate's identifier for one of the DJ's My Tags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TagId(pub i64);

impl std::fmt::Display for TagId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

/// The colour label a track carries: one of rekordbox's eight track colours.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrackColour {
    /// Pink.
    Pink,
    /// Red.
    Red,
    /// Orange.
    Orange,
    /// Yellow.
    Yellow,
    /// Green.
    Green,
    /// Aqua.
    Aqua,
    /// Blue.
    Blue,
    /// Purple.
    Purple,
}

impl TrackColour {
    fn storage_name(self) -> &'static str {
        match self {
            Self::Pink => "Pink",
            Self::Red => "Red",
            Self::Orange => "Orange",
            Self::Yellow => "Yellow",
            Self::Green => "Green",
            Self::Aqua => "Aqua",
            Self::Blue => "Blue",
            Self::Purple => "Purple",
        }
    }

    fn from_storage_name(name: &str) -> Option<Self> {
        match name {
            "Pink" => Some(Self::Pink),
            "Red" => Some(Self::Red),
            "Orange" => Some(Self::Orange),
            "Yellow" => Some(Self::Yellow),
            "Green" => Some(Self::Green),
            "Aqua" => Some(Self::Aqua),
            "Blue" => Some(Self::Blue),
            "Purple" => Some(Self::Purple),
            _ => None,
        }
    }
}

/// One of the DJ's My Tags.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Tag {
    /// Its id.
    pub id: TagId,
    /// The name the DJ gave it.
    pub name: String,
}

/// One of the DJ's My Tag categories and the tags it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagCategory {
    /// Its id.
    pub id: TagCategoryId,
    /// The name the DJ gave it.
    pub name: String,
    /// Its tags in the DJ's order.
    pub tags: Vec<Tag>,
}

/// What Dubplate has stored for a track: the DJ's rating, colour label, comment and tags.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TrackMetadata {
    /// Its audio file, when it is known.
    pub track_path: Option<PathBuf>,
    /// The rating from 0 to 5 stars; 0 is unrated.
    pub rating: u8,
    /// The colour label; `None` when the track has none.
    pub colour: Option<TrackColour>,
    /// The comment.
    pub comment: String,
    /// Its tags, in the order of the categories and the tags inside them.
    pub tags: Vec<Tag>,
}

impl TrackMetadata {
    /// The metadata to use for a track from what rekordbox has for it, when Dubplate has none
    /// stored yet: its rating and comment, without a colour label or tags.
    pub fn from_rekordbox(track: &dubplate_rekordbox::Track) -> Self {
        Self {
            track_path: track.file_path.clone(),
            rating: track.rating.unwrap_or(0),
            colour: None,
            comment: track.comment.clone().unwrap_or_default(),
            tags: Vec::new(),
        }
    }
}

/// A change to a track's metadata or to the My Tag system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrackTagEdit {
    /// Give a track a rating, from 0 to 5 stars, 0 being unrated.
    SetRating {
        /// The track.
        track: TrackIdentity,
        /// Its audio file, when it is known.
        track_path: Option<PathBuf>,
        /// The rating from 0 to 5 stars.
        rating: u8,
    },
    /// Give a track a colour label, or take its label away with `None`.
    SetColour {
        /// The track.
        track: TrackIdentity,
        /// Its audio file, when it is known.
        track_path: Option<PathBuf>,
        /// The label, or none.
        colour: Option<TrackColour>,
    },
    /// Give a track a comment.
    SetComment {
        /// The track.
        track: TrackIdentity,
        /// Its audio file, when it is known.
        track_path: Option<PathBuf>,
        /// The comment.
        comment: String,
    },
    /// Put a tag on tracks. A track it holds already keeps it once.
    AddTag {
        /// The tracks.
        tracks: Vec<TrackIdentity>,
        /// The tag.
        tag: TagId,
    },
    /// Take a tag off tracks.
    RemoveTag {
        /// The tracks.
        tracks: Vec<TrackIdentity>,
        /// The tag.
        tag: TagId,
    },
    /// Make a category at `position` among the categories.
    CreateCategory {
        /// Its name.
        name: String,
        /// Its place from 0 among the categories; past the end puts it last.
        position: usize,
    },
    /// Give a category a new name.
    RenameCategory {
        /// The category.
        category: TagCategoryId,
        /// Its new name.
        name: String,
    },
    /// Delete a category, the tags it holds, and those tags off every track.
    DeleteCategory(TagCategoryId),
    /// Move a category to `position` among the categories.
    MoveCategory {
        /// The category.
        category: TagCategoryId,
        /// Its place from 0 among the categories; past the end puts it last.
        position: usize,
    },
    /// Make a tag in the category at `position` among its tags.
    CreateTag {
        /// The category.
        category: TagCategoryId,
        /// Its name.
        name: String,
        /// Its place from 0 among the category's tags; past the end puts it last.
        position: usize,
    },
    /// Give a tag a new name.
    RenameTag {
        /// The tag.
        tag: TagId,
        /// Its new name.
        name: String,
    },
    /// Delete a tag and take it off every track.
    DeleteTag(TagId),
    /// Move a tag to `position` among its category's tags.
    MoveTag {
        /// The tag.
        tag: TagId,
        /// Its place from 0 among the category's tags; past the end puts it last.
        position: usize,
    },
}

/// What an edit to a track's metadata or to the tags did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TrackTagEdited {
    /// The category it made, for [`TrackTagEdit::CreateCategory`].
    pub created_category: Option<TagCategoryId>,
    /// The tag it made, for [`TrackTagEdit::CreateTag`].
    pub created_tag: Option<TagId>,
    /// How many tracks it put a tag on, for [`TrackTagEdit::AddTag`].
    pub added: usize,
}

/// A track's stored rows as they are read, before they become its `TrackMetadata`.
#[derive(Default)]
struct ReadMetadata {
    has_row: bool,
    track_path: Option<PathBuf>,
    rating: u8,
    colour: Option<TrackColour>,
    comment: String,
    tags: Vec<Tag>,
}

impl SetStore {
    /// What Dubplate has stored for the track, or `None` when the DJ has made nothing for it
    /// yet.
    pub fn track_metadata(
        &self,
        track: &TrackIdentity,
    ) -> Result<Option<TrackMetadata>, StoreError> {
        let metadata = self.track_metadata_many(std::slice::from_ref(track))?;
        Ok(metadata.into_values().next())
    }

    /// What Dubplate has stored for each of the tracks, in one query, for the library table:
    /// a track with nothing stored for it is absent.
    pub fn track_metadata_many(
        &self,
        tracks: &[TrackIdentity],
    ) -> Result<HashMap<TrackIdentity, TrackMetadata>, StoreError> {
        let mut asked: HashSet<&TrackIdentity> = HashSet::new();
        let tracks: Vec<&TrackIdentity> =
            tracks.iter().filter(|track| asked.insert(track)).collect();
        if tracks.is_empty() {
            return Ok(HashMap::new());
        }
        let mut found = HashMap::with_capacity(tracks.len());
        for chunk in tracks.chunks(METADATA_READ_CHUNK) {
            found.extend(self.track_metadata_chunk(chunk)?);
        }
        Ok(found)
    }

    fn track_metadata_chunk(
        &self,
        tracks: &[&TrackIdentity],
    ) -> Result<HashMap<TrackIdentity, TrackMetadata>, StoreError> {
        // The query is built for the tracks it is asked about, so it is prepared, not cached.
        let placeholders = std::iter::repeat_n("(?, ?)", tracks.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT COALESCE(meta.identity_kind, tagged.identity_kind),
                COALESCE(meta.identity, tagged.identity),
                meta.track_path, meta.rating, meta.colour, meta.comment, tag.id, tag.name
             FROM track_metadata AS meta
             FULL OUTER JOIN (track_tag AS tagged
                JOIN tag AS tag ON tag.id = tagged.tag_id
                JOIN tag_category AS category ON category.id = tag.category_id)
                ON tagged.identity_kind = meta.identity_kind AND tagged.identity = meta.identity
             WHERE (COALESCE(meta.identity_kind, tagged.identity_kind),
                    COALESCE(meta.identity, tagged.identity)) IN ({placeholders})
             ORDER BY 1, 2, category.position, tag.position, tag.id"
        );
        let mut params: Vec<String> = Vec::with_capacity(tracks.len() * 2);
        for track in tracks {
            let (kind, key) = track.storage_key();
            params.push(kind.to_owned());
            params.push(key.into_owned());
        }
        let mut statement = self.connection.prepare(&sql)?;
        let mut rows = statement.query(params_from_iter(params))?;
        let mut by_identity: HashMap<(String, String), ReadMetadata> = HashMap::new();
        while let Some(row) = rows.next()? {
            let stored_rating: Option<i64> = row.get(3)?;
            let read = by_identity.entry((row.get(0)?, row.get(1)?)).or_default();
            if let Some(rating) = stored_rating {
                read.has_row = true;
                read.track_path = row.get::<_, Option<String>>(2)?.map(PathBuf::from);
                read.rating = u8::try_from(rating).unwrap_or(0);
                read.colour = row
                    .get::<_, Option<String>>(4)?
                    .and_then(|name| TrackColour::from_storage_name(&name));
                read.comment = row.get(5)?;
            }
            let tag_id: Option<i64> = row.get(6)?;
            if let Some(stored_id) = tag_id {
                read.tags.push(Tag {
                    id: TagId(stored_id),
                    name: row.get(7)?,
                });
            }
        }
        Ok(by_identity
            .into_iter()
            .filter(|(_, read)| read.has_row || !read.tags.is_empty())
            .filter_map(|((kind, identity), read)| {
                Some((
                    TrackIdentity::from_storage_key(&kind, identity)?,
                    TrackMetadata {
                        track_path: read.track_path,
                        rating: read.rating,
                        colour: read.colour,
                        comment: read.comment,
                        tags: read.tags,
                    },
                ))
            })
            .collect())
    }

    /// The My Tag categories and their tags in the DJ's order.
    pub fn tag_categories(&self) -> Result<Vec<TagCategory>, StoreError> {
        let categories: Vec<(i64, String)> = self
            .connection
            .prepare_cached("SELECT id, name FROM tag_category ORDER BY position, id")?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        let tags: Vec<(i64, i64, String)> = self
            .connection
            .prepare_cached(
                "SELECT id, category_id, name FROM tag ORDER BY category_id, position, id",
            )?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        let mut by_category: HashMap<i64, Vec<Tag>> = HashMap::new();
        for (id, category_id, name) in tags {
            by_category.entry(category_id).or_default().push(Tag {
                id: TagId(id),
                name,
            });
        }
        Ok(categories
            .into_iter()
            .map(|(id, name)| TagCategory {
                id: TagCategoryId(id),
                name,
                tags: by_category.remove(&id).unwrap_or_default(),
            })
            .collect())
    }

    /// The tracks that carry every one of `tags`, in their identity order; none when the list
    /// is empty.
    pub fn tracks_with_all_tags(&self, tags: &[TagId]) -> Result<Vec<TrackIdentity>, StoreError> {
        if tags.is_empty() {
            return Ok(Vec::new());
        }
        let params: Vec<i64> = tags
            .iter()
            .map(|tag| tag.0)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let placeholders = std::iter::repeat_n("?", params.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT identity_kind, identity FROM track_tag
             WHERE tag_id IN ({placeholders})
             GROUP BY identity_kind, identity
             HAVING COUNT(tag_id) = {count}
             ORDER BY identity_kind, identity",
            count = params.len()
        );
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement
            .query_map(params_from_iter(params), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows
            .into_iter()
            .filter_map(|(kind, identity)| TrackIdentity::from_storage_key(&kind, identity))
            .collect())
    }

    /// Carries out `edit` in one transaction: all of it, or nothing when it fails.
    pub fn edit_track_tags(&mut self, edit: TrackTagEdit) -> Result<TrackTagEdited, StoreError> {
        let transaction = self.transaction()?;
        let edited = apply(&transaction, edit)?;
        transaction.commit()?;
        Ok(edited)
    }
}

fn apply(transaction: &Transaction<'_>, edit: TrackTagEdit) -> Result<TrackTagEdited, StoreError> {
    match edit {
        TrackTagEdit::SetRating {
            track,
            track_path,
            rating,
        } => {
            if rating > 5 {
                return Err(StoreError::RatingOutOfRange(rating));
            }
            let (kind, key) = track.storage_key();
            execute_cached(
                transaction,
                "INSERT INTO track_metadata (identity_kind, identity, track_path, rating, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (identity_kind, identity) DO UPDATE SET
                     track_path = COALESCE(excluded.track_path, track_path),
                     rating = excluded.rating,
                     updated_at = excluded.updated_at",
                (
                    kind,
                    &key,
                    track_path.as_deref().map(Path::to_string_lossy),
                    rating as i64,
                    now_unix_millis(),
                ),
            )?;
            Ok(TrackTagEdited::default())
        }
        TrackTagEdit::SetColour {
            track,
            track_path,
            colour,
        } => {
            let (kind, key) = track.storage_key();
            execute_cached(
                transaction,
                "INSERT INTO track_metadata (identity_kind, identity, track_path, colour, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (identity_kind, identity) DO UPDATE SET
                     track_path = COALESCE(excluded.track_path, track_path),
                     colour = excluded.colour,
                     updated_at = excluded.updated_at",
                (
                    kind,
                    &key,
                    track_path.as_deref().map(Path::to_string_lossy),
                    colour.map(TrackColour::storage_name),
                    now_unix_millis(),
                ),
            )?;
            Ok(TrackTagEdited::default())
        }
        TrackTagEdit::SetComment {
            track,
            track_path,
            comment,
        } => {
            let (kind, key) = track.storage_key();
            execute_cached(
                transaction,
                "INSERT INTO track_metadata (identity_kind, identity, track_path, comment, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (identity_kind, identity) DO UPDATE SET
                     track_path = COALESCE(excluded.track_path, track_path),
                     comment = excluded.comment,
                     updated_at = excluded.updated_at",
                (
                    kind,
                    &key,
                    track_path.as_deref().map(Path::to_string_lossy),
                    &comment,
                    now_unix_millis(),
                ),
            )?;
            Ok(TrackTagEdited::default())
        }
        TrackTagEdit::AddTag { tracks, tag } => add_tag(transaction, &tracks, tag),
        TrackTagEdit::RemoveTag { tracks, tag } => {
            let mut seen: HashSet<&TrackIdentity> = HashSet::new();
            for track in tracks.iter().filter(|track| seen.insert(track)) {
                let (kind, key) = track.storage_key();
                execute_cached(
                    transaction,
                    "DELETE FROM track_tag WHERE identity_kind = ?1 AND identity = ?2 AND tag_id = ?3",
                    (kind, &key, tag.0),
                )?;
            }
            Ok(TrackTagEdited::default())
        }
        TrackTagEdit::CreateCategory { name, position } => {
            create_category(transaction, name, position)
        }
        TrackTagEdit::RenameCategory { category, name } => {
            let changed = execute_cached(
                transaction,
                "UPDATE tag_category SET name = ?2 WHERE id = ?1",
                (category.0, &name),
            )?;
            if changed == 0 {
                return Err(StoreError::TagCategoryNotFound(category));
            }
            Ok(TrackTagEdited::default())
        }
        TrackTagEdit::DeleteCategory(category) => {
            let deleted = execute_cached(
                transaction,
                "DELETE FROM tag_category WHERE id = ?1",
                (category.0,),
            )?;
            if deleted == 0 {
                return Err(StoreError::TagCategoryNotFound(category));
            }
            Ok(TrackTagEdited::default())
        }
        TrackTagEdit::MoveCategory { category, position } => {
            move_category(transaction, category, position)?;
            Ok(TrackTagEdited::default())
        }
        TrackTagEdit::CreateTag {
            category,
            name,
            position,
        } => create_tag(transaction, category, name, position),
        TrackTagEdit::RenameTag { tag, name } => {
            let changed = execute_cached(
                transaction,
                "UPDATE tag SET name = ?2 WHERE id = ?1",
                (tag.0, &name),
            )?;
            if changed == 0 {
                return Err(StoreError::TagNotFound(tag));
            }
            Ok(TrackTagEdited::default())
        }
        TrackTagEdit::DeleteTag(tag) => {
            let deleted = execute_cached(transaction, "DELETE FROM tag WHERE id = ?1", (tag.0,))?;
            if deleted == 0 {
                return Err(StoreError::TagNotFound(tag));
            }
            Ok(TrackTagEdited::default())
        }
        TrackTagEdit::MoveTag { tag, position } => {
            move_tag(transaction, tag, position)?;
            Ok(TrackTagEdited::default())
        }
    }
}

/// Puts `tag` on each of `tracks`, counting the tracks it put it on.
fn add_tag(
    transaction: &Transaction<'_>,
    tracks: &[TrackIdentity],
    tag: TagId,
) -> Result<TrackTagEdited, StoreError> {
    let existing: i64 = query_row_cached(
        transaction,
        "SELECT COUNT(*) FROM tag WHERE id = ?1",
        (tag.0,),
        |row| row.get(0),
    )?;
    if existing == 0 {
        return Err(StoreError::TagNotFound(tag));
    }
    let mut seen: HashSet<&TrackIdentity> = HashSet::new();
    let mut added = 0;
    for track in tracks.iter().filter(|track| seen.insert(track)) {
        let (kind, key) = track.storage_key();
        added += execute_cached(
            transaction,
            "INSERT OR IGNORE INTO track_tag (identity_kind, identity, tag_id) VALUES (?1, ?2, ?3)",
            (kind, &key, tag.0),
        )?;
    }
    Ok(TrackTagEdited {
        added,
        ..TrackTagEdited::default()
    })
}

fn create_category(
    transaction: &Transaction<'_>,
    name: String,
    position: usize,
) -> Result<TrackTagEdited, StoreError> {
    transaction.execute(
        "INSERT INTO tag_category (position, name) VALUES (?1, ?2)",
        (i64::MAX, &name),
    )?;
    let created = TagCategoryId(transaction.last_insert_rowid());
    let mut order = category_order(transaction)?;
    order.retain(|id| *id != created);
    order.insert(position.min(order.len()), created);
    write_category_order(transaction, &order)?;
    Ok(TrackTagEdited {
        created_category: Some(created),
        ..TrackTagEdited::default()
    })
}

fn create_tag(
    transaction: &Transaction<'_>,
    category: TagCategoryId,
    name: String,
    position: usize,
) -> Result<TrackTagEdited, StoreError> {
    let existing: i64 = query_row_cached(
        transaction,
        "SELECT COUNT(*) FROM tag_category WHERE id = ?1",
        (category.0,),
        |row| row.get(0),
    )?;
    if existing == 0 {
        return Err(StoreError::TagCategoryNotFound(category));
    }
    transaction.execute(
        "INSERT INTO tag (category_id, position, name) VALUES (?1, ?2, ?3)",
        (category.0, i64::MAX, &name),
    )?;
    let created = TagId(transaction.last_insert_rowid());
    let mut order = tag_siblings(transaction, category)?;
    order.retain(|id| *id != created);
    order.insert(position.min(order.len()), created);
    write_tag_order(transaction, &order)?;
    Ok(TrackTagEdited {
        created_tag: Some(created),
        ..TrackTagEdited::default()
    })
}

fn move_category(
    transaction: &Transaction<'_>,
    category: TagCategoryId,
    position: usize,
) -> Result<(), StoreError> {
    let mut order = category_order(transaction)?;
    if !order.contains(&category) {
        return Err(StoreError::TagCategoryNotFound(category));
    }
    order.retain(|id| *id != category);
    order.insert(position.min(order.len()), category);
    write_category_order(transaction, &order)
}

fn move_tag(transaction: &Transaction<'_>, tag: TagId, position: usize) -> Result<(), StoreError> {
    let category_id: Option<i64> = query_row_cached(
        transaction,
        "SELECT category_id FROM tag WHERE id = ?1",
        (tag.0,),
        |row| row.get(0),
    )
    .optional()?;
    let Some(category_id) = category_id else {
        return Err(StoreError::TagNotFound(tag));
    };
    let mut order = tag_siblings(transaction, TagCategoryId(category_id))?;
    order.retain(|id| *id != tag);
    order.insert(position.min(order.len()), tag);
    write_tag_order(transaction, &order)
}

/// The categories in the DJ's order.
fn category_order(transaction: &Transaction<'_>) -> Result<Vec<TagCategoryId>, StoreError> {
    let mut statement =
        transaction.prepare_cached("SELECT id FROM tag_category ORDER BY position, id")?;
    let ids = statement
        .query_map([], |row| row.get(0))?
        .collect::<Result<Vec<i64>, _>>()?;
    Ok(ids.into_iter().map(TagCategoryId).collect())
}

/// Numbers the categories in the order given.
fn write_category_order(
    transaction: &Transaction<'_>,
    order: &[TagCategoryId],
) -> Result<(), StoreError> {
    let mut update =
        transaction.prepare_cached("UPDATE tag_category SET position = ?2 WHERE id = ?1")?;
    for (position, id) in order.iter().enumerate() {
        update.execute((id.0, position as i64))?;
    }
    Ok(())
}

/// The tags of the category `category` in the DJ's order.
fn tag_siblings(
    transaction: &Transaction<'_>,
    category: TagCategoryId,
) -> Result<Vec<TagId>, StoreError> {
    let mut statement = transaction
        .prepare_cached("SELECT id FROM tag WHERE category_id = ?1 ORDER BY position, id")?;
    let ids = statement
        .query_map((category.0,), |row| row.get(0))?
        .collect::<Result<Vec<i64>, _>>()?;
    Ok(ids.into_iter().map(TagId).collect())
}

/// Numbers the tags of one category in the order given.
fn write_tag_order(transaction: &Transaction<'_>, order: &[TagId]) -> Result<(), StoreError> {
    let mut update = transaction.prepare_cached("UPDATE tag SET position = ?2 WHERE id = ?1")?;
    for (position, id) in order.iter().enumerate() {
        update.execute((id.0, position as i64))?;
    }
    Ok(())
}

/// Points the metadata and tags kept for the file `from` at the file `to`, like the cues and
/// playlist places do.
pub(super) fn move_file_rows(
    transaction: &Transaction<'_>,
    from: &Path,
    to: &Path,
) -> Result<(), StoreError> {
    let moved = TrackIdentity::File(from.to_path_buf());
    let (kind, from_key) = moved.storage_key();
    let to_key = to.to_string_lossy();
    let has_metadata: i64 = query_row_cached(
        transaction,
        "SELECT COUNT(*) FROM track_metadata WHERE identity_kind = ?1 AND identity = ?2",
        (kind, &from_key),
        |row| row.get(0),
    )?;
    if has_metadata > 0 {
        execute_cached(
            transaction,
            "DELETE FROM track_metadata WHERE identity_kind = ?1 AND identity = ?2",
            (kind, &to_key),
        )?;
        execute_cached(
            transaction,
            "UPDATE track_metadata SET identity = ?3 WHERE identity_kind = ?1 AND identity = ?2",
            (kind, &from_key, &to_key),
        )?;
    }
    let has_tags: i64 = query_row_cached(
        transaction,
        "SELECT COUNT(*) FROM track_tag WHERE identity_kind = ?1 AND identity = ?2",
        (kind, &from_key),
        |row| row.get(0),
    )?;
    if has_tags > 0 {
        execute_cached(
            transaction,
            "DELETE FROM track_tag WHERE identity_kind = ?1 AND identity = ?2",
            (kind, &to_key),
        )?;
        execute_cached(
            transaction,
            "UPDATE track_tag SET identity = ?3 WHERE identity_kind = ?1 AND identity = ?2",
            (kind, &from_key, &to_key),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::super::{MIGRATIONS, SCHEMA_VERSION, SetId};
    use super::*;

    #[test]
    fn a_database_from_before_track_metadata_upgrades_and_keeps_its_sets() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("dubplate.db");
        let connection = Connection::open(&path).unwrap();
        for migration in &MIGRATIONS[..6] {
            connection.execute_batch(migration).unwrap();
        }
        connection
            .execute_batch(
                "INSERT INTO sets (id, name, target_minutes, created_at, updated_at)
                 VALUES (1, 'Friday', 90, 0, 0);
                 INSERT INTO set_entry (set_id, position, track_path) VALUES (1, 0, '/Music/a.mp3');",
            )
            .unwrap();
        drop(connection);

        let store = SetStore::open(&path).unwrap();
        let version: i64 = store
            .connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, i64::from(SCHEMA_VERSION));
        assert_eq!(store.load_set(SetId(1)).unwrap().main_entries().count(), 1);
        let categories = store.tag_categories().unwrap();
        assert_eq!(
            categories
                .iter()
                .map(|category| category.name.clone())
                .collect::<Vec<_>>(),
            ["Genre", "Components", "Situation", "Other"]
        );
        let backups = std::fs::read_dir(directory.path().join("backups")).unwrap();
        assert!(
            backups
                .filter_map(Result::ok)
                .any(|backup| backup.file_name().to_string_lossy().starts_with("v6-")),
            "the previous version's file is backed up before the upgrade"
        );
    }
}
