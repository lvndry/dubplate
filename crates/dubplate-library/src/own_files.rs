//! What every file Dubplate writes for itself has in common: it never goes inside one of
//! rekordbox's folders, and the times it records are Unix milliseconds.

use std::path::{Component, Path};
use std::time::{SystemTime, UNIX_EPOCH};

/// Folder names that mark a folder as rekordbox's, in any case.
const REKORDBOX_FOLDER_NAMES: [&str; 2] = ["Pioneer", "rekordbox"];

/// Whether any folder on `path` is one of rekordbox's, where Dubplate never writes.
pub fn is_inside_rekordbox(path: &Path) -> bool {
    path.components().any(|component| {
        matches!(
            component,
            Component::Normal(name)
                if REKORDBOX_FOLDER_NAMES
                    .iter()
                    .any(|known| name.eq_ignore_ascii_case(known))
        )
    })
}

/// `time` in milliseconds since 1970, 0 for a time before it.
pub fn unix_millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map(|since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn any_rekordbox_folder_on_the_path_counts_whatever_its_case() {
        assert!(is_inside_rekordbox(Path::new(
            "/Users/dj/Library/Pioneer/rekordbox/sets.db"
        )));
        assert!(is_inside_rekordbox(Path::new("/Volumes/USB/PIONEER/x.xml")));
        assert!(is_inside_rekordbox(Path::new(
            "relative/REKORDBOX/cache.json"
        )));
        assert!(!is_inside_rekordbox(Path::new(
            "/Users/dj/Library/Application Support/Dubplate/sets.db"
        )));
        assert!(!is_inside_rekordbox(Path::new("/Music/pioneer-dj-mix.mp3")));
    }

    #[test]
    fn times_count_milliseconds_from_1970_and_never_go_below_zero() {
        assert_eq!(
            unix_millis(UNIX_EPOCH + Duration::from_millis(1_234)),
            1_234
        );
        assert_eq!(unix_millis(UNIX_EPOCH - Duration::from_secs(5)), 0);
    }
}
