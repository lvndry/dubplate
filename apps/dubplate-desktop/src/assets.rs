//! The images built into the app: the Dubplate mark and the icons the component library draws.

use std::borrow::Cow;

use gpui::assets::Assets as ComponentIcons;
use gpui::{AssetSource, Result, SharedString};

/// Path of the Dubplate mark.
pub const LOGO_PATH: &str = "logo/mark.svg";
/// Path of the magnifier in the search field.
pub const SEARCH_ICON_PATH: &str = "icons/search.svg";
/// Path of the play triangle.
pub const PLAY_ICON_PATH: &str = "icons/play.svg";
/// Path of the pause bars.
pub const PAUSE_ICON_PATH: &str = "icons/pause.svg";
/// Path of the small arrow that opens a menu.
pub const CHEVRON_DOWN_PATH: &str = "icons/chevron-down.svg";
/// Path of the arrow beside a column sorted A to Z.
pub const ARROW_UP_PATH: &str = "icons/arrow-up.svg";
/// Path of the arrow beside a column sorted Z to A.
pub const ARROW_DOWN_PATH: &str = "icons/arrow-down.svg";
/// Path of the cross that clears a filter.
pub const CLOSE_ICON_PATH: &str = "icons/close.svg";
/// Path of the tick beside a track played this session.
pub const PLAYED_ICON_PATH: &str = "icons/check.svg";

const LOGO: &[u8] = include_bytes!("../../../assets/logo/mark.svg");

/// Every image the app can draw.
pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path == LOGO_PATH {
            return Ok(Some(Cow::Borrowed(LOGO)));
        }
        ComponentIcons.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = ComponentIcons.list(path)?;
        if LOGO_PATH.starts_with(path) {
            paths.push(LOGO_PATH.into());
        }
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serves_the_mark_and_the_icons_the_views_use() {
        for path in [
            LOGO_PATH,
            SEARCH_ICON_PATH,
            PLAY_ICON_PATH,
            PAUSE_ICON_PATH,
            CHEVRON_DOWN_PATH,
            ARROW_UP_PATH,
            ARROW_DOWN_PATH,
            CLOSE_ICON_PATH,
            PLAYED_ICON_PATH,
        ] {
            let image = AppAssets.load(path).unwrap();
            assert!(image.is_some_and(|bytes| !bytes.is_empty()), "{path}");
        }
    }
}
