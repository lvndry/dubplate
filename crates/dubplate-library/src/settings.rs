//! Where Dubplate keeps its own files, and the part of its settings that says what the library
//! is made of: the music folders, whether rekordbox's collection is in it, and the files the DJ
//! relinked.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const SETTINGS_NAME: &str = "settings.toml";
const LIBRARY_CACHE_NAME: &str = "library-cache.json";
const SETS_NAME: &str = "sets.sqlite";
const ENERGY_CACHE_NAME: &str = "energy.sqlite";

/// Dubplate's own folder in the user's configuration, never one of rekordbox's:
/// `~/Library/Application Support/dubplate/` on macOS and `~/.config/dubplate/` on Linux.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataFolder {
    root: PathBuf,
}

impl DataFolder {
    /// The folder for the current user, or `None` when the system has no configuration
    /// directory for them.
    pub fn for_user() -> Option<Self> {
        dirs::config_dir().map(|config| Self::at(config.join("dubplate")))
    }

    /// A data folder at `root`.
    pub fn at(root: PathBuf) -> Self {
        Self { root }
    }

    /// The settings file.
    pub fn settings(&self) -> PathBuf {
        self.root.join(SETTINGS_NAME)
    }

    /// The scans of the music folders, kept between launches.
    pub fn library_cache(&self) -> PathBuf {
        self.root.join(LIBRARY_CACHE_NAME)
    }

    /// The set database.
    pub fn sets(&self) -> PathBuf {
        self.root.join(SETS_NAME)
    }

    /// The tracks' energy ratings, kept between launches.
    pub fn energy_cache(&self) -> PathBuf {
        self.root.join(ENERGY_CACHE_NAME)
    }
}

/// A rekordbox track's file, found at a new place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Relink {
    /// Where rekordbox says the file is.
    pub from: PathBuf,
    /// Where it is now.
    pub to: PathBuf,
}

/// What the library is made of, as the settings file keeps it. The app's settings hold these
/// beside everything else; readers that only need the library load just this part.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LibrarySettings {
    /// The music folders that make up the library, in the order they were added.
    pub music_folders: Vec<PathBuf>,
    /// Whether rekordbox's collection is part of the library.
    pub import_rekordbox: bool,
    /// The new places of rekordbox tracks whose files moved, which the library plays from
    /// instead of where rekordbox says. rekordbox itself is never written.
    pub relinks: Vec<Relink>,
}

/// Why the library settings could not be read.
#[derive(Debug, thiserror::Error)]
pub enum LibrarySettingsError {
    /// The file could not be read.
    #[error("{path}: {source}")]
    Io {
        /// The settings file.
        path: PathBuf,
        /// What went wrong.
        source: std::io::Error,
    },
    /// The file is not valid TOML or holds a library setting of the wrong shape.
    #[error("{path} is not valid: {source}")]
    Parse {
        /// The settings file.
        path: PathBuf,
        /// What is wrong.
        source: Box<toml::de::Error>,
    },
}

impl LibrarySettings {
    /// Reads the library part of the settings file at `path`, leaving the file as it is. A
    /// missing file gives the defaults.
    pub fn load(path: &Path) -> Result<Self, LibrarySettingsError> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(source) => {
                return Err(LibrarySettingsError::Io {
                    path: path.to_path_buf(),
                    source,
                });
            }
        };
        toml::from_str(&text).map_err(|source| LibrarySettingsError::Parse {
            path: path.to_path_buf(),
            source: Box::new(source),
        })
    }

    /// The relinks as a map from the path rekordbox gave to the file's new place.
    pub fn relink_map(&self) -> HashMap<PathBuf, PathBuf> {
        self.relinks
            .iter()
            .map(|relink| (relink.from.clone(), relink.to.clone()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_library_part_reads_from_a_whole_settings_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(SETTINGS_NAME);
        std::fs::write(
            &path,
            "lock_playing_deck = false\nmusic_folders = [\"/Music/House\"]\n\
             import_rekordbox = true\n\n[sampler]\nvolume = 0.5\n\n\
             [[relinks]]\nfrom = \"/Old/Track.m4a\"\nto = \"/New/Track.m4a\"\n",
        )
        .unwrap();
        let settings = LibrarySettings::load(&path).unwrap();
        assert_eq!(settings.music_folders, [PathBuf::from("/Music/House")]);
        assert!(settings.import_rekordbox);
        assert_eq!(
            settings.relink_map().get(Path::new("/Old/Track.m4a")),
            Some(&PathBuf::from("/New/Track.m4a"))
        );
    }

    #[test]
    fn a_missing_file_is_the_defaults_and_a_broken_one_says_which_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(SETTINGS_NAME);
        assert_eq!(
            LibrarySettings::load(&path).unwrap(),
            LibrarySettings::default()
        );
        std::fs::write(&path, "import_rekordbox = \"sometimes\"\n").unwrap();
        let error = LibrarySettings::load(&path).unwrap_err();
        assert!(error.to_string().contains(SETTINGS_NAME), "{error}");
    }

    #[test]
    fn every_file_lives_in_the_data_folder() {
        let folder = DataFolder::at(PathBuf::from("/Config/dubplate"));
        for path in [
            folder.settings(),
            folder.library_cache(),
            folder.sets(),
            folder.energy_cache(),
        ] {
            assert_eq!(path.parent(), Some(Path::new("/Config/dubplate")));
        }
    }
}
