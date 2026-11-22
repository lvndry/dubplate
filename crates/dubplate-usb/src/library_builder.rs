//! Builds the Device Library of an export on top of what the stick already holds: names become
//! rows once, tracks keep their ids so players' histories still point at them, and a playlist of
//! the same name is replaced.

use std::collections::HashMap;

use dubplate_rekordbox::device_library::{
    Album, AlbumId, ArtistId, DeviceLibrary, DeviceTrack, GenreId, KeyId, LabelId, Named,
    PlaylistEntry, PlaylistId, PlaylistNode, TrackId,
};

/// Hands out the next id of one table.
#[derive(Debug, Clone, Copy)]
struct NextId(u32);

impl NextId {
    fn after(highest: Option<u32>) -> Self {
        Self(highest.unwrap_or(0) + 1)
    }

    fn take<Id>(&mut self, make: fn(u32) -> Option<Id>) -> Id {
        let id = make(self.0).unwrap_or_else(|| unreachable!("ids start at 1"));
        self.0 += 1;
        id
    }
}

/// A table of names, each given one row.
#[derive(Debug)]
struct NameTable<Id> {
    ids: HashMap<String, Id>,
    next: NextId,
}

impl<Id: Copy> NameTable<Id> {
    fn new(rows: &[Named<Id>], number: fn(Id) -> u32) -> Self {
        Self {
            ids: rows.iter().map(|row| (row.name.clone(), row.id)).collect(),
            next: NextId::after(rows.iter().map(|row| number(row.id)).max()),
        }
    }

    /// The row of `name`, made when new, or `None` for no name.
    fn id(
        &mut self,
        name: Option<&str>,
        make: fn(u32) -> Option<Id>,
        rows: &mut Vec<Named<Id>>,
    ) -> Option<Id> {
        let name = name.map(str::trim).filter(|name| !name.is_empty())?;
        if let Some(id) = self.ids.get(name) {
            return Some(*id);
        }
        let id = self.next.take(make);
        self.ids.insert(name.to_owned(), id);
        rows.push(Named {
            id,
            name: name.to_owned(),
        });
        Some(id)
    }
}

/// The library of an export, grown from the one on the stick.
#[derive(Debug)]
pub(crate) struct LibraryBuilder {
    library: DeviceLibrary,
    artists: NameTable<ArtistId>,
    genres: NameTable<GenreId>,
    labels: NameTable<LabelId>,
    keys: NameTable<KeyId>,
    albums: HashMap<(String, Option<ArtistId>), AlbumId>,
    next_album: NextId,
    tracks_by_path: HashMap<String, TrackId>,
    next_track: NextId,
    next_playlist: NextId,
}

impl LibraryBuilder {
    /// A builder that adds to `library`.
    pub(crate) fn new(library: DeviceLibrary) -> Self {
        Self {
            artists: NameTable::new(&library.artists, ArtistId::get),
            genres: NameTable::new(&library.genres, GenreId::get),
            labels: NameTable::new(&library.labels, LabelId::get),
            keys: NameTable::new(&library.keys, KeyId::get),
            albums: library
                .albums
                .iter()
                .map(|album| ((album.name.clone(), album.artist), album.id))
                .collect(),
            next_album: NextId::after(library.albums.iter().map(|album| album.id.get()).max()),
            tracks_by_path: library
                .tracks
                .iter()
                .map(|track| (track.file_path.clone(), track.id))
                .collect(),
            next_track: NextId::after(library.tracks.iter().map(|track| track.id.get()).max()),
            next_playlist: NextId::after(library.playlists.iter().map(|node| node.id.get()).max()),
            library,
        }
    }

    /// The library so far.
    pub(crate) fn library(&self) -> &DeviceLibrary {
        &self.library
    }

    /// The finished library.
    pub(crate) fn finish(self) -> DeviceLibrary {
        self.library
    }

    pub(crate) fn artist(&mut self, name: Option<&str>) -> Option<ArtistId> {
        self.artists
            .id(name, ArtistId::new, &mut self.library.artists)
    }

    pub(crate) fn genre(&mut self, name: Option<&str>) -> Option<GenreId> {
        self.genres.id(name, GenreId::new, &mut self.library.genres)
    }

    pub(crate) fn label(&mut self, name: Option<&str>) -> Option<LabelId> {
        self.labels.id(name, LabelId::new, &mut self.library.labels)
    }

    pub(crate) fn key(&mut self, name: Option<&str>) -> Option<KeyId> {
        self.keys.id(name, KeyId::new, &mut self.library.keys)
    }

    pub(crate) fn album(
        &mut self,
        name: Option<&str>,
        artist: Option<ArtistId>,
    ) -> Option<AlbumId> {
        let name = name.map(str::trim).filter(|name| !name.is_empty())?;
        let key = (name.to_owned(), artist);
        if let Some(id) = self.albums.get(&key) {
            return Some(*id);
        }
        let id = self.next_album.take(AlbumId::new);
        self.albums.insert(key, id);
        self.library.albums.push(Album {
            id,
            artist,
            name: name.to_owned(),
        });
        Some(id)
    }

    /// The id of the track at `file_path` on the stick: the one it had when the stick already
    /// holds it, else a new one.
    pub(crate) fn track_id(&mut self, file_path: &str) -> TrackId {
        if let Some(id) = self.tracks_by_path.get(file_path) {
            return *id;
        }
        let id = self.next_track.take(TrackId::new);
        self.tracks_by_path.insert(file_path.to_owned(), id);
        id
    }

    /// Puts `track` in the library, in place of the row with its id.
    pub(crate) fn put_track(&mut self, track: DeviceTrack) {
        match self
            .library
            .tracks
            .iter_mut()
            .find(|existing| existing.id == track.id)
        {
            Some(existing) => *existing = track,
            None => self.library.tracks.push(track),
        }
    }

    /// Makes the top-level playlist `name` hold `tracks` in order, in place of a top-level
    /// playlist of that name the stick already has, or after the others.
    pub(crate) fn put_playlist(&mut self, name: &str, tracks: &[TrackId]) -> PlaylistId {
        let existing = self
            .library
            .playlists
            .iter()
            .find(|node| node.parent.is_none() && !node.is_folder && node.name == name)
            .map(|node| node.id);
        let id = existing.unwrap_or_else(|| {
            let id = self.next_playlist.take(PlaylistId::new);
            let sort_order = self
                .library
                .playlists
                .iter()
                .filter(|node| node.parent.is_none())
                .map(|node| node.sort_order + 1)
                .max()
                .unwrap_or(0);
            self.library.playlists.push(PlaylistNode {
                id,
                parent: None,
                sort_order,
                is_folder: false,
                name: name.to_owned(),
            });
            id
        });
        self.library
            .playlist_entries
            .retain(|entry| entry.playlist != id);
        self.library
            .playlist_entries
            .extend((1..).zip(tracks).map(|(entry_index, track)| PlaylistEntry {
                entry_index,
                track: *track,
                playlist: id,
            }));
        id
    }
}

#[cfg(test)]
mod tests {
    use dubplate_rekordbox::device_library::LibraryId;

    use super::*;

    #[test]
    fn names_get_one_row_each_and_ids_carry_on_from_the_stick() {
        let mut builder = LibraryBuilder::new(DeviceLibrary::new(LibraryId(1), "2026-11-22"));
        let shimza = builder.artist(Some("Shimza"));
        assert_eq!(builder.artist(Some(" Shimza ")), shimza);
        assert_eq!(builder.artist(Some("")), None);
        let first = builder.track_id("/Contents/A/B/one.mp3");
        builder.put_playlist("Saturday", &[first]);
        let library = builder.finish();

        let mut again = LibraryBuilder::new(library);
        assert_eq!(again.artist(Some("Shimza")), shimza);
        assert_eq!(again.track_id("/Contents/A/B/one.mp3"), first);
        let second = again.track_id("/Contents/A/B/two.mp3");
        assert_eq!(second.get(), first.get() + 1);
        let saturday = again.put_playlist("Saturday", &[second, first]);
        let sunday = again.put_playlist("Sunday", &[second]);
        let library = again.finish();
        assert_eq!(library.playlists.len(), 2);
        let saturday_tracks: Vec<TrackId> = library
            .playlist_entries
            .iter()
            .filter(|entry| entry.playlist == saturday)
            .map(|entry| entry.track)
            .collect();
        assert_eq!(saturday_tracks, [second, first]);
        let sunday_node = library
            .playlists
            .iter()
            .find(|node| node.id == sunday)
            .unwrap();
        assert_eq!(sunday_node.sort_order, 1);
    }
}
