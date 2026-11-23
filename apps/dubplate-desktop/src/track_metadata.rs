//! What Dubplate has stored for the library's tracks, read from the set store in one query
//! when the library loads, and kept beside the collection for the search filters to read.

use std::collections::HashMap;
use std::sync::Arc;

use dubplate_prep::{TagCategory, TrackColour, TrackIdentity, TrackMetadata};

use crate::search_filter::TrackFilterMetadata;

/// What the filters read of one track, kept by the cache.
#[derive(Debug, Clone)]
pub struct StoredTrackMetadata {
    /// The rating from 0 to 5 stars; 0 is unrated.
    pub rating: u8,
    /// The colour label, when the track has one.
    pub colour: Option<TrackColour>,
    /// The names of the track's My Tags.
    pub tags: Vec<String>,
    /// The comment.
    pub comment: String,
}

impl From<&TrackMetadata> for StoredTrackMetadata {
    fn from(stored: &TrackMetadata) -> Self {
        Self {
            rating: stored.rating,
            colour: stored.colour,
            tags: stored.tags.iter().map(|tag| tag.name.clone()).collect(),
            comment: stored.comment.clone(),
        }
    }
}

/// The stored metadata of the library's tracks and the DJ's My Tag names, as one read of the
/// set store keeps them.
#[derive(Debug, Clone, Default)]
pub struct TrackMetadataCache {
    by_identity: HashMap<TrackIdentity, Arc<StoredTrackMetadata>>,
    tag_names: Vec<String>,
}

impl TrackMetadataCache {
    /// Builds the cache: each track's stored metadata, seeded from rekordbox for the tracks
    /// Dubplate has nothing stored for.
    pub fn from_library(
        tracks: impl IntoIterator<Item = (TrackIdentity, TrackMetadata)>,
        categories: &[TagCategory],
    ) -> Self {
        let tag_names = categories
            .iter()
            .flat_map(|category| category.tags.iter().map(|tag| tag.name.clone()))
            .collect();
        let by_identity = tracks
            .into_iter()
            .map(|(identity, stored)| (identity, Arc::new(StoredTrackMetadata::from(&stored))))
            .collect();
        Self {
            by_identity,
            tag_names,
        }
    }

    /// What Dubplate has stored for the track `identity`; `None` when the cache holds nothing
    /// for it.
    pub fn for_track(&self, identity: &TrackIdentity) -> Option<Arc<StoredTrackMetadata>> {
        self.by_identity.get(identity).cloned()
    }

    /// What the filters read of the track `identity`; nothing stored when the cache holds
    /// nothing for it.
    pub fn facts_for(&self, identity: &TrackIdentity) -> TrackFilterMetadata {
        self.for_track(identity)
            .map_or_else(TrackFilterMetadata::none, |stored| TrackFilterMetadata {
                rating: stored.rating,
                colour: stored.colour,
                tags: stored.tags.clone(),
                comment: stored.comment.clone(),
            })
    }

    /// The names of the DJ's My Tags, in the order of the categories.
    pub fn tag_names(&self) -> &[String] {
        &self.tag_names
    }
}

#[cfg(test)]
mod tests {
    use dubplate_rekordbox::Track;

    use super::*;

    fn rekordbox_track(id: &str, rating: u8) -> Track {
        Track {
            id: id.to_owned(),
            title: Some("Title".to_owned()),
            artist: Some("Artist".to_owned()),
            album: None,
            genre: None,
            label: None,
            key: None,
            bpm: None,
            length_seconds: None,
            file_path: None,
            file_size: None,
            rating: Some(rating),
            comment: None,
            analysis_path: None,
            date_added: None,
            auto_gain_db: None,
        }
    }

    #[test]
    fn a_track_without_stored_metadata_keeps_what_rekordbox_has() {
        let seeded = TrackMetadata::from_rekordbox(&rekordbox_track("42", 4));
        let cache = TrackMetadataCache::from_library(
            [(TrackIdentity::Rekordbox("42".to_owned()), seeded)],
            &[],
        );
        let stored = cache
            .for_track(&TrackIdentity::Rekordbox("42".to_owned()))
            .ok_or("seeded track missing")
            .unwrap();
        assert_eq!(stored.rating, 4);
        assert_eq!(stored.colour, None);
        assert!(stored.tags.is_empty());
    }

    #[test]
    fn stored_metadata_wins_and_missing_tracks_keep_none() {
        let stored = TrackMetadata {
            track_path: None,
            rating: 2,
            colour: Some(TrackColour::Blue),
            comment: "Closing".to_owned(),
            tags: vec![],
        };
        let cache = TrackMetadataCache::from_library(
            [(TrackIdentity::Rekordbox("1".to_owned()), stored)],
            &[],
        );
        let found = cache
            .for_track(&TrackIdentity::Rekordbox("1".to_owned()))
            .ok_or("stored track missing")
            .unwrap();
        assert_eq!(found.rating, 2);
        assert_eq!(found.colour, Some(TrackColour::Blue));
        assert_eq!(found.comment, "Closing");
        assert!(
            cache
                .for_track(&TrackIdentity::Rekordbox("2".to_owned()))
                .is_none()
        );
    }
}
