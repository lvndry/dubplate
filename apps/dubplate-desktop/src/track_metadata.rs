//! What Dubplate has stored for the library's tracks, read from the set store in one query
//! when the library loads, and kept beside the collection for the search filters to read.

use std::collections::HashMap;
use std::sync::Arc;

use dubplate_prep::{TagCategory, TagId, TrackColour, TrackIdentity, TrackMetadata};

use crate::search_filter::TrackFilterMetadata;

/// The eight track colours in rekordbox's palette order, which the Colour column sorts by and
/// the menu lists.
pub const TRACK_COLOURS: [TrackColour; 8] = [
    TrackColour::Pink,
    TrackColour::Red,
    TrackColour::Orange,
    TrackColour::Yellow,
    TrackColour::Green,
    TrackColour::Aqua,
    TrackColour::Blue,
    TrackColour::Purple,
];

/// The most stars a track can have.
pub const MAX_RATING: u8 = 5;

/// The colour's name, as the menu and the Colour column say it.
pub fn colour_name(colour: TrackColour) -> &'static str {
    match colour {
        TrackColour::Pink => "Pink",
        TrackColour::Red => "Red",
        TrackColour::Orange => "Orange",
        TrackColour::Yellow => "Yellow",
        TrackColour::Green => "Green",
        TrackColour::Aqua => "Aqua",
        TrackColour::Blue => "Blue",
        TrackColour::Purple => "Purple",
    }
}

/// Where the colour comes in rekordbox's palette order, 0 for pink.
pub fn colour_rank(colour: TrackColour) -> usize {
    TRACK_COLOURS
        .iter()
        .position(|candidate| *candidate == colour)
        .unwrap_or(TRACK_COLOURS.len())
}

/// The colour a Colour cell's text names, or `None` for an empty cell.
pub fn colour_named(name: &str) -> Option<TrackColour> {
    TRACK_COLOURS
        .into_iter()
        .find(|colour| colour_name(*colour) == name)
}

/// A rating as the Rating column shows it: one ★ a star, nothing for an unrated track.
pub fn rating_stars(rating: u8) -> String {
    "\u{2605}".repeat(usize::from(rating.min(MAX_RATING)))
}

/// A rating in words, for a screen reader: `4 stars`.
pub fn rating_description(rating: u8) -> String {
    match rating.min(MAX_RATING) {
        0 => "Not rated".to_owned(),
        1 => "1 star".to_owned(),
        stars => format!("{stars} stars"),
    }
}

/// What the filters read of one track, kept by the cache.
#[derive(Debug, Clone)]
pub struct StoredTrackMetadata {
    /// The rating from 0 to 5 stars; 0 is unrated.
    pub rating: u8,
    /// The colour label, when the track has one.
    pub colour: Option<TrackColour>,
    /// The names of the track's My Tags.
    pub tags: Vec<String>,
    /// The ids of the track's My Tags.
    pub tag_ids: Vec<TagId>,
    /// The comment.
    pub comment: String,
}

impl From<&TrackMetadata> for StoredTrackMetadata {
    fn from(stored: &TrackMetadata) -> Self {
        Self {
            rating: stored.rating,
            colour: stored.colour,
            tags: stored.tags.iter().map(|tag| tag.name.clone()).collect(),
            tag_ids: stored.tags.iter().map(|tag| tag.id).collect(),
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
    categories: Vec<TagCategory>,
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
            categories: categories.to_vec(),
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

    /// The DJ's My Tag categories and their tags, in the DJ's order.
    pub fn categories(&self) -> &[TagCategory] {
        &self.categories
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
    fn a_rating_shows_as_that_many_stars_and_none_as_nothing() {
        assert_eq!(rating_stars(0), "");
        assert_eq!(rating_stars(4), "\u{2605}\u{2605}\u{2605}\u{2605}");
        assert_eq!(rating_stars(9), rating_stars(5));
        assert_eq!(rating_description(0), "Not rated");
        assert_eq!(rating_description(1), "1 star");
        assert_eq!(rating_description(3), "3 stars");
    }

    #[test]
    fn colours_keep_rekordbox_palette_order_and_names() {
        let names: Vec<&str> = TRACK_COLOURS.into_iter().map(colour_name).collect();
        assert_eq!(
            names,
            [
                "Pink", "Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple"
            ]
        );
        for (position, colour) in TRACK_COLOURS.into_iter().enumerate() {
            assert_eq!(colour_rank(colour), position);
            assert_eq!(colour_named(colour_name(colour)), Some(colour));
        }
        assert_eq!(colour_named(""), None);
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
