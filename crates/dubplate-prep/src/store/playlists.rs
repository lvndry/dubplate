//! The DJ's own playlists and folders of playlists, kept in Dubplate's database and never in
//! rekordbox's. A playlist holds tracks in the DJ's order, each by its identity, so a rekordbox
//! track stays in its playlists when its file is relinked; a folder holds playlists and other
//! folders, as rekordbox's playlist tree does.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::{OptionalExtension, Transaction};

use super::{SetStore, StoreError, now_unix_millis};
use crate::identity::TrackIdentity;

/// Dubplate's identifier for one of the DJ's playlists or folders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PlaylistId(pub i64);

impl std::fmt::Display for PlaylistId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

/// Whether an entry of the playlist tree holds tracks or other entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlaylistKind {
    /// Tracks in the DJ's order.
    Playlist,
    /// Playlists and other folders.
    Folder,
}

/// One track of a playlist: who it is, and where its file was when it was added, for the
/// exports when the library no longer holds it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlaylistTrack {
    /// The track.
    pub identity: TrackIdentity,
    /// Its audio file.
    pub track_path: Option<PathBuf>,
}

/// One entry of the DJ's playlist tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Playlist {
    /// Its id.
    pub id: PlaylistId,
    /// The folder it sits in, `None` at the top.
    pub parent: Option<PlaylistId>,
    /// How deep it sits: 0 at the top.
    pub depth: usize,
    /// The name the DJ gave it.
    pub name: String,
    /// A playlist or a folder.
    pub kind: PlaylistKind,
    /// Its tracks in order; a folder has none of its own.
    pub tracks: Vec<PlaylistTrack>,
}

/// The DJ's playlists and folders in the order the sidebar shows them: each folder followed by
/// what it holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlaylistTree {
    entries: Vec<Playlist>,
    positions: HashMap<PlaylistId, usize>,
}

impl PlaylistTree {
    fn new(entries: Vec<Playlist>) -> Self {
        let positions = entries
            .iter()
            .enumerate()
            .map(|(position, entry)| (entry.id, position))
            .collect();
        Self { entries, positions }
    }

    /// Every entry, each folder followed by what it holds.
    pub fn entries(&self) -> &[Playlist] {
        &self.entries
    }

    /// Whether the DJ has no playlist or folder yet.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The entry `id`, when it is in the tree.
    pub fn get(&self, id: PlaylistId) -> Option<&Playlist> {
        self.entries.get(*self.positions.get(&id)?)
    }

    /// The entries inside the folder `id`, at any depth, in the tree's order. A playlist holds
    /// none.
    pub fn descendants(&self, id: PlaylistId) -> &[Playlist] {
        let Some(&start) = self.positions.get(&id) else {
            return &[];
        };
        let depth = self.entries[start].depth;
        let inside = self.entries[start + 1..]
            .iter()
            .take_while(|entry| entry.depth > depth)
            .count();
        &self.entries[start + 1..start + 1 + inside]
    }

    /// The tracks the entry `id` shows: a playlist's own, or for a folder every track of the
    /// playlists inside it, each once, in the tree's order.
    pub fn tracks(&self, id: PlaylistId) -> Vec<&PlaylistTrack> {
        let Some(entry) = self.get(id) else {
            return Vec::new();
        };
        match entry.kind {
            PlaylistKind::Playlist => entry.tracks.iter().collect(),
            PlaylistKind::Folder => {
                let mut seen = HashSet::new();
                self.descendants(id)
                    .iter()
                    .flat_map(|inside| &inside.tracks)
                    .filter(|track| seen.insert(&track.identity))
                    .collect()
            }
        }
    }

    /// The playlists that can take tracks, in the tree's order.
    pub fn playlists(&self) -> impl Iterator<Item = &Playlist> {
        self.entries
            .iter()
            .filter(|entry| entry.kind == PlaylistKind::Playlist)
    }
}

/// A change to the DJ's playlists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaylistEdit {
    /// Make a playlist or folder at the end of the folder `parent`, or of the top, with tracks
    /// already in it when it is a playlist.
    Create {
        /// The folder it goes in.
        parent: Option<PlaylistId>,
        /// Its name.
        name: String,
        /// A playlist or a folder.
        kind: PlaylistKind,
        /// The tracks a new playlist starts with.
        tracks: Vec<PlaylistTrack>,
    },
    /// Give an entry a new name.
    Rename {
        /// The entry.
        playlist: PlaylistId,
        /// Its new name.
        name: String,
    },
    /// Delete an entry, and for a folder everything inside it. The tracks' files stay.
    Delete(PlaylistId),
    /// Move an entry into the folder `parent`, or the top, at `position` among what is there.
    Move {
        /// The entry.
        playlist: PlaylistId,
        /// Where it goes.
        parent: Option<PlaylistId>,
        /// Its place among the folder's entries, from 0; past the end puts it last.
        position: usize,
    },
    /// Add tracks at the end of a playlist. A track the playlist holds already stays where it
    /// is.
    AddTracks {
        /// The playlist.
        playlist: PlaylistId,
        /// The tracks.
        tracks: Vec<PlaylistTrack>,
    },
    /// Insert tracks into a playlist at `position` in its order, pushing the later tracks
    /// back. A track the playlist holds already stays where it is.
    AddTracksAt {
        /// The playlist.
        playlist: PlaylistId,
        /// The place from 0 the new tracks take; past the end appends.
        position: usize,
        /// The tracks.
        tracks: Vec<PlaylistTrack>,
    },
    /// Take tracks out of a playlist.
    RemoveTracks {
        /// The playlist.
        playlist: PlaylistId,
        /// The tracks.
        tracks: Vec<TrackIdentity>,
    },
    /// Move one track of a playlist to `position` in its order.
    MoveTrack {
        /// The playlist.
        playlist: PlaylistId,
        /// The track.
        track: TrackIdentity,
        /// Its new place from 0; past the end puts it last.
        position: usize,
    },
}

/// What an edit to the playlists did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlaylistEdited {
    /// The entry it made, for [`PlaylistEdit::Create`].
    pub created: Option<PlaylistId>,
    /// How many tracks it put in a playlist.
    pub added: usize,
    /// How many tracks it was asked to add that the playlist held already.
    pub already_there: usize,
}

const PLAYLIST_KIND: &str = "playlist";
const FOLDER_KIND: &str = "folder";

impl PlaylistKind {
    fn storage_name(self) -> &'static str {
        match self {
            Self::Playlist => PLAYLIST_KIND,
            Self::Folder => FOLDER_KIND,
        }
    }

    fn from_storage_name(name: &str) -> Option<Self> {
        match name {
            PLAYLIST_KIND => Some(Self::Playlist),
            FOLDER_KIND => Some(Self::Folder),
            _ => None,
        }
    }
}

/// A row of the `playlist` table before the tree is put together.
struct PlaylistRow {
    id: PlaylistId,
    parent: Option<PlaylistId>,
    name: String,
    kind: PlaylistKind,
}

impl SetStore {
    /// The DJ's playlists and folders, each folder followed by what it holds.
    pub fn playlists(&self) -> Result<PlaylistTree, StoreError> {
        let rows: Vec<PlaylistRow> = self
            .connection
            .prepare_cached(
                "SELECT id, parent_id, name, kind FROM playlist ORDER BY parent_id, position, id",
            )?
            .query_map([], |row| {
                Ok((
                    PlaylistId(row.get(0)?),
                    row.get::<_, Option<i64>>(1)?.map(PlaylistId),
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?
            .filter_map(|row| {
                row.map(|(id, parent, name, kind)| {
                    Some(PlaylistRow {
                        id,
                        parent,
                        name,
                        kind: PlaylistKind::from_storage_name(&kind)?,
                    })
                })
                .transpose()
            })
            .collect::<Result<_, _>>()?;
        let mut tracks: HashMap<PlaylistId, Vec<PlaylistTrack>> = HashMap::new();
        let mut statement = self.connection.prepare_cached(
            "SELECT playlist_id, identity_kind, identity, track_path FROM playlist_track
             ORDER BY playlist_id, position",
        )?;
        let mut track_rows = statement.query([])?;
        while let Some(row) = track_rows.next()? {
            let kind: String = row.get(1)?;
            let Some(identity) = TrackIdentity::from_storage_key(&kind, row.get(2)?) else {
                continue;
            };
            tracks
                .entry(PlaylistId(row.get(0)?))
                .or_default()
                .push(PlaylistTrack {
                    identity,
                    track_path: row.get::<_, Option<String>>(3)?.map(PathBuf::from),
                });
        }
        let mut children: HashMap<Option<PlaylistId>, Vec<&PlaylistRow>> = HashMap::new();
        for row in &rows {
            children.entry(row.parent).or_default().push(row);
        }
        let mut entries = Vec::with_capacity(rows.len());
        add_children(&children, None, 0, &mut tracks, &mut entries);
        Ok(PlaylistTree::new(entries))
    }

    /// Carries out `edit` in one transaction: all of it, or nothing when it fails.
    pub fn edit_playlists(&mut self, edit: PlaylistEdit) -> Result<PlaylistEdited, StoreError> {
        let transaction = self.transaction()?;
        let edited = apply(&transaction, edit)?;
        transaction.commit()?;
        Ok(edited)
    }
}

/// Puts the entries inside `parent` into `entries` depth first, each followed by what it holds.
fn add_children(
    children: &HashMap<Option<PlaylistId>, Vec<&PlaylistRow>>,
    parent: Option<PlaylistId>,
    depth: usize,
    tracks: &mut HashMap<PlaylistId, Vec<PlaylistTrack>>,
    entries: &mut Vec<Playlist>,
) {
    for row in children.get(&parent).into_iter().flatten() {
        entries.push(Playlist {
            id: row.id,
            parent: row.parent,
            depth,
            name: row.name.clone(),
            kind: row.kind,
            tracks: tracks.remove(&row.id).unwrap_or_default(),
        });
        if row.kind == PlaylistKind::Folder {
            add_children(children, Some(row.id), depth + 1, tracks, entries);
        }
    }
}

fn apply(transaction: &Transaction<'_>, edit: PlaylistEdit) -> Result<PlaylistEdited, StoreError> {
    match edit {
        PlaylistEdit::Create {
            parent,
            name,
            kind,
            tracks,
        } => {
            if let Some(folder) = parent {
                require_kind(transaction, folder, PlaylistKind::Folder)?;
            }
            let position = sibling_ids(transaction, parent)?.len();
            let timestamp = now_unix_millis();
            transaction.execute(
                "INSERT INTO playlist (parent_id, position, name, kind, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                (
                    parent.map(|folder| folder.0),
                    position as i64,
                    &name,
                    kind.storage_name(),
                    timestamp,
                ),
            )?;
            let created = PlaylistId(transaction.last_insert_rowid());
            let mut edited = PlaylistEdited {
                created: Some(created),
                ..PlaylistEdited::default()
            };
            if kind == PlaylistKind::Playlist && !tracks.is_empty() {
                let added = add_tracks(transaction, created, tracks)?;
                edited.added = added.added;
                edited.already_there = added.already_there;
            }
            Ok(edited)
        }
        PlaylistEdit::Rename { playlist, name } => {
            let changed = transaction.execute(
                "UPDATE playlist SET name = ?2, updated_at = ?3 WHERE id = ?1",
                (playlist.0, &name, now_unix_millis()),
            )?;
            if changed == 0 {
                return Err(StoreError::PlaylistNotFound(playlist));
            }
            Ok(PlaylistEdited::default())
        }
        PlaylistEdit::Delete(playlist) => {
            let parent = parent_of(transaction, playlist)?;
            transaction.execute("DELETE FROM playlist WHERE id = ?1", (playlist.0,))?;
            let siblings = sibling_ids(transaction, parent)?;
            write_sibling_order(transaction, &siblings)?;
            Ok(PlaylistEdited::default())
        }
        PlaylistEdit::Move {
            playlist,
            parent,
            position,
        } => {
            let old_parent = parent_of(transaction, playlist)?;
            if let Some(folder) = parent {
                require_kind(transaction, folder, PlaylistKind::Folder)?;
                if folder == playlist || is_inside(transaction, folder, playlist)? {
                    return Err(StoreError::PlaylistInsideItself(playlist));
                }
            }
            let mut old_siblings = sibling_ids(transaction, old_parent)?;
            old_siblings.retain(|sibling| *sibling != playlist);
            write_sibling_order(transaction, &old_siblings)?;
            let mut new_siblings = sibling_ids(transaction, parent)?;
            new_siblings.retain(|sibling| *sibling != playlist);
            new_siblings.insert(position.min(new_siblings.len()), playlist);
            transaction.execute(
                "UPDATE playlist SET parent_id = ?2, updated_at = ?3 WHERE id = ?1",
                (playlist.0, parent.map(|folder| folder.0), now_unix_millis()),
            )?;
            write_sibling_order(transaction, &new_siblings)?;
            Ok(PlaylistEdited::default())
        }
        PlaylistEdit::AddTracks { playlist, tracks } => {
            require_kind(transaction, playlist, PlaylistKind::Playlist)?;
            add_tracks(transaction, playlist, tracks)
        }
        PlaylistEdit::RemoveTracks { playlist, tracks } => {
            require_kind(transaction, playlist, PlaylistKind::Playlist)?;
            let removed: HashSet<&TrackIdentity> = tracks.iter().collect();
            let mut kept = playlist_tracks(transaction, playlist)?;
            kept.retain(|track| !removed.contains(&track.identity));
            write_tracks(transaction, playlist, &kept)?;
            Ok(PlaylistEdited::default())
        }
        PlaylistEdit::AddTracksAt {
            playlist,
            position,
            tracks,
        } => {
            require_kind(transaction, playlist, PlaylistKind::Playlist)?;
            let ordered = playlist_tracks(transaction, playlist)?;
            let mut held: HashSet<&TrackIdentity> =
                ordered.iter().map(|track| &track.identity).collect();
            let asked = tracks.len();
            let new_tracks: Vec<&PlaylistTrack> = tracks
                .iter()
                .filter(|track| held.insert(&track.identity))
                .collect();
            let added = new_tracks.len();
            if added > 0 {
                let mut inserted = ordered;
                for (offset, track) in new_tracks.iter().enumerate() {
                    inserted.insert(position.min(inserted.len()) + offset, (*track).clone());
                }
                write_tracks(transaction, playlist, &inserted)?;
            }
            Ok(PlaylistEdited {
                created: None,
                added,
                already_there: asked - added,
            })
        }
        PlaylistEdit::MoveTrack {
            playlist,
            track,
            position,
        } => {
            require_kind(transaction, playlist, PlaylistKind::Playlist)?;
            let mut ordered = playlist_tracks(transaction, playlist)?;
            let Some(from) = ordered.iter().position(|kept| kept.identity == track) else {
                return Ok(PlaylistEdited::default());
            };
            let moved = ordered.remove(from);
            ordered.insert(position.min(ordered.len()), moved);
            write_tracks(transaction, playlist, &ordered)?;
            Ok(PlaylistEdited::default())
        }
    }
}

/// Adds `tracks` at the end of `playlist`, skipping those it holds already and repeats.
fn add_tracks(
    transaction: &Transaction<'_>,
    playlist: PlaylistId,
    tracks: Vec<PlaylistTrack>,
) -> Result<PlaylistEdited, StoreError> {
    let existing = playlist_tracks(transaction, playlist)?;
    let mut held: HashSet<&TrackIdentity> = existing.iter().map(|track| &track.identity).collect();
    let asked = tracks.len();
    let new_tracks: Vec<&PlaylistTrack> = tracks
        .iter()
        .filter(|track| held.insert(&track.identity))
        .collect();
    let added = new_tracks.len();
    if added > 0 {
        insert_tracks(transaction, playlist, existing.len(), new_tracks)?;
        touch_playlist(transaction, playlist)?;
    }
    Ok(PlaylistEdited {
        created: None,
        added,
        already_there: asked - added,
    })
}

/// The tracks of `playlist` in order.
fn playlist_tracks(
    transaction: &Transaction<'_>,
    playlist: PlaylistId,
) -> Result<Vec<PlaylistTrack>, StoreError> {
    let mut statement = transaction.prepare_cached(
        "SELECT identity_kind, identity, track_path FROM playlist_track
         WHERE playlist_id = ?1 ORDER BY position",
    )?;
    let mut rows = statement.query((playlist.0,))?;
    let mut tracks = Vec::new();
    while let Some(row) = rows.next()? {
        let kind: String = row.get(0)?;
        if let Some(identity) = TrackIdentity::from_storage_key(&kind, row.get(1)?) {
            tracks.push(PlaylistTrack {
                identity,
                track_path: row.get::<_, Option<String>>(2)?.map(PathBuf::from),
            });
        }
    }
    Ok(tracks)
}

/// Writes `tracks` as the whole of `playlist`, in order.
fn write_tracks(
    transaction: &Transaction<'_>,
    playlist: PlaylistId,
    tracks: &[PlaylistTrack],
) -> Result<(), StoreError> {
    transaction.execute(
        "DELETE FROM playlist_track WHERE playlist_id = ?1",
        (playlist.0,),
    )?;
    insert_tracks(transaction, playlist, 0, tracks)?;
    touch_playlist(transaction, playlist)
}

/// Inserts `tracks` into `playlist` from `first_position` on.
fn insert_tracks<'track>(
    transaction: &Transaction<'_>,
    playlist: PlaylistId,
    first_position: usize,
    tracks: impl IntoIterator<Item = &'track PlaylistTrack>,
) -> Result<(), StoreError> {
    let mut insert = transaction.prepare_cached(
        "INSERT INTO playlist_track (playlist_id, position, identity_kind, identity, track_path)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    for (offset, track) in tracks.into_iter().enumerate() {
        let (kind, key) = track.identity.storage_key();
        insert.execute((
            playlist.0,
            (first_position + offset) as i64,
            kind,
            &key,
            track.track_path.as_deref().map(Path::to_string_lossy),
        ))?;
    }
    Ok(())
}

/// Marks `playlist` as changed now.
fn touch_playlist(transaction: &Transaction<'_>, playlist: PlaylistId) -> Result<(), StoreError> {
    transaction.execute(
        "UPDATE playlist SET updated_at = ?2 WHERE id = ?1",
        (playlist.0, now_unix_millis()),
    )?;
    Ok(())
}

/// The entries in the folder `parent`, or at the top, in order.
fn sibling_ids(
    transaction: &Transaction<'_>,
    parent: Option<PlaylistId>,
) -> Result<Vec<PlaylistId>, StoreError> {
    let mut statement = transaction
        .prepare_cached("SELECT id FROM playlist WHERE parent_id IS ?1 ORDER BY position, id")?;
    let ids = statement
        .query_map((parent.map(|folder| folder.0),), |row| {
            Ok(PlaylistId(row.get(0)?))
        })?
        .collect::<Result<_, _>>()?;
    Ok(ids)
}

/// Numbers `siblings` from 0 in the order given.
fn write_sibling_order(
    transaction: &Transaction<'_>,
    siblings: &[PlaylistId],
) -> Result<(), StoreError> {
    let mut update =
        transaction.prepare_cached("UPDATE playlist SET position = ?2 WHERE id = ?1")?;
    for (position, sibling) in siblings.iter().enumerate() {
        update.execute((sibling.0, position as i64))?;
    }
    Ok(())
}

/// The folder `playlist` sits in, failing when it is gone.
fn parent_of(
    transaction: &Transaction<'_>,
    playlist: PlaylistId,
) -> Result<Option<PlaylistId>, StoreError> {
    transaction
        .query_row(
            "SELECT parent_id FROM playlist WHERE id = ?1",
            (playlist.0,),
            |row| row.get::<_, Option<i64>>(0),
        )
        .optional()?
        .map(|parent| parent.map(PlaylistId))
        .ok_or(StoreError::PlaylistNotFound(playlist))
}

/// Fails unless `playlist` is there and is a `kind`.
fn require_kind(
    transaction: &Transaction<'_>,
    playlist: PlaylistId,
    kind: PlaylistKind,
) -> Result<(), StoreError> {
    let found: Option<String> = transaction
        .query_row(
            "SELECT kind FROM playlist WHERE id = ?1",
            (playlist.0,),
            |row| row.get(0),
        )
        .optional()?;
    match found.as_deref().and_then(PlaylistKind::from_storage_name) {
        None => Err(StoreError::PlaylistNotFound(playlist)),
        Some(found) if found == kind => Ok(()),
        Some(_) => Err(StoreError::WrongPlaylistKind { playlist, kind }),
    }
}

/// Whether `folder` sits inside `ancestor`, at any depth.
fn is_inside(
    transaction: &Transaction<'_>,
    folder: PlaylistId,
    ancestor: PlaylistId,
) -> Result<bool, StoreError> {
    let mut current = parent_of(transaction, folder)?;
    let mut visited = HashSet::new();
    while let Some(parent) = current {
        if parent == ancestor {
            return Ok(true);
        }
        if !visited.insert(parent) {
            return Ok(false);
        }
        current = parent_of(transaction, parent)?;
    }
    Ok(false)
}

/// Points the playlist tracks kept for the file `from` at the file `to`. A playlist that holds
/// both keeps the first.
pub(super) fn move_file_tracks(
    transaction: &Transaction<'_>,
    from: &Path,
    to: &Path,
) -> Result<(), StoreError> {
    let moved = TrackIdentity::File(from.to_path_buf());
    let (kind, from_key) = moved.storage_key();
    let to_key = to.to_string_lossy();
    let playlists: Vec<i64> = transaction
        .prepare_cached(
            "SELECT DISTINCT playlist_id FROM playlist_track
             WHERE identity_kind = ?1 AND identity = ?2",
        )?
        .query_map((kind, &from_key), |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    for playlist in playlists {
        let playlist = PlaylistId(playlist);
        let mut tracks = playlist_tracks(transaction, playlist)?;
        let mut seen = HashSet::new();
        for track in &mut tracks {
            if track.identity == moved {
                track.identity = TrackIdentity::File(to.to_path_buf());
            }
        }
        tracks.retain(|track| seen.insert(track.identity.clone()));
        write_tracks(transaction, playlist, &tracks)?;
    }
    transaction.execute(
        "UPDATE playlist_track SET track_path = ?2 WHERE track_path = ?1",
        (from.to_string_lossy(), &to_key),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::super::{MIGRATIONS, SCHEMA_VERSION, SetId};
    use super::*;

    #[test]
    fn a_database_from_before_playlists_upgrades_and_keeps_its_sets() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("dubplate.db");
        let connection = Connection::open(&path).unwrap();
        for migration in &MIGRATIONS[..usize::try_from(SCHEMA_VERSION - 1).unwrap()] {
            connection.execute_batch(migration).unwrap();
        }
        connection
            .execute_batch(
                "INSERT INTO sets (id, name, target_minutes, created_at, updated_at)
                 VALUES (1, 'Friday', 90, 0, 0);
                 INSERT INTO set_entry (set_id, position, track_path) VALUES (1, 0, '/a.mp3');",
            )
            .unwrap();
        drop(connection);

        let mut store = SetStore::open(&path).unwrap();
        let version: i64 = store
            .connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, i64::from(SCHEMA_VERSION));
        assert_eq!(store.load_set(SetId(1)).unwrap().main_entries().count(), 1);
        assert!(store.playlists().unwrap().is_empty());
        let backups = std::fs::read_dir(directory.path().join("backups")).unwrap();
        assert!(
            backups
                .filter_map(Result::ok)
                .any(|backup| backup.file_name().to_string_lossy().starts_with("v5-")),
            "the version 5 file is backed up before the upgrade"
        );

        let created = store
            .edit_playlists(PlaylistEdit::Create {
                parent: None,
                name: "Peak".to_owned(),
                kind: PlaylistKind::Playlist,
                tracks: Vec::new(),
            })
            .unwrap();
        assert!(created.created.is_some());
    }
}
