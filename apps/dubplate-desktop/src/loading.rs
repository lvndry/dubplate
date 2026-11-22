//! What can be loaded on a deck and when: every way of loading (drag and drop, double-click,
//! the Load buttons and the keys) asks these rules before anything changes on the deck.

use std::fmt;
use std::path::{Path, PathBuf};

use dubplate_engine::DeckId;

use crate::collection::{CollectionTrack, TrackIndex};
use crate::deck::deck_letter;

/// The file types the engine plays.
const PLAYABLE_EXTENSIONS: [&str; 9] = [
    "mp3", "m4a", "mp4", "aac", "flac", "wav", "aif", "aiff", "ogg",
];

/// What to load on a deck.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadSource {
    /// A track of the rekordbox collection.
    Collection(TrackIndex),
    /// An audio file, such as one dropped from Finder.
    File(PathBuf),
}

/// Why a load was refused. The deck stays as it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadRefusal {
    /// The deck is playing and playing decks are locked.
    DeckPlaying(DeckId),
    /// The file is not a kind of audio the engine plays.
    NotAudio(String),
    /// The rekordbox library has not been read.
    LibraryNotReady,
    /// The track's audio file is not where the library says, named by the track's title.
    FileMissing(String),
}

impl fmt::Display for LoadRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeckPlaying(deck) => write!(
                formatter,
                "Deck {} is playing. Pause it or turn off Lock playing deck to load.",
                deck_letter(*deck)
            ),
            Self::NotAudio(name) => write!(
                formatter,
                "{name} is not an audio file Dubplate plays (MP3, M4A, AAC, FLAC, WAV, AIFF or Ogg)."
            ),
            Self::LibraryNotReady => formatter.write_str("The rekordbox library is not read yet."),
            Self::FileMissing(title) => {
                write!(formatter, "{title} was not loaded: its file is missing.")
            }
        }
    }
}

/// Whether the engine plays files with this name's extension.
pub fn is_playable_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            PLAYABLE_EXTENSIONS
                .iter()
                .any(|playable| playable.eq_ignore_ascii_case(extension))
        })
}

/// Checks that `path` can be loaded.
pub fn check_file(path: &Path) -> Result<(), LoadRefusal> {
    if is_playable_file(path) {
        Ok(())
    } else {
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        Err(LoadRefusal::NotAudio(name))
    }
}

/// Checks that a library track's audio file is still there, so a deck never takes a track it
/// cannot play.
pub fn check_present(track: &CollectionTrack) -> Result<(), LoadRefusal> {
    if track.track.file_path.as_deref().is_some_and(Path::is_file) {
        Ok(())
    } else {
        Err(LoadRefusal::FileMissing(track.row.title.to_string()))
    }
}

/// The lock rule of CDJs and rekordbox: with the lock on, a playing deck keeps its track.
pub fn check_deck_free(
    deck: DeckId,
    deck_playing: bool,
    lock_playing_deck: bool,
) -> Result<(), LoadRefusal> {
    if deck_playing && lock_playing_deck {
        Err(LoadRefusal::DeckPlaying(deck))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_files_are_accepted_in_any_case() {
        for name in [
            "a.mp3", "a.M4A", "a.flac", "a.wav", "a.aiff", "a.AIF", "a.ogg", "a.aac", "a.mp4",
        ] {
            assert!(is_playable_file(Path::new(name)), "{name}");
        }
    }

    #[test]
    fn other_files_are_refused_by_name() {
        for name in [
            "notes.txt",
            "cover.jpg",
            "set.m3u8",
            "folder",
            "track.mp3.zip",
        ] {
            assert!(!is_playable_file(Path::new(name)), "{name}");
        }
        let refusal = check_file(Path::new("/Users/dj/cover.jpg")).unwrap_err();
        assert_eq!(refusal, LoadRefusal::NotAudio("cover.jpg".to_owned()));
        assert!(
            refusal
                .to_string()
                .starts_with("cover.jpg is not an audio file")
        );
    }

    #[test]
    fn a_track_whose_file_is_gone_is_refused_by_title() {
        let directory = tempfile::tempdir().unwrap();
        let present = directory.path().join("Uthando.m4a");
        std::fs::write(&present, b"audio").unwrap();
        assert_eq!(check_present(&CollectionTrack::from_file(&present)), Ok(()));
        let gone = CollectionTrack::from_file(&directory.path().join("Abalele.m4a"));
        let refusal = check_present(&gone).unwrap_err();
        assert_eq!(refusal, LoadRefusal::FileMissing("Abalele".to_owned()));
        assert_eq!(
            refusal.to_string(),
            "Abalele was not loaded: its file is missing."
        );
    }

    #[test]
    fn a_locked_playing_deck_keeps_its_track() {
        assert_eq!(
            check_deck_free(DeckId::A, true, true),
            Err(LoadRefusal::DeckPlaying(DeckId::A))
        );
        assert_eq!(
            LoadRefusal::DeckPlaying(DeckId::A).to_string(),
            "Deck A is playing. Pause it or turn off Lock playing deck to load."
        );
    }

    #[test]
    fn a_stopped_deck_or_an_unlocked_one_takes_a_track() {
        assert_eq!(check_deck_free(DeckId::B, false, true), Ok(()));
        assert_eq!(check_deck_free(DeckId::B, true, false), Ok(()));
        assert_eq!(check_deck_free(DeckId::B, false, false), Ok(()));
    }
}
