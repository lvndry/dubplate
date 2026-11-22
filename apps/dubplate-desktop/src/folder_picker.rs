//! Where the system's folder picker opens: in the DJ's music rather than in Documents. GPUI's
//! picker takes no start folder, so on macOS Dubplate sets the folder the open panel restores,
//! which macOS keeps per app under `NSNavLastRootDirectory`.

use std::path::{Path, PathBuf};

/// The folder a picker for music folders opens in: beside the music folder added last, which
/// is where more of the DJ's music usually sits, else the user's Music folder.
pub fn music_start_folder(music_folders: &[PathBuf]) -> Option<PathBuf> {
    music_folders
        .last()
        .and_then(|folder| folder.parent())
        .map(Path::to_path_buf)
        .or_else(dirs::audio_dir)
}

/// Makes the next folder picker open in `folder`.
#[cfg(target_os = "macos")]
pub fn open_next_picker_in(folder: &Path) {
    use objc2_foundation::{NSString, NSUserDefaults};

    let defaults = NSUserDefaults::standardUserDefaults();
    let value = NSString::from_str(&folder.to_string_lossy());
    let key = NSString::from_str("NSNavLastRootDirectory");
    // SAFETY: the value is an NSString, a property list object the defaults store accepts.
    unsafe { defaults.setObject_forKey(Some(&value), &key) };
}

/// Makes the next folder picker open in `folder`, where the platform's picker allows it.
#[cfg(not(target_os = "macos"))]
pub fn open_next_picker_in(_folder: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_picker_opens_beside_the_last_music_folder() {
        let folders = [
            PathBuf::from("/Users/dj/Music/House"),
            PathBuf::from("/Users/dj/Music/DJ tracks"),
        ];
        assert_eq!(
            music_start_folder(&folders),
            Some(PathBuf::from("/Users/dj/Music"))
        );
        assert_eq!(music_start_folder(&[]), dirs::audio_dir());
    }
}
