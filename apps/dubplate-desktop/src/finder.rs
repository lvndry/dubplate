//! Showing a track's file where it lives, as Finder's Show in Enclosing Folder does.

use std::path::Path;

/// Opens a Finder window on the folder holding `path`, with the file selected.
#[cfg(target_os = "macos")]
pub fn reveal(path: &Path) -> Result<(), String> {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSArray, NSString, NSURL};

    let path = path
        .to_str()
        .ok_or_else(|| format!("{} is not a path Finder can open", path.display()))?;
    let url = NSURL::fileURLWithPath(&NSString::from_str(path));
    NSWorkspace::sharedWorkspace()
        .activateFileViewerSelectingURLs(&NSArray::from_retained_slice(&[url]));
    Ok(())
}

/// Opens the folder holding `path` in the desktop's file manager.
#[cfg(not(target_os = "macos"))]
pub fn reveal(path: &Path) -> Result<(), String> {
    let folder = path.parent().unwrap_or(path).to_path_buf();
    std::thread::spawn(move || std::process::Command::new("xdg-open").arg(folder).status());
    Ok(())
}
