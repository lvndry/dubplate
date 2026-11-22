//! How the app presents itself on macOS when its binary is started on its own, outside
//! Dubplate.app: the name "Dubplate" in the menu bar and the One groove icon in the Dock. The
//! bundle that `cargo run` and `scripts/bundle-macos.sh` make also names the Dock item. Also
//! keeps macOS from slowing the app down while another app is in front.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, ClassType, MainThreadMarker};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationOptions, NSApplicationActivationPolicy, NSImage,
    NSRunningApplication,
};
use objc2_foundation::{
    NSActivityOptions, NSBundle, NSData, NSMutableDictionary, NSObjectProtocol, NSProcessInfo,
    NSString,
};

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

/// Tells macOS that Dubplate answers the DJ in real time, so App Nap never slows its timers
/// and its controller handling while another app is in front. The Mac may still sleep when
/// idle. Lasts until the app quits.
pub fn stay_responsive() {
    let activity = NSProcessInfo::processInfo().beginActivityWithOptions_reason(
        NSActivityOptions::UserInitiatedAllowingIdleSystemSleep
            | NSActivityOptions::LatencyCritical,
        &NSString::from_str("Playing music from DJ controllers"),
    );
    // The activity ends when its token is released; the app keeps it until it quits.
    std::mem::forget(activity);
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

/// Keeps this Dubplate out of the Dock, the app switcher and the menu bar, and never in front,
/// for a copy running with offline devices beside the DJ's own. Does nothing off the main
/// thread.
pub fn stay_in_background() {
    let Some(main_thread) = MainThreadMarker::new() else {
        return;
    };
    NSApplication::sharedApplication(main_thread)
        .setActivationPolicy(NSApplicationActivationPolicy::Accessory);
}

/// Brings the Dubplate running as `process_id` to the front, for a second launch that quits.
pub fn bring_to_front(process_id: u32) {
    let Ok(process_id) = i32::try_from(process_id) else {
        return;
    };
    if let Some(running) = NSRunningApplication::runningApplicationWithProcessIdentifier(process_id)
    {
        running.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows);
    }
}

#[cfg(test)]
mod tests {
    use super::APP_ICON_PNG;

    #[test]
    fn the_icon_is_a_png_rendered_from_the_artwork() {
        assert_eq!(&APP_ICON_PNG[..8], b"\x89PNG\r\n\x1a\n");
    }
}
