//! The fonts built into the app: Inter for the interface and Geist Mono for numbers, both under
//! the SIL Open Font License (see `assets/fonts`).

use std::borrow::Cow;

use gpui::App;

/// The interface font.
pub const UI_FAMILY: &str = "Inter";
/// The font of times, tempos, keys and counts.
pub const MONO_FAMILY: &str = "Geist Mono";

const FONT_FILES: [&[u8]; 6] = [
    include_bytes!("../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../assets/fonts/Inter-SemiBold.ttf"),
    include_bytes!("../assets/fonts/GeistMono-Regular.ttf"),
    include_bytes!("../assets/fonts/GeistMono-Medium.ttf"),
    include_bytes!("../assets/fonts/GeistMono-SemiBold.ttf"),
];

/// Makes the built-in fonts available to the text system.
pub fn register(cx: &App) -> anyhow::Result<()> {
    cx.text_system()
        .add_fonts(FONT_FILES.iter().map(|file| Cow::Borrowed(*file)).collect())
}
