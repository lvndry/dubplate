//! The menu a track row opens on a right-click or a Control-click, as a Mac app has it. Every
//! item is a command the keys and the menu bar already send, so it shows the same shortcut and
//! acts on the row just selected; items that do not apply to the track are dimmed.

use dubplate_engine::DeckId;
use gpui::FocusHandle;
use gpui::component::menu::PopupMenu;

use crate::commands::{Command, SetEdit};
use crate::shortcuts::Perform;

/// Where the row is: the library's tracks, or the set being prepared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowPlace {
    /// A row of the library.
    Library,
    /// A track of the set.
    Set,
}

/// What a track's menu needs to know about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowFacts {
    /// Where the row is.
    pub place: RowPlace,
    /// The track has an audio file where the library says.
    pub file_present: bool,
    /// The library knows a file for the track, found or not.
    pub has_path: bool,
    /// Its stems can be separated now: none yet, or the last try failed.
    pub can_separate: bool,
}

/// One entry of the menu: a command and whether it applies, or a separator.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MenuEntry {
    /// An item that sends `command`.
    Item {
        /// What it says.
        label: &'static str,
        /// What it does.
        command: Command,
        /// Whether it applies to the track.
        enabled: bool,
    },
    /// A line between groups.
    Separator,
}

/// The entries of the menu for a track with `facts`.
pub fn entries(facts: RowFacts) -> Vec<MenuEntry> {
    let item = |label, command, enabled| MenuEntry::Item {
        label,
        command,
        enabled,
    };
    let playable = facts.file_present;
    let mut entries = vec![
        item("Load on Deck A", Command::LoadSelected(DeckId::A), playable),
        item("Load on Deck B", Command::LoadSelected(DeckId::B), playable),
        item("Preview in Headphones", Command::PreviewSelected, playable),
        MenuEntry::Separator,
    ];
    match facts.place {
        RowPlace::Library => entries.extend([
            item("Add to Set", Command::EditSet(SetEdit::Add), facts.has_path),
            item(
                "Insert After Selection",
                Command::EditSet(SetEdit::InsertAfterSelection),
                facts.has_path,
            ),
        ]),
        RowPlace::Set => entries.push(item(
            "Remove from Set",
            Command::EditSet(SetEdit::Remove),
            true,
        )),
    }
    entries.push(MenuEntry::Separator);
    entries.push(item(
        "Separate Stems",
        Command::SeparateSelectedStems,
        playable && facts.can_separate,
    ));
    if facts.has_path && !facts.file_present {
        entries.push(item("Relink\u{2026}", Command::RelinkSelected, true));
    }
    entries.extend([
        MenuEntry::Separator,
        item("Show in Finder", Command::RevealSelected, playable),
        item("Copy File Path", Command::CopySelectedPath, facts.has_path),
    ]);
    entries
}

/// `menu` filled with the entries for `facts`, its commands sent from `keys`, the focus whose
/// shortcuts they show.
pub fn build(menu: PopupMenu, facts: RowFacts, keys: FocusHandle) -> PopupMenu {
    entries(facts)
        .into_iter()
        .fold(menu.action_context(keys), |menu, entry| match entry {
            MenuEntry::Item {
                label,
                command,
                enabled,
            } => menu.menu_with_disabled(label, Box::new(Perform(command)), !enabled),
            MenuEntry::Separator => menu.separator(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(facts: RowFacts) -> Vec<(&'static str, bool)> {
        entries(facts)
            .into_iter()
            .filter_map(|entry| match entry {
                MenuEntry::Item { label, enabled, .. } => Some((label, enabled)),
                MenuEntry::Separator => None,
            })
            .collect()
    }

    fn library_track() -> RowFacts {
        RowFacts {
            place: RowPlace::Library,
            file_present: true,
            has_path: true,
            can_separate: true,
        }
    }

    #[test]
    fn a_library_track_offers_everything_and_relink_only_when_missing() {
        let found = labels(library_track());
        assert_eq!(
            found.iter().map(|(label, _)| *label).collect::<Vec<_>>(),
            [
                "Load on Deck A",
                "Load on Deck B",
                "Preview in Headphones",
                "Add to Set",
                "Insert After Selection",
                "Separate Stems",
                "Show in Finder",
                "Copy File Path",
            ]
        );
        assert!(found.iter().all(|(_, enabled)| *enabled));

        let missing = labels(RowFacts {
            file_present: false,
            ..library_track()
        });
        let enabled = |label: &str| {
            missing
                .iter()
                .find(|(name, _)| *name == label)
                .map(|(_, enabled)| *enabled)
        };
        assert_eq!(enabled("Relink\u{2026}"), Some(true));
        assert_eq!(enabled("Load on Deck A"), Some(false), "dimmed, not hidden");
        assert_eq!(enabled("Show in Finder"), Some(false));
        assert_eq!(enabled("Copy File Path"), Some(true));
    }

    #[test]
    fn a_set_track_is_removed_from_the_set_instead_of_added() {
        let found = labels(RowFacts {
            place: RowPlace::Set,
            can_separate: false,
            ..library_track()
        });
        assert!(found.contains(&("Remove from Set", true)));
        assert!(!found.iter().any(|(label, _)| *label == "Add to Set"));
        assert!(found.contains(&("Separate Stems", false)));
    }
}
