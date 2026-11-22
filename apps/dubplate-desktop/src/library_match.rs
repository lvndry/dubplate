//! How every library track would mix after the track the room follows: the tempo master's, or
//! the deck the room hears when no deck leads. The library's Match column shows it and sorts by
//! the order the Next suggestions use, so the library reads as a ranked list of what to play
//! next.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use dubplate_engine::DeckId;
use dubplate_prep::{EnergyDirection, PrepTrack, Relation, suggest_next};

use crate::deck::deck_letter;

/// A tempo change bigger than this, in percent, is a big jump for the pitch fader, so the
/// column marks it.
pub const FAR_TEMPO_PERCENT: f32 = 6.0;

/// How a track's key follows the reference's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyMatch {
    /// A move harmonic mixing names, such as one step on the wheel.
    Named(Relation),
    /// No named move: the keys clash.
    Clash,
}

/// How a key move sounds, which picks its colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyTone {
    /// The keys share most of their notes: the same key, one step, relative or diagonal.
    Smooth,
    /// A bold lift: an energy boost, a semitone boost or a change of mood.
    Lift,
    /// The keys clash.
    Clash,
}

impl KeyMatch {
    /// How the move sounds.
    pub fn tone(self) -> KeyTone {
        match self {
            Self::Named(
                Relation::Same | Relation::Adjacent | Relation::Relative | Relation::Diagonal,
            ) => KeyTone::Smooth,
            Self::Named(Relation::EnergyBoost | Relation::SemitoneBoost | Relation::MoodChange) => {
                KeyTone::Lift
            }
            Self::Clash => KeyTone::Clash,
        }
    }

    /// The move in Euphoria's words, such as "One step on the wheel" or "Key clash".
    pub fn label(self) -> &'static str {
        match self {
            Self::Named(relation) => relation.label(),
            Self::Clash => "Key clash",
        }
    }
}

/// How one track would mix after the reference.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackMatch {
    /// Where it comes in the Next suggestions' order, 1 for the best, when it mixes well
    /// enough to be suggested.
    pub rank: Option<usize>,
    /// How its key follows, when both keys are known.
    pub key: Option<KeyMatch>,
    /// How much faster it is than the reference plays now, in percent, when both tempos are
    /// known.
    pub tempo_change_percent: Option<f32>,
    /// Its energy minus the reference's, when both are rated.
    pub energy_change: Option<f32>,
}

impl TrackMatch {
    /// Whether the tempo change is a big jump for the pitch fader.
    pub fn is_far_in_tempo(&self) -> bool {
        self.tempo_change_percent
            .is_some_and(|percent| percent.abs() > FAR_TEMPO_PERCENT)
    }

    /// The match in words, for a screen reader and the cell's tooltip, such as
    /// "Suggestion 3: One step on the wheel, +1.6% tempo, energy +2".
    pub fn description(&self) -> String {
        let mut parts = Vec::new();
        parts.push(self.key.map_or("Key unknown", KeyMatch::label).to_owned());
        if let Some(percent) = self.tempo_change_percent {
            parts.push(format!("{percent:+.1}% tempo"));
        }
        if let Some(energy) = self.energy_change {
            parts.push(format!("energy {energy:+.0}"));
        }
        let facts = parts.join(", ");
        match self.rank {
            Some(rank) => format!("Suggestion {rank}: {facts}"),
            None => facts,
        }
    }
}

/// How the library mixes after one reference track, worked out once when the reference, its
/// tempo or the catalogue changes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LibraryMatches {
    reference: Option<PathBuf>,
    reference_title: String,
    heading: String,
    tracks: HashMap<PathBuf, TrackMatch>,
}

impl LibraryMatches {
    /// How every track of `catalogue` mixes after `reference`, ranked as the Next suggestions
    /// rank them with the energy held.
    pub fn new(deck: DeckId, reference: &PrepTrack, catalogue: &[PrepTrack]) -> Self {
        let ranks: HashMap<&Path, usize> = suggest_next(
            reference,
            catalogue,
            EnergyDirection::Same,
            |_| false,
            usize::MAX,
        )
        .into_iter()
        .enumerate()
        .map(|(position, suggestion)| (suggestion.track.track_path.as_path(), position + 1))
        .collect();
        let tracks = catalogue
            .iter()
            .filter(|candidate| candidate.track_path != reference.track_path)
            .map(|candidate| {
                let found = TrackMatch {
                    rank: ranks.get(candidate.track_path.as_path()).copied(),
                    key: reference.key.zip(candidate.key).map(|(from, to)| {
                        from.relation_to(&to)
                            .map_or(KeyMatch::Clash, KeyMatch::Named)
                    }),
                    tempo_change_percent: reference
                        .bpm
                        .zip(candidate.bpm)
                        .filter(|(from, _)| *from > 0.0)
                        .map(|(from, to)| (to / from - 1.0) * 100.0),
                    energy_change: reference
                        .energy
                        .zip(candidate.energy)
                        .map(|(from, to)| to - from),
                };
                (candidate.track_path.clone(), found)
            })
            .collect();
        Self {
            reference: Some(reference.track_path.clone()),
            reference_title: reference.title.clone(),
            heading: format!("vs {}: {}", deck_letter(deck), reference.title),
            tracks,
        }
    }

    /// How the track whose file is `path` mixes after the reference.
    pub fn get(&self, path: &Path) -> Option<&TrackMatch> {
        self.tracks.get(path)
    }

    /// Whether `path` is the reference track's own file.
    pub fn is_reference(&self, path: &Path) -> bool {
        self.reference.as_deref() == Some(path)
    }

    /// The Match column's heading, naming the deck and the track matched against, such as
    /// "vs A: Kukaya", when there is one.
    pub fn heading(&self) -> Option<&str> {
        self.reference.as_ref().map(|_| self.heading.as_str())
    }

    /// The title of the track matched against, when there is one.
    pub fn reference_title(&self) -> Option<&str> {
        self.reference
            .as_ref()
            .map(|_| self.reference_title.as_str())
    }
}

#[cfg(test)]
mod tests {
    use dubplate_prep::Key;

    use super::*;

    fn track(name: &str, key: &str, bpm: f32, energy: f32) -> PrepTrack {
        PrepTrack {
            track_path: PathBuf::from(format!("/{name}.mp3")),
            rekordbox_id: None,
            title: name.to_owned(),
            artist: String::new(),
            key: Key::parse_file_key(key),
            bpm: Some(bpm),
            energy: Some(energy),
        }
    }

    #[test]
    fn tracks_are_ranked_as_the_suggestions_and_every_one_is_described() {
        let playing = track("playing", "8A", 124.0, 6.0);
        let catalogue = [
            playing.clone(),
            track("clash", "3B", 124.0, 6.0),
            track("step", "9A", 126.0, 7.0),
            track("same", "8A", 124.0, 6.0),
            track("boost", "10A", 140.0, 8.0),
        ];
        let matches = LibraryMatches::new(DeckId::A, &playing, &catalogue);
        assert_eq!(matches.heading(), Some("vs A: playing"));
        let at = |name: &str| *matches.get(Path::new(&format!("/{name}.mp3"))).unwrap();
        assert!(matches.is_reference(Path::new("/playing.mp3")));
        assert!(matches.get(Path::new("/playing.mp3")).is_none());
        assert_eq!(at("same").rank, Some(1));
        assert_eq!(at("step").rank, Some(2));
        assert_eq!(at("clash").rank, None);
        assert_eq!(at("clash").key, Some(KeyMatch::Clash));
        assert_eq!(at("clash").key.unwrap().tone(), KeyTone::Clash);
        assert_eq!(at("step").key.unwrap().tone(), KeyTone::Smooth);
        let boost = at("boost");
        assert_eq!(boost.key.unwrap().tone(), KeyTone::Lift);
        assert!(boost.is_far_in_tempo());
        assert_eq!(boost.rank, None, "a big tempo jump is never suggested");
        assert_eq!(
            at("step").description(),
            "Suggestion 2: One step on the wheel, +1.6% tempo, energy +1"
        );
    }
}
