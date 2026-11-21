//! Where the rekordbox library and its key live on this computer.

use std::path::{Path, PathBuf};

use crate::error::Error;
use crate::library::DatabaseKey;

/// The environment variable that holds the library key, ahead of the key file.
pub const KEY_ENVIRONMENT_VARIABLE: &str = "DUBPLATE_REKORDBOX_KEY";

/// The file that holds the library key: `~/Library/Application Support/dubplate/rekordbox.key`
/// on macOS and `~/.config/dubplate/rekordbox.key` on Linux.
pub fn key_file() -> Result<PathBuf, Error> {
    let config = dirs::config_dir().ok_or(Error::NoConfigurationDirectory)?;
    Ok(config.join("dubplate").join("rekordbox.key"))
}

/// rekordbox's `master.db` for the current user, where rekordbox keeps it.
pub fn default_master_db() -> Result<PathBuf, Error> {
    let base = if cfg!(target_os = "macos") {
        dirs::home_dir()
            .ok_or(Error::NoConfigurationDirectory)?
            .join("Library")
    } else {
        dirs::config_dir().ok_or(Error::NoConfigurationDirectory)?
    };
    Ok(base.join("Pioneer").join("rekordbox").join("master.db"))
}

impl DatabaseKey {
    /// Reads the key from [`KEY_ENVIRONMENT_VARIABLE`] or, when that is unset or empty, from
    /// [`key_file`].
    pub fn from_user_configuration() -> Result<Self, Error> {
        key_from(std::env::var(KEY_ENVIRONMENT_VARIABLE).ok(), &key_file()?)
    }
}

fn key_from(environment_value: Option<String>, key_file: &Path) -> Result<DatabaseKey, Error> {
    if let Some(key) = environment_value.filter(|key| !key.trim().is_empty()) {
        return Ok(DatabaseKey::new(key));
    }
    match std::fs::read_to_string(key_file) {
        Ok(key) if !key.trim().is_empty() => Ok(DatabaseKey::new(key)),
        Ok(_) => Err(Error::EmptyKey(key_file.to_path_buf())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(Error::NoKey {
            key_file: key_file.to_path_buf(),
        }),
        Err(source) => Err(Error::KeyRead {
            path: key_file.to_path_buf(),
            source,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_environment_wins_over_the_key_file() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("rekordbox.key");
        std::fs::write(&file, "from-file").unwrap();
        let key = key_from(Some(" from-environment \n".to_owned()), &file).unwrap();
        assert_eq!(key.secret(), "from-environment");
        let key = key_from(Some("  ".to_owned()), &file).unwrap();
        assert_eq!(key.secret(), "from-file");
    }

    #[test]
    fn a_missing_or_empty_key_file_says_where_the_key_goes() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("rekordbox.key");
        let missing = key_from(None, &file).unwrap_err();
        assert!(matches!(missing, Error::NoKey { ref key_file } if key_file == &file));
        assert!(missing.to_string().contains(KEY_ENVIRONMENT_VARIABLE));
        std::fs::write(&file, "\n").unwrap();
        assert!(matches!(key_from(None, &file), Err(Error::EmptyKey(_))));
    }
}
