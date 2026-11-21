//! How the app presents itself on macOS when it runs outside an app bundle, as `cargo run`
//! does: the name "Dubplate" in the menu bar and the Dock, and the One groove icon in the Dock.
//! The app bundle (`scripts/bundle-macos.sh`) has both in its `Info.plist` and icon.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, ClassType, MainThreadMarker};
use objc2_app_kit::{NSApplication, NSImage};
use objc2_foundation::{NSBundle, NSData, NSMutableDictionary, NSObjectProtocol, NSString};

/// The app icon, rendered from `assets/logo/app-icon.svg` by the build script.
const APP_ICON_PNG: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/app-icon.png"));

/// Gives the app `name` in the menu bar and the Dock. An executable outside a bundle has no
/// `Info.plist`, so macOS would show the file name; this writes the name into the bundle
/// information macOS reads when the application starts. Call it before the application starts.
pub fn set_app_name(name: &str) {
    let Some(information) = NSBundle::mainBundle().infoDictionary() else {
        return;
    };
    if !information.isKindOfClass(NSMutableDictionary::<NSString, AnyObject>::class()) {
        return;
    }
    // SAFETY: the dictionary was just checked to be mutable, and its keys are strings.
    let information: Retained<NSMutableDictionary<NSString, AnyObject>> =
        unsafe { Retained::cast_unchecked(information) };
    let value = NSString::from_str(name);
    for key in ["CFBundleName", "CFBundleDisplayName"] {
        information.insert(&*NSString::from_str(key), &value);
    }
}

/// Shows the One groove icon in the Dock. Does nothing off the main thread or when the image
/// cannot be read.
pub fn show_dock_icon() {
    let Some(main_thread) = MainThreadMarker::new() else {
        return;
    };
    let data = NSData::with_bytes(APP_ICON_PNG);
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
        return;
    };
    let application = NSApplication::sharedApplication(main_thread);
    // SAFETY: the image is a valid, retained `NSImage`, which is what the setter requires.
    unsafe { application.setApplicationIconImage(Some(&image)) };
}

#[cfg(test)]
mod tests {
    use super::APP_ICON_PNG;

    #[test]
    fn the_icon_is_a_png_rendered_from_the_artwork() {
        assert_eq!(&APP_ICON_PNG[..8], b"\x89PNG\r\n\x1a\n");
    }
}
