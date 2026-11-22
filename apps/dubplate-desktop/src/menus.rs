//! The menu bar, as a Mac app has it: every item goes through the same command layer as the
//! keys and the controller, shows its shortcut, and is dimmed when it does not apply.

use gpui::component::input::{Copy, Cut, Paste, Redo, SelectAll};
use gpui::{App, KeyBinding, Menu, MenuItem, OsAction, SystemMenuType};

use crate::commands::{Command, Mode, SetEdit};
use crate::shortcuts::{Perform, WINDOW_CONTEXT};

/// Where the guide is read.
const GUIDE_URL: &str = "https://github.com/lvndry/dubplate/blob/main/docs/guide.md";

gpui::actions!(
    dubplate,
    [
        /// Shows the About panel.
        About,
        /// Hides Dubplate.
        HideDubplate,
        /// Hides every other app.
        HideOthers,
        /// Shows every app again.
        ShowAll,
        /// Minimizes the window to the Dock.
        Minimize,
        /// Zooms the window to fill the screen, or back.
        Zoom,
        /// Enters or leaves full screen.
        ToggleFullScreen,
        /// Opens the guide in the browser.
        OpenGuide,
    ]
);

/// What the menus depend on, so they are rebuilt only when it changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuState {
    /// The screen on show.
    pub mode: Mode,
    /// Whether there is a set with tracks to export.
    pub has_set: bool,
    /// Whether the mix is being recorded.
    pub recording: bool,
    /// Whether the rekordbox collection is in the library.
    pub rekordbox_imported: bool,
    /// Whether some tracks' files are missing.
    pub missing_files: bool,
}

/// The menu bar for `state`.
pub fn menus(app_name: &'static str, state: MenuState) -> Vec<Menu> {
    let perform = |name: &'static str, command: Command| MenuItem::action(name, Perform(command));
    vec![
        Menu::new(app_name).items([
            MenuItem::action(format!("About {app_name}"), About),
            MenuItem::separator(),
            perform("Settings\u{2026}", Command::ToggleLibrary),
            MenuItem::separator(),
            MenuItem::os_submenu("Services", SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action(format!("Hide {app_name}"), HideDubplate),
            MenuItem::action("Hide Others", HideOthers),
            MenuItem::action("Show All", ShowAll),
            MenuItem::separator(),
            MenuItem::action(format!("Quit {app_name}"), crate::Quit),
        ]),
        Menu::new("File").items([
            perform("Add Music Folders\u{2026}", Command::AddMusicFolders),
            perform("Import rekordbox Collection", Command::ImportRekordbox)
                .disabled(state.rekordbox_imported),
            MenuItem::separator(),
            perform("Export Set to rekordbox\u{2026}", Command::ExportSet).disabled(!state.has_set),
            perform(
                if state.recording {
                    "Stop Recording"
                } else {
                    "Start Recording"
                },
                Command::ToggleRecording,
            ),
            MenuItem::separator(),
            perform("Relink Missing Files\u{2026}", Command::RelinkMissingFiles)
                .disabled(!state.missing_files),
        ]),
        Menu::new("Edit").items([
            MenuItem::os_action(
                "Undo",
                Perform(Command::EditSet(SetEdit::Undo)),
                OsAction::Undo,
            ),
            MenuItem::os_action("Redo", Redo, OsAction::Redo),
            MenuItem::separator(),
            MenuItem::os_action("Cut", Cut, OsAction::Cut),
            MenuItem::os_action("Copy", Copy, OsAction::Copy),
            MenuItem::os_action("Paste", Paste, OsAction::Paste),
            MenuItem::os_action("Select All", SelectAll, OsAction::SelectAll),
            MenuItem::separator(),
            perform("Find", Command::FocusSearch),
        ]),
        Menu::new("View").items([
            perform("Prepare", Command::SwitchMode(Mode::Prepare))
                .checked(state.mode == Mode::Prepare),
            perform("Perform", Command::SwitchMode(Mode::Perform))
                .checked(state.mode == Mode::Perform),
            MenuItem::separator(),
            MenuItem::action("Enter Full Screen", ToggleFullScreen),
        ]),
        Menu::new("Window").items([
            MenuItem::action("Minimize", Minimize),
            MenuItem::action("Zoom", Zoom),
        ]),
        Menu::new("Help").items([
            perform("Keyboard Shortcuts", Command::ToggleHelp),
            MenuItem::action(format!("{app_name} Guide"), OpenGuide),
        ]),
    ]
}

/// The Mac's own shortcuts for the items that are not Dubplate commands.
pub fn key_bindings() -> [KeyBinding; 4] {
    [
        KeyBinding::new("cmd-h", HideDubplate, None),
        KeyBinding::new("alt-cmd-h", HideOthers, None),
        KeyBinding::new("cmd-m", Minimize, Some(WINDOW_CONTEXT)),
        KeyBinding::new("ctrl-cmd-f", ToggleFullScreen, Some(WINDOW_CONTEXT)),
    ]
}

/// Handles the items that act on the app rather than the window.
pub fn register_app_actions(cx: &mut App) {
    cx.on_action(|_: &About, _| {
        #[cfg(target_os = "macos")]
        crate::macos_identity::show_about_panel();
    });
    cx.on_action(|_: &HideDubplate, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|_: &OpenGuide, cx| cx.open_url(GUIDE_URL));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> MenuState {
        MenuState {
            mode: Mode::Perform,
            has_set: false,
            recording: false,
            rekordbox_imported: true,
            missing_files: false,
        }
    }

    fn item<'menus>(menus: &'menus [Menu], menu: &str, name: &str) -> &'menus MenuItem {
        menus
            .iter()
            .find(|candidate| candidate.name.as_ref() == menu)
            .and_then(|menu| {
                menu.items.iter().find(|item| {
                    matches!(item, MenuItem::Action { name: item_name, .. } if item_name.as_ref() == name)
                })
            })
            .unwrap_or_else(|| panic!("{menu} > {name}"))
    }

    #[test]
    fn the_menu_bar_has_what_a_mac_app_has() {
        let menus = menus("Dubplate", state());
        let names: Vec<&str> = menus.iter().map(|menu| menu.name.as_ref()).collect();
        assert_eq!(
            names,
            ["Dubplate", "File", "Edit", "View", "Window", "Help"]
        );
        for (menu, name) in [
            ("Dubplate", "About Dubplate"),
            ("Dubplate", "Settings\u{2026}"),
            ("Dubplate", "Hide Dubplate"),
            ("Dubplate", "Quit Dubplate"),
            ("File", "Add Music Folders\u{2026}"),
            ("Edit", "Paste"),
            ("View", "Enter Full Screen"),
            ("Window", "Minimize"),
            ("Help", "Dubplate Guide"),
        ] {
            item(&menus, menu, name);
        }
    }

    #[test]
    fn items_that_do_not_apply_are_dimmed_and_the_screen_is_checked() {
        let menus = menus("Dubplate", state());
        assert!(item(&menus, "File", "Export Set to rekordbox\u{2026}").is_disabled());
        assert!(item(&menus, "File", "Import rekordbox Collection").is_disabled());
        assert!(item(&menus, "File", "Relink Missing Files\u{2026}").is_disabled());
        assert!(item(&menus, "View", "Perform").is_checked());
        assert!(!item(&menus, "View", "Prepare").is_checked());
        let recording = menus_with(MenuState {
            recording: true,
            has_set: true,
            ..state()
        });
        item(&recording, "File", "Stop Recording");
        assert!(!item(&recording, "File", "Export Set to rekordbox\u{2026}").is_disabled());
    }

    fn menus_with(state: MenuState) -> Vec<Menu> {
        menus("Dubplate", state)
    }
}
