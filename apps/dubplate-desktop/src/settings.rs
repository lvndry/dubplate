//! What Dubplate remembers between launches, in `settings.toml` next to the rekordbox key:
//! `~/Library/Application Support/dubplate/` on macOS and `~/.config/dubplate/` on Linux.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::effects::EffectUnitSetting;

const FILE_NAME: &str = "settings.toml";
const LIBRARY_CACHE_NAME: &str = "library-cache.json";
const SETS_NAME: &str = "sets.sqlite";

/// Why the settings could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    /// The system has no configuration directory for this user.
    #[error("there is no configuration directory for this user")]
    NoConfigurationDirectory,
    /// The file could not be read or written.
    #[error("{path}: {source}")]
    Io {
        /// The settings file.
        path: PathBuf,
        /// What went wrong.
        source: std::io::Error,
    },
    /// The file is not valid settings.
    #[error("{path} is not valid: {source}")]
    Parse {
        /// The settings file.
        path: PathBuf,
        /// What is wrong.
        source: toml::de::Error,
    },
    /// The settings could not be written as TOML.
    #[error("the settings could not be written: {0}")]
    Serialize(#[from] toml::ser::Error),
}

/// Everything Dubplate remembers. Settings missing from the file take their default, so a file
/// from an older version still loads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    /// Whether a playing deck refuses a new track.
    pub lock_playing_deck: bool,
    /// The sound card last used, by name.
    pub audio_device: Option<String>,
    /// The music folders that make up the library, in the order they were added.
    pub music_folders: Vec<PathBuf>,
    /// Whether rekordbox's collection is part of the library.
    pub import_rekordbox: bool,
    /// Each effect unit's effect, target, beat and depth, FX 1 first.
    pub effect_units: Vec<EffectUnitSetting>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            lock_playing_deck: true,
            audio_device: None,
            music_folders: Vec::new(),
            import_rekordbox: false,
            effect_units: Vec::new(),
        }
    }
}

impl Settings {
    /// Where the settings live for this user.
    pub fn default_path() -> Result<PathBuf, SettingsError> {
        Ok(configuration_directory()?.join(FILE_NAME))
    }

    /// Where the sets are kept.
    pub fn sets_path() -> Result<PathBuf, SettingsError> {
        Ok(configuration_directory()?.join(SETS_NAME))
    }

    /// Where the scans of the music folders are kept between launches.
    pub fn library_cache_path() -> Result<PathBuf, SettingsError> {
        Ok(configuration_directory()?.join(LIBRARY_CACHE_NAME))
    }

    /// Reads the settings at `path`. A missing file gives the defaults.
    pub fn load(path: &Path) -> Result<Self, SettingsError> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(source) => {
                return Err(SettingsError::Io {
                    path: path.to_path_buf(),
                    source,
                });
            }
        };
        toml::from_str(&text).map_err(|source| SettingsError::Parse {
            path: path.to_path_buf(),
            source,
        })
    }

    /// Writes the settings to `path`, through a temporary file so a crash never leaves half a
    /// file behind.
    pub fn save(&self, path: &Path) -> Result<(), SettingsError> {
        let text = toml::to_string_pretty(self)?;
        let io_error = |source| SettingsError::Io {
            path: path.to_path_buf(),
            source,
        };
        if let Some(directory) = path.parent() {
            std::fs::create_dir_all(directory).map_err(io_error)?;
        }
        let temporary = path.with_extension("toml.partial");
        std::fs::write(&temporary, text).map_err(io_error)?;
        std::fs::rename(&temporary, path).map_err(io_error)
    }
}

/// Dubplate's own folder in the user's configuration, never one of rekordbox's.
fn configuration_directory() -> Result<PathBuf, SettingsError> {
    let config = dirs::config_dir().ok_or(SettingsError::NoConfigurationDirectory)?;
    Ok(config.join("dubplate"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_survive_a_save_and_a_load() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("nested").join(FILE_NAME);
        let settings = Settings {
            lock_playing_deck: false,
            audio_device: Some("Studio Speakers".to_owned()),
            music_folders: vec![
                PathBuf::from("/Music/House"),
                PathBuf::from("/Music/Amapiano"),
            ],
            import_rekordbox: true,
            effect_units: vec![EffectUnitSetting {
                effect: "reverb".to_owned(),
                target: "M".to_owned(),
                beat: "2".to_owned(),
                depth: 0.25,
            }],
        };
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), settings);
        assert!(!path.with_extension("toml.partial").exists());
    }

    #[test]
    fn a_missing_file_or_setting_takes_the_default() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(FILE_NAME);
        assert_eq!(Settings::load(&path).unwrap(), Settings::default());
        std::fs::write(&path, "audio_device = \"Headphones\"\n").unwrap();
        let settings = Settings::load(&path).unwrap();
        assert!(settings.lock_playing_deck);
        assert_eq!(settings.audio_device.as_deref(), Some("Headphones"));
        assert!(settings.music_folders.is_empty());
        assert!(
            !settings.import_rekordbox,
            "rekordbox is only read when the DJ asks"
        );
    }

    #[test]
    fn a_broken_file_says_which_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(FILE_NAME);
        std::fs::write(&path, "lock_playing_deck = \"sometimes\"\n").unwrap();
        let error = Settings::load(&path).unwrap_err();
        assert!(error.to_string().contains(FILE_NAME), "{error}");
    }
}
