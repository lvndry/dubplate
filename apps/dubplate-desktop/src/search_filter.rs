//! Quick filters typed in the search field as `name:value`, such as `bpm:120-124`, `key:8A`,
//! `key:fits`, `genre:amapiano`, `energy:6+`, `artist:kabza`, `played:no`, `rated:4+`,
//! `color:red`, `tag:peak` and `comment:club`, mixed with the free words; and the suggestions
//! that teach them as the DJ types.

use dubplate_prep::{Key, KeyFit, TrackColour};

/// The filter names, in the order the suggestions offer them.
const NAMES: [&str; 10] = [
    "bpm", "key", "genre", "energy", "artist", "played", "rated", "color", "tag", "comment",
];
/// How far either side of a single tempo, such as `bpm:120`, still matches.
const BPM_TOLERANCE: f64 = 0.5;
/// How far either side of the reference tempo a suggested BPM range reaches.
const SUGGESTED_BPM_SPAN: f64 = 2.0;
const MAX_ENERGY: f32 = 10.0;
const RATING_LIMIT: u8 = 5;
/// The colour names, in the order the suggestions offer them.
const COLOUR_NAMES: [&str; 8] = [
    "pink", "red", "orange", "yellow", "green", "aqua", "blue", "purple",
];

/// Inclusive bounds on a number.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds<Number> {
    lowest: Number,
    highest: Number,
}

impl<Number: PartialOrd + Copy> Bounds<Number> {
    fn keeps(self, value: Number) -> bool {
        self.lowest <= value && value <= self.highest
    }
}

/// Which keys a `key:` filter keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyFilter {
    /// One key, in any notation.
    Is(Key),
    /// The keys that mix well after the reference track's, as Key fits keeps them.
    Fits,
}

/// Which ratings a `rated:` filter keeps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RatedFilter {
    /// No rating at all: `rated:0` or `rated:none`.
    Unrated,
    /// A rating inside the bounds: `rated:4`, `rated:4+` or `rated:4-5`.
    Stars(Bounds<u8>),
}

/// One `name:value` filter.
#[derive(Debug, Clone, PartialEq)]
pub enum SearchFilter {
    /// `bpm:120`, `bpm:120-124` or `bpm:120+`.
    Bpm(Bounds<f64>),
    /// `key:8A` or `key:fits`.
    Key(KeyFilter),
    /// `genre:amapiano`: the genre, or the folders the track is filed in, contains it.
    Genre(String),
    /// `artist:kabza`: an artist's name contains it.
    Artist(String),
    /// `energy:6`, `energy:6+` or `energy:4-6`.
    Energy(Bounds<f32>),
    /// `played:yes` or `played:no`: whether a deck played the track this session, a minute or
    /// more, as the history counts a play.
    Played(bool),
    /// `rated:4`, `rated:4+`, or `rated:0`/`rated:none` for the unrated tracks.
    Rated(RatedFilter),
    /// `color:red` (spelled `colour:` too), or `color:none` for the tracks without a label.
    Color(Option<TrackColour>),
    /// `tag:peak`: the track carries a My Tag whose name matches, case-insensitive.
    Tag(String),
    /// `comment:club`: the track's comment contains the word, case-insensitive.
    Comment(String),
}

/// The stored metadata the `rated:`, `color:`, `tag:` and `comment:` filters read of a track.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackFilterMetadata {
    /// The rating from 0 to 5 stars; 0 is unrated.
    pub rating: u8,
    /// The colour label, when the track has one.
    pub colour: Option<TrackColour>,
    /// The names of the track's My Tags.
    pub tags: Vec<String>,
    /// The comment.
    pub comment: String,
}

impl TrackFilterMetadata {
    /// Metadata without any stored rating, colour, tag or comment.
    pub fn none() -> Self {
        Self {
            rating: 0,
            colour: None,
            tags: Vec::new(),
            comment: String::new(),
        }
    }
}

/// What the filters read of one track.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackFacts<'track> {
    /// The artists, as the table shows them.
    pub artist: &'track str,
    /// The genre cell, with the folders when the track has no genre tag.
    pub genre: &'track str,
    /// The tempo the BPM column shows.
    pub bpm: Option<f64>,
    /// The key the Key column shows.
    pub key: Option<Key>,
    /// The energy the Energy column shows.
    pub energy: Option<f32>,
    /// Whether a deck played it this session.
    pub played: bool,
    /// What Dubplate has stored for the track, or what rekordbox has when nothing is stored.
    pub metadata: TrackFilterMetadata,
}

/// Whether `word` names a filter, typed in full or not, so a half-typed one is left out of the
/// free words instead of matching nothing.
pub fn names_a_filter(word: &str) -> bool {
    word.split_once(':').is_some_and(|(name, _)| {
        let name = name.to_lowercase();
        name == "colour" || NAMES.contains(&name.as_str())
    })
}

impl SearchFilter {
    /// The filter `word` spells, or `None` for a word that is no filter, such as one still
    /// being typed (`bpm:12`... is a filter, `bpm:` not yet).
    pub fn parse(word: &str) -> Option<Self> {
        let (name, value) = word.split_once(':')?;
        let value = value.trim();
        if value.is_empty() {
            return None;
        }
        let name = name.to_lowercase();
        match name.as_str() {
            "bpm" => number_bounds(value, f64::INFINITY, BPM_TOLERANCE).map(Self::Bpm),
            "key" if value.eq_ignore_ascii_case("fits") => Some(Self::Key(KeyFilter::Fits)),
            "key" => Key::parse_file_key(value).map(|key| Self::Key(KeyFilter::Is(key))),
            "genre" => Some(Self::Genre(value.to_lowercase())),
            "artist" => Some(Self::Artist(value.to_lowercase())),
            "energy" => number_bounds(value, MAX_ENERGY, 0.0).map(Self::Energy),
            "played" => match value.to_lowercase().as_str() {
                "yes" | "y" | "true" => Some(Self::Played(true)),
                "no" | "n" | "false" => Some(Self::Played(false)),
                _ => None,
            },
            "rated" if value.eq_ignore_ascii_case("none") || value == "0" => {
                Some(Self::Rated(RatedFilter::Unrated))
            }
            "rated" => number_bounds(value, RATING_LIMIT, 0)
                .map(|bounds| Self::Rated(RatedFilter::Stars(bounds))),
            "color" | "colour" if value.eq_ignore_ascii_case("none") => Some(Self::Color(None)),
            "color" | "colour" => colour_named(value).map(|colour| Self::Color(Some(colour))),
            "tag" => Some(Self::Tag(value.to_lowercase())),
            "comment" => Some(Self::Comment(value.to_lowercase())),
            _ => None,
        }
    }

    /// Whether `track` passes, with `fits` the key moves `key:fits` keeps, when there is a
    /// reference key. A track missing what the filter reads does not pass.
    pub fn keeps(&self, track: &TrackFacts<'_>, fits: Option<KeyFit>) -> bool {
        let metadata = &track.metadata;
        match self {
            Self::Bpm(bounds) => track.bpm.is_some_and(|bpm| bounds.keeps(bpm)),
            Self::Key(KeyFilter::Is(key)) => track.key.is_some_and(|found| found == *key),
            Self::Key(KeyFilter::Fits) => fits
                .zip(track.key)
                .is_some_and(|(fit, key)| fit.key_move(&key).is_some()),
            Self::Genre(text) => track.genre.to_lowercase().contains(text.as_str()),
            Self::Artist(text) => track.artist.to_lowercase().contains(text.as_str()),
            Self::Energy(bounds) => track.energy.is_some_and(|energy| bounds.keeps(energy)),
            Self::Played(played) => track.played == *played,
            Self::Rated(RatedFilter::Unrated) => metadata.rating == 0,
            Self::Rated(RatedFilter::Stars(bounds)) => bounds.keeps(metadata.rating),
            Self::Color(colour) => metadata.colour == *colour,
            Self::Tag(text) => metadata
                .tags
                .iter()
                .any(|name| name.to_lowercase() == text.as_str()),
            Self::Comment(text) => metadata.comment.to_lowercase().contains(text.as_str()),
        }
    }
}

/// The colour a colour name names, case-insensitive.
fn colour_named(name: &str) -> Option<TrackColour> {
    match name.to_lowercase().as_str() {
        "pink" => Some(TrackColour::Pink),
        "red" => Some(TrackColour::Red),
        "orange" => Some(TrackColour::Orange),
        "yellow" => Some(TrackColour::Yellow),
        "green" => Some(TrackColour::Green),
        "aqua" => Some(TrackColour::Aqua),
        "blue" => Some(TrackColour::Blue),
        "purple" => Some(TrackColour::Purple),
        _ => None,
    }
}

/// `6`, `6+` or `4-6` as inclusive bounds; a single number reaches `tolerance` either side.
fn number_bounds<Number>(text: &str, ceiling: Number, tolerance: Number) -> Option<Bounds<Number>>
where
    Number: std::str::FromStr + PartialOrd + Copy + std::ops::Add<Output = Number>,
    Number: std::ops::Sub<Output = Number>,
{
    if let Some(lowest) = text.strip_suffix('+') {
        let lowest = lowest.trim().parse().ok()?;
        return Some(Bounds {
            lowest,
            highest: ceiling,
        });
    }
    if let Some((lowest, highest)) = text.split_once('-') {
        let lowest: Number = lowest.trim().parse().ok()?;
        let highest: Number = highest.trim().parse().ok()?;
        return (lowest <= highest).then_some(Bounds { lowest, highest });
    }
    let exact: Number = text.parse().ok()?;
    Some(Bounds {
        lowest: exact - tolerance,
        highest: exact + tolerance,
    })
}

/// What the suggestions start from: the reference track's tempo and key, the genre of the
/// selected track, and the names of the DJ's My Tags.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SuggestionContext {
    /// The reference track's tempo.
    pub bpm: Option<f64>,
    /// The reference track's key.
    pub key: Option<Key>,
    /// The selected track's genre, its first word.
    pub genre: Option<String>,
    /// The names of the DJ's My Tags, from the set store.
    pub tag_names: Vec<String>,
}

/// The filters to offer for the last word typed in `text`, each a whole word to put in its
/// place: every filter while nothing is being typed, the names that start with a partial
/// word, and example values once a name and its colon are typed.
pub fn suggestions(text: &str, context: &SuggestionContext) -> Vec<String> {
    let partial = if text.ends_with(char::is_whitespace) {
        ""
    } else {
        text.split_whitespace().last().unwrap_or("")
    };
    let partial = partial.to_lowercase();
    let typed: Vec<String> = text
        .split_whitespace()
        .filter_map(|word| word.split_once(':').map(|(name, _)| name.to_lowercase()))
        .collect();
    NAMES
        .iter()
        .filter(|name| offered(name, &partial, &typed))
        .flat_map(|name| examples(name, context))
        .filter(|example| example.starts_with(&partial) && *example != partial)
        .collect()
}

/// Whether the examples of the filter `name` are offered while `partial` is being typed.
fn offered(name: &str, partial: &str, typed: &[String]) -> bool {
    if name == "color" {
        // The colour filter accepts both spellings, so either one being typed or typed keeps
        // the examples in or out.
        return partial.starts_with("color:")
            || partial.starts_with("colour:")
            || (!typed.contains(&"color".to_string()) && !typed.contains(&"colour".to_string()));
    }
    partial.starts_with(&format!("{name}:")) || !typed.contains(&name.to_string())
}

/// Ready-made values for one filter name.
fn examples(name: &str, context: &SuggestionContext) -> Vec<String> {
    match name {
        "bpm" => vec![context.bpm.map_or_else(
            || "bpm:120-124".to_owned(),
            |bpm| {
                format!(
                    "bpm:{:.0}-{:.0}",
                    (bpm - SUGGESTED_BPM_SPAN).floor(),
                    (bpm + SUGGESTED_BPM_SPAN).ceil()
                )
            },
        )],
        "key" => {
            let mut keys = vec!["key:fits".to_owned()];
            if let Some(key) = context.key {
                keys.push(format!("key:{}", key.camelot()));
            }
            keys
        }
        "genre" => vec![format!(
            "genre:{}",
            context.genre.as_deref().unwrap_or("amapiano")
        )],
        "energy" => vec!["energy:6+".to_owned()],
        "artist" => vec!["artist:".to_owned()],
        "played" => vec!["played:no".to_owned()],
        "rated" => vec!["rated:4+".to_owned(), "rated:0".to_owned()],
        "color" => COLOUR_NAMES
            .iter()
            .map(|name| format!("color:{name}"))
            .chain(std::iter::once("color:none".to_owned()))
            .collect(),
        "tag" => context
            .tag_names
            .iter()
            .map(|name| format!("tag:{name}"))
            .collect(),
        "comment" => vec!["comment:".to_owned()],
        _ => Vec::new(),
    }
}

/// The word Hide played puts in the search.
const HIDE_PLAYED: &str = "played:no";

/// `text` with `played:no` added at the end, or, when it already hides played tracks, with
/// every `played:` filter taken out; the other words stay as they were.
pub fn toggle_hide_played(text: &str) -> String {
    let hides = text
        .split_whitespace()
        .any(|word| SearchFilter::parse(word) == Some(SearchFilter::Played(false)));
    let mut kept: Vec<&str> = text
        .split_whitespace()
        .filter(|word| !matches!(SearchFilter::parse(word), Some(SearchFilter::Played(_))))
        .collect();
    if !hides {
        kept.push(HIDE_PLAYED);
    }
    kept.join(" ")
}

/// `text` with its last, partly typed word replaced by `word`, ready for more typing.
pub fn complete(text: &str, word: &str) -> String {
    let kept = if text.ends_with(char::is_whitespace) || text.is_empty() {
        text.trim_end()
    } else {
        text.trim_end()
            .rsplit_once(char::is_whitespace)
            .map_or("", |(before, _)| before)
    };
    let separator = if kept.is_empty() { "" } else { " " };
    let space_after = if word.ends_with(':') { "" } else { " " };
    format!("{kept}{separator}{word}{space_after}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::SearchQuery;

    fn metadata() -> TrackFilterMetadata {
        TrackFilterMetadata {
            rating: 4,
            colour: Some(TrackColour::Red),
            tags: vec![
                "peak".to_owned(),
                "warmup".to_owned(),
                "afrohouse".to_owned(),
            ],
            comment: "Club closer, keep the bass tight".to_owned(),
        }
    }

    fn track() -> TrackFacts<'static> {
        TrackFacts {
            artist: "Kabza De Small, DJ Maphorisa",
            genre: "amapiano / peak",
            bpm: Some(113.0),
            key: Key::parse_file_key("8A"),
            energy: Some(7.0),
            played: false,
            metadata: metadata(),
        }
    }

    fn keeps(word: &str, track: &TrackFacts<'_>) -> bool {
        let fits = Key::parse_file_key("9A").map(KeyFit::new);
        SearchFilter::parse(word).unwrap().keeps(track, fits)
    }

    #[test]
    fn hide_played_adds_played_no_and_takes_it_out_again() {
        assert_eq!(toggle_hide_played(""), "played:no");
        assert_eq!(
            toggle_hide_played("kabza bpm:112-114"),
            "kabza bpm:112-114 played:no"
        );
        assert_eq!(
            toggle_hide_played("kabza played:no bpm:112"),
            "kabza bpm:112"
        );
        assert_eq!(
            toggle_hide_played("played:yes kabza"),
            "kabza played:no",
            "played:yes gives way to played:no"
        );
        assert_eq!(
            toggle_hide_played("Played:N"),
            "",
            "any spelling of played:no"
        );
    }

    #[test]
    fn each_filter_reads_its_column() {
        let track = track();
        assert!(keeps("bpm:112-114", &track));
        assert!(keeps("bpm:113", &track));
        assert!(!keeps("bpm:120+", &track));
        assert!(keeps("key:8a", &track));
        assert!(keeps("key:Am", &track), "any key notation");
        assert!(
            keeps("key:fits", &track),
            "8A follows 9A one step on the wheel"
        );
        assert!(keeps("GENRE:Amapiano", &track));
        assert!(keeps("artist:maphorisa", &track));
        assert!(!keeps("artist:shimza", &track));
        assert!(keeps("energy:6+", &track));
        assert!(!keeps("energy:4-6", &track));
        assert!(keeps("played:no", &track));
        assert!(!keeps("played:yes", &track));
        assert!(keeps("rated:4+", &track));
        assert!(keeps("rated:4", &track));
        assert!(keeps("rated:3-5", &track));
        assert!(!keeps("rated:5", &track));
    }

    #[test]
    fn rated_zero_and_none_keep_only_the_unrated() {
        let track = track();
        assert!(!keeps("rated:0", &track));
        assert!(!keeps("rated:none", &track));
        let unrated = TrackFacts {
            metadata: TrackFilterMetadata::none(),
            ..track
        };
        assert!(keeps("rated:0", &unrated));
        assert!(keeps("RATED:NONE", &unrated));
        assert!(!keeps("rated:4+", &unrated));
        assert_eq!(
            SearchFilter::parse("rated:0"),
            Some(SearchFilter::Rated(RatedFilter::Unrated))
        );
        assert_eq!(
            SearchFilter::parse("rated:none"),
            Some(SearchFilter::Rated(RatedFilter::Unrated))
        );
    }

    #[test]
    fn color_keeps_its_label_and_none_keeps_the_unlabelled() {
        let track = track();
        assert!(keeps("color:red", &track));
        assert!(keeps("Colour:RED", &track));
        assert!(!keeps("color:blue", &track));
        assert!(!keeps("color:none", &track));
        let unlabelled = TrackFacts {
            metadata: TrackFilterMetadata::none(),
            ..track
        };
        assert!(keeps("color:none", &unlabelled));
        assert!(!keeps("color:red", &unlabelled));
        for name in COLOUR_NAMES {
            assert_eq!(
                SearchFilter::parse(&format!("colour:{name}")),
                SearchFilter::parse(&format!("color:{name}")),
                "colour:{name} spells the same filter"
            );
        }
        assert_eq!(
            SearchFilter::parse("color:none"),
            Some(SearchFilter::Color(None))
        );
        assert_eq!(SearchFilter::parse("color:gold"), None);
        assert!(names_a_filter("colour:"));
        assert!(names_a_filter("color:"));
    }

    #[test]
    fn tag_keeps_a_matching_tag_case_insensitively() {
        let track = track();
        assert!(keeps("tag:peak", &track));
        assert!(keeps("tag:PEAK", &track));
        assert!(keeps("tag:afrohouse", &track));
        assert!(!keeps("tag:peak time", &track));
        assert!(!keeps("tag:building", &track));
        assert!(
            !keeps(
                "tag:peak",
                &TrackFacts {
                    metadata: TrackFilterMetadata::none(),
                    ..track
                }
            ),
            "a track without the tag keeps nothing"
        );
    }

    #[test]
    fn several_tag_filters_must_all_match() {
        let track = track();
        let query = SearchQuery::parse("tag:peak tag:warmup");
        assert!(query.filters_keep(&track, None));
        let missing = TrackFacts {
            metadata: TrackFilterMetadata {
                tags: vec!["peak".to_owned()],
                ..metadata()
            },
            ..track
        };
        assert!(
            !query.filters_keep(&missing, None),
            "every tag: word must keep the track"
        );
    }

    #[test]
    fn comment_keeps_its_word_case_insensitively() {
        let track = track();
        assert!(keeps("comment:club", &track));
        assert!(keeps("COMMENT:Closer", &track));
        assert!(!keeps("comment:warehouse", &track));
        assert!(
            !keeps(
                "comment:club",
                &TrackFacts {
                    metadata: TrackFilterMetadata::none(),
                    ..track
                },
            ),
            "a track without a comment keeps nothing"
        );
    }

    #[test]
    fn half_typed_and_unknown_words_are_no_filters() {
        for word in [
            "bpm:",
            "bpm:fast",
            "key:xyz",
            "played:maybe",
            "feat:shimza",
            "plain",
            "color:",
            "color:gold",
            "rated:0.5",
            "tag:",
            "comment:",
        ] {
            assert_eq!(SearchFilter::parse(word), None, "{word}");
        }
        assert_eq!(
            SearchFilter::parse("bpm:124-120"),
            None,
            "the range runs upwards"
        );
    }

    #[test]
    fn names_a_filter_knows_the_spelling_of_every_filter() {
        for name in ["bpm", "key", "genre", "energy", "artist", "played", "rated"] {
            assert!(names_a_filter(&format!("{name}:x")));
        }
        for name in ["color", "colour", "tag", "comment"] {
            assert!(names_a_filter(&format!("{name}:x")), "{name}");
        }
        assert!(!names_a_filter("plain"));
    }

    #[test]
    fn suggestions_follow_what_is_typed() {
        let context = SuggestionContext {
            bpm: Some(113.4),
            key: Key::parse_file_key("8A"),
            genre: Some("amapiano".to_owned()),
            tag_names: vec!["peak".to_owned(), "warmup".to_owned()],
        };
        assert_eq!(suggestions("k", &context), ["key:fits", "key:8A"]);
        assert_eq!(suggestions("key:", &context), ["key:fits", "key:8A"]);
        assert!(
            !suggestions("key:fits ", &context)
                .iter()
                .any(|word| word.starts_with("key:")),
            "a filter already typed is not offered again"
        );
        assert!(suggestions("key:fits", &context).is_empty());
        assert_eq!(suggestions("color:re", &context), ["color:red"]);
        assert!(
            suggestions("color:", &context)
                .iter()
                .any(|word| word == "color:none")
        );
        assert!(
            !suggestions("color:red ", &context)
                .iter()
                .any(|word| word.starts_with("color:") || word.starts_with("colour:")),
            "a colour filter already typed is not offered again"
        );
        assert_eq!(suggestions("tag:", &context), ["tag:peak", "tag:warmup"]);
        assert_eq!(suggestions("tag:pe", &context), ["tag:peak"]);
        assert!(suggestions("tag:", &SuggestionContext::default()).is_empty());
        assert!(
            suggestions("rated:", &context)
                .iter()
                .any(|word| word == "rated:0")
        );
        assert!(
            suggestions("rated:", &context)
                .iter()
                .any(|word| word == "rated:4+")
        );
    }

    #[test]
    fn completing_replaces_the_word_being_typed() {
        assert_eq!(complete("kabza ke", "key:fits"), "kabza key:fits ");
        assert_eq!(complete("kabza ", "played:no"), "kabza played:no ");
        assert_eq!(complete("", "artist:"), "artist:");
    }
}
