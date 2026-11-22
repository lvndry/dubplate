//! Quick filters typed in the search field as `name:value`, such as `bpm:120-124`, `key:8A`,
//! `key:fits`, `genre:amapiano`, `energy:6+`, `artist:kabza`, `played:no` and `rated:4+`,
//! mixed with the free words; and the suggestions that teach them as the DJ types.

use dubplate_prep::{Key, KeyFit};

/// The filter names, in the order the suggestions offer them.
const NAMES: [&str; 7] = ["bpm", "key", "genre", "energy", "artist", "played", "rated"];
/// How far either side of a single tempo, such as `bpm:120`, still matches.
const BPM_TOLERANCE: f64 = 0.5;
/// How far either side of the reference tempo a suggested BPM range reaches.
const SUGGESTED_BPM_SPAN: f64 = 2.0;
const MAX_ENERGY: f32 = 10.0;
const RATING_LIMIT: u8 = 5;

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
    /// `played:yes` or `played:no`: whether a deck played the track this session.
    Played(bool),
    /// `rated:4` or `rated:4+`: rekordbox's stars.
    Rated(Bounds<u8>),
}

/// What the filters read of one track.
#[derive(Debug, Clone, Copy, PartialEq)]
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
    /// rekordbox's stars.
    pub rating: Option<u8>,
}

/// Whether `word` names a filter, typed in full or not, so a half-typed one is left out of the
/// free words instead of matching nothing.
pub fn names_a_filter(word: &str) -> bool {
    word.split_once(':')
        .is_some_and(|(name, _)| NAMES.contains(&name.to_lowercase().as_str()))
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
        match name.to_lowercase().as_str() {
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
            "rated" => number_bounds(value, RATING_LIMIT, 0).map(Self::Rated),
            _ => None,
        }
    }

    /// Whether `track` passes, with `fits` the key moves `key:fits` keeps, when there is a
    /// reference key. A track missing what the filter reads does not pass.
    pub fn keeps(&self, track: &TrackFacts<'_>, fits: Option<KeyFit>) -> bool {
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
            Self::Rated(bounds) => track.rating.is_some_and(|stars| bounds.keeps(stars)),
        }
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

/// What the suggestions start from: the reference track's tempo and key, and the genre of the
/// selected track.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SuggestionContext {
    /// The reference track's tempo.
    pub bpm: Option<f64>,
    /// The reference track's key.
    pub key: Option<Key>,
    /// The selected track's genre, its first word.
    pub genre: Option<String>,
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
        .filter(|name| {
            partial.starts_with(&format!("{name}:")) || !typed.contains(&name.to_string())
        })
        .flat_map(|name| examples(name, context))
        .filter(|example| example.starts_with(&partial) && *example != partial)
        .collect()
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
        "rated" => vec!["rated:4+".to_owned()],
        _ => Vec::new(),
    }
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

    fn facts() -> TrackFacts<'static> {
        TrackFacts {
            artist: "Kabza De Small, DJ Maphorisa",
            genre: "amapiano / peak",
            bpm: Some(113.0),
            key: Key::parse_file_key("8A"),
            energy: Some(7.0),
            played: false,
            rating: Some(4),
        }
    }

    fn keeps(word: &str) -> bool {
        let fits = Key::parse_file_key("9A").map(KeyFit::new);
        SearchFilter::parse(word).unwrap().keeps(&facts(), fits)
    }

    #[test]
    fn each_filter_reads_its_column() {
        assert!(keeps("bpm:112-114"));
        assert!(keeps("bpm:113"));
        assert!(!keeps("bpm:120+"));
        assert!(keeps("key:8a"));
        assert!(keeps("key:Am"), "any key notation");
        assert!(keeps("key:fits"), "8A follows 9A one step on the wheel");
        assert!(keeps("GENRE:Amapiano"));
        assert!(keeps("artist:maphorisa"));
        assert!(!keeps("artist:shimza"));
        assert!(keeps("energy:6+"));
        assert!(!keeps("energy:4-6"));
        assert!(keeps("played:no"));
        assert!(!keeps("played:yes"));
        assert!(keeps("rated:4+"));
        assert!(!keeps("rated:5"));
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
    fn suggestions_follow_what_is_typed() {
        let context = SuggestionContext {
            bpm: Some(113.4),
            key: Key::parse_file_key("8A"),
            genre: Some("amapiano".to_owned()),
        };
        assert_eq!(
            suggestions("", &context),
            [
                "bpm:111-116",
                "key:fits",
                "key:8A",
                "genre:amapiano",
                "energy:6+",
                "artist:",
                "played:no",
                "rated:4+"
            ]
        );
        assert_eq!(suggestions("kabza k", &context), ["key:fits", "key:8A"]);
        assert_eq!(suggestions("key:", &context), ["key:fits", "key:8A"]);
        assert!(
            !suggestions("key:fits ", &context)
                .iter()
                .any(|word| word.starts_with("key:")),
            "a filter already typed is not offered again"
        );
        assert!(suggestions("key:fits", &context).is_empty());
    }

    #[test]
    fn completing_replaces_the_word_being_typed() {
        assert_eq!(complete("kabza ke", "key:fits"), "kabza key:fits ");
        assert_eq!(complete("kabza ", "played:no"), "kabza played:no ");
        assert_eq!(complete("", "artist:"), "artist:");
    }
}
