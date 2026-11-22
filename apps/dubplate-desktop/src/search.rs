//! Type-to-search over titles, artists, genres, the folders a track is filed in and comments:
//! every word typed must appear, in any order and case, and every `name:value` filter must
//! keep the track (see [`crate::search_filter`]).

use dubplate_prep::KeyFit;

use crate::collection::{Collection, CrateIndex, TrackIndex};
use crate::search_filter::{SearchFilter, TrackFacts, names_a_filter};

/// A track's searchable text, folded to lower case once when the collection loads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchText(String);

impl SearchText {
    /// Joins `fields` and folds them for matching.
    pub fn new<'field>(fields: impl IntoIterator<Item = &'field str>) -> Self {
        let joined: Vec<&str> = fields.into_iter().collect();
        Self(joined.join(" ").to_lowercase())
    }
}

/// What the DJ typed in the search field: free words and `name:value` filters.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SearchQuery {
    words: Vec<String>,
    filters: Vec<SearchFilter>,
}

impl SearchQuery {
    /// Reads the search field's text. A filter still being typed, such as `bpm:`, is left out
    /// until it reads as one.
    pub fn parse(text: &str) -> Self {
        let mut query = Self::default();
        for word in text.split_whitespace() {
            match SearchFilter::parse(word) {
                Some(filter) => query.filters.push(filter),
                None if names_a_filter(word) => {}
                None => query.words.push(word.to_lowercase()),
            }
        }
        query
    }

    /// True when nothing was typed, so every track matches.
    pub fn is_empty(&self) -> bool {
        self.words.is_empty() && self.filters.is_empty()
    }

    /// Whether the query has filters, which read more of each track than its text.
    pub fn has_filters(&self) -> bool {
        !self.filters.is_empty()
    }

    /// Whether the query hides the tracks played this session, as `played:no` and Hide played
    /// do.
    pub fn hides_played(&self) -> bool {
        self.filters.contains(&SearchFilter::Played(false))
    }

    /// Whether every filter keeps `track`, with `fits` the key moves `key:fits` keeps.
    pub fn filters_keep(&self, track: &TrackFacts<'_>, fits: Option<KeyFit>) -> bool {
        self.filters.iter().all(|filter| filter.keeps(track, fits))
    }

    /// True when every free word appears in `text`.
    pub fn matches(&self, text: &SearchText) -> bool {
        self.words.iter().all(|word| text.0.contains(word.as_str()))
    }
}

/// The tracks of a crate that match a query, in the crate's order.
pub fn filter_tracks(
    collection: &Collection,
    crate_index: CrateIndex,
    query: &SearchQuery,
) -> Vec<TrackIndex> {
    collection
        .crate_at(crate_index)
        .tracks
        .iter()
        .copied()
        .filter(|index| query.matches(&collection.track(*index).search_text))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use dubplate_rekordbox::Track;

    use super::*;
    use crate::collection::RekordboxLibrary;

    fn track(id: &str, title: &str, artist: &str) -> Track {
        Track {
            id: id.to_owned(),
            title: Some(title.to_owned()),
            artist: Some(artist.to_owned()),
            album: None,
            genre: None,
            label: None,
            key: None,
            bpm: None,
            length_seconds: None,
            file_path: None,
            file_size: None,
            rating: None,
            comment: None,
            analysis_path: None,
            date_added: None,
        }
    }

    fn collection() -> Collection {
        Collection::build(
            Some(&RekordboxLibrary::new(
                vec![
                    track("1", "2 Step", "Shimza, Kabza De Small"),
                    track("2", "3rd Eye", "Hyenah, Mr. V"),
                    track("3", "Abalele", "Kabza De Small, DJ Maphorisa"),
                ],
                Vec::new(),
                Vec::new(),
                Path::new("/share"),
            )),
            &[],
        )
    }

    fn titles(collection: &Collection, indices: &[TrackIndex]) -> Vec<String> {
        indices
            .iter()
            .map(|index| collection.track(*index).row.title.to_string())
            .collect()
    }

    #[test]
    fn an_empty_query_keeps_every_track_in_order() {
        let collection = collection();
        let query = SearchQuery::parse("   ");
        assert!(query.is_empty());
        let found = filter_tracks(&collection, CrateIndex::COLLECTION, &query);
        assert_eq!(
            titles(&collection, &found),
            ["2 Step", "3rd Eye", "Abalele"]
        );
    }

    #[test]
    fn every_word_must_match_the_title_or_artist_in_any_case() {
        let collection = collection();
        let by_artist = SearchQuery::parse("KABZA");
        let found = filter_tracks(&collection, CrateIndex::COLLECTION, &by_artist);
        assert_eq!(titles(&collection, &found), ["2 Step", "Abalele"]);

        let mixed = SearchQuery::parse("small step");
        let found = filter_tracks(&collection, CrateIndex::COLLECTION, &mixed);
        assert_eq!(titles(&collection, &found), ["2 Step"]);

        let none = SearchQuery::parse("eye shimza");
        assert!(filter_tracks(&collection, CrateIndex::COLLECTION, &none).is_empty());
    }
}
