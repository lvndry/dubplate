//! The menu a track row opens on a right-click or a Control-click, as a Mac app has it. Every
//! item is a command the keys and the menu bar already send, so it shows the same shortcut and
//! acts on the row just selected; items that do not apply to the track are dimmed.

use std::sync::Arc;

use dubplate_engine::DeckId;
use dubplate_prep::{TagCategory, TagId, TrackColour};
use gpui::component::menu::PopupMenu;
use gpui::prelude::*;
use gpui::{Context, FocusHandle, Pixels, Window, div, px};

use crate::commands::{Command, SetEdit};
use crate::shortcuts::Perform;
use crate::theme;
use crate::track_metadata::{
    MAX_RATING, StoredTrackMetadata, TRACK_COLOURS, TrackMetadataCache, colour_name, rating_stars,
};

/// The tallest the menu and its My Tag list grow, so they scroll rather than leave the screen.
const MENU_MAX_HEIGHT: Pixels = px(560.0);
/// Room kept between a menu and the edges of the window.
const MENU_WINDOW_MARGIN: Pixels = px(64.0);
/// The submenus come after the group that loads and previews, and the group that adds to the
/// set: that is, after the second separator.
const SEPARATORS_BEFORE_TAGS: usize = 2;
/// The side of a colour swatch in the menu.
const SWATCH_SIZE: Pixels = px(12.0);

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

/// One choice of a submenu: a command, ticked when it is what the track has now.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    /// What it says.
    pub label: String,
    /// What it does.
    pub command: Command,
    /// Whether the track has it now.
    pub checked: bool,
    /// The colour swatch beside the label, for the colour choices.
    pub swatch: Option<TrackColour>,
}

/// The Rating submenu: no stars, then one to five, the track's own ticked.
pub fn rating_choices(current: u8) -> Vec<Choice> {
    (0..=MAX_RATING)
        .map(|stars| Choice {
            label: match stars {
                0 => "No Rating".to_owned(),
                1 => format!("{}  1 star", rating_stars(stars)),
                _ => format!("{}  {stars} stars", rating_stars(stars)),
            },
            command: Command::RateSelected(stars),
            checked: stars == current,
            swatch: None,
        })
        .collect()
}

/// The Colour submenu: none, then the eight colours in rekordbox's order, the track's own
/// ticked.
pub fn colour_choices(current: Option<TrackColour>) -> Vec<Choice> {
    std::iter::once(None)
        .chain(TRACK_COLOURS.map(Some))
        .map(|colour| Choice {
            label: colour.map_or("None", colour_name).to_owned(),
            command: Command::ColourSelected(colour),
            checked: colour == current,
            swatch: colour,
        })
        .collect()
}

/// A My Tag category and its tags as menu choices.
#[derive(Debug, Clone, PartialEq)]
pub struct TagGroup {
    /// The category's name.
    pub category: String,
    /// Its tags, each ticked when the track carries it.
    pub tags: Vec<Choice>,
}

/// The My Tag submenu: each category with at least one tag, and its tags as toggles.
pub fn tag_groups(categories: &[TagCategory], carried: &[TagId]) -> Vec<TagGroup> {
    categories
        .iter()
        .filter(|category| !category.tags.is_empty())
        .map(|category| TagGroup {
            category: category.name.clone(),
            tags: category
                .tags
                .iter()
                .map(|tag| Choice {
                    label: tag.name.clone(),
                    command: Command::ToggleSelectedTag(tag.id),
                    checked: carried.contains(&tag.id),
                    swatch: None,
                })
                .collect(),
        })
        .collect()
}

/// What a track has stored, for the Rating, Colour and My Tag submenus to tick.
#[derive(Clone, Default)]
pub struct TrackTags {
    stored: Option<Arc<StoredTrackMetadata>>,
    cache: Arc<TrackMetadataCache>,
}

impl TrackTags {
    /// What `cache` holds for a track.
    pub fn new(stored: Option<Arc<StoredTrackMetadata>>, cache: Arc<TrackMetadataCache>) -> Self {
        Self { stored, cache }
    }
}

/// The height a menu may reach in `window`: all it needs up to a limit, and never more than
/// the window leaves room for.
fn menu_height(window: &Window) -> Pixels {
    (window.viewport_size().height - MENU_WINDOW_MARGIN)
        .max(px(160.0))
        .min(MENU_MAX_HEIGHT)
}

fn add_choice(menu: PopupMenu, choice: &Choice) -> PopupMenu {
    let action = Box::new(Perform(choice.command));
    match choice.swatch {
        Some(colour) => {
            let label = choice.label.clone();
            menu.menu_element_with_check(choice.checked, action, move |_, _| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .size(SWATCH_SIZE)
                            .flex_none()
                            .rounded(px(3.0))
                            .bg(theme::active().track_colour(colour)),
                    )
                    .child(label.clone())
            })
        }
        None => menu.menu_with_check(choice.label.clone(), choice.checked, action),
    }
}

fn add_choices(menu: PopupMenu, choices: &[Choice]) -> PopupMenu {
    choices.iter().fold(menu, add_choice)
}

fn add_tag_groups(menu: PopupMenu, groups: &[TagGroup]) -> PopupMenu {
    let menu = if groups.is_empty() {
        menu.label("No My Tags yet")
    } else {
        groups
            .iter()
            .enumerate()
            .fold(menu, |menu, (position, group)| {
                let menu = if position == 0 {
                    menu
                } else {
                    menu.separator()
                };
                add_choices(menu.label(group.category.clone()), &group.tags)
            })
    };
    menu.separator().menu(
        "Edit My Tags\u{2026}",
        Box::new(Perform(Command::OpenTagManager)),
    )
}

/// `menu` filled with the entries for `facts`, its commands sent from `keys`, the focus whose
/// shortcuts they show. With `tags`, the track also gets the Rating, Colour and My Tag
/// submenus, ticked from what it has stored.
pub fn build(
    menu: PopupMenu,
    facts: RowFacts,
    tags: Option<&TrackTags>,
    keys: FocusHandle,
    window: &mut Window,
    cx: &mut Context<'_, PopupMenu>,
) -> PopupMenu {
    let height = menu_height(window);
    let submenus = tags.map(|tags| {
        let (rating, colour, carried) = tags.stored.as_ref().map_or_else(
            || (0, None, Vec::new()),
            |stored| (stored.rating, stored.colour, stored.tag_ids.clone()),
        );
        (
            rating_choices(rating),
            colour_choices(colour),
            tag_groups(tags.cache.categories(), &carried),
        )
    });
    let mut menu = menu
        .action_context(keys.clone())
        .max_h(height)
        .scrollable(true);
    let mut separators = 0;
    for entry in entries(facts) {
        menu = match entry {
            MenuEntry::Item {
                label,
                command,
                enabled,
            } => menu.menu_with_disabled(label, Box::new(Perform(command)), !enabled),
            MenuEntry::Separator => {
                let separated = menu.separator();
                separators += 1;
                match &submenus {
                    Some((ratings, colours, groups)) if separators == SEPARATORS_BEFORE_TAGS => {
                        add_track_tag_submenus(
                            separated, ratings, colours, groups, &keys, height, window, cx,
                        )
                    }
                    _ => separated,
                }
            }
        };
    }
    menu
}

#[allow(clippy::too_many_arguments)]
fn add_track_tag_submenus(
    menu: PopupMenu,
    ratings: &[Choice],
    colours: &[Choice],
    groups: &[TagGroup],
    keys: &FocusHandle,
    height: Pixels,
    window: &mut Window,
    cx: &mut Context<'_, PopupMenu>,
) -> PopupMenu {
    let ratings = ratings.to_vec();
    let colours = colours.to_vec();
    let groups = groups.to_vec();
    let (rating_keys, colour_keys, tag_keys) = (keys.clone(), keys.clone(), keys.clone());
    menu.submenu("Rating", window, cx, move |submenu, _, _| {
        add_choices(submenu.action_context(rating_keys.clone()), &ratings)
    })
    .submenu("Colour", window, cx, move |submenu, _, _| {
        add_choices(submenu.action_context(colour_keys.clone()), &colours)
    })
    .submenu("My Tag", window, cx, move |submenu, _, _| {
        add_tag_groups(
            submenu
                .action_context(tag_keys.clone())
                .max_h(height)
                .scrollable(true),
            &groups,
        )
    })
    .separator()
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

    #[test]
    fn the_rating_submenu_ticks_the_tracks_stars() {
        let choices = rating_choices(4);
        assert_eq!(choices.len(), 6);
        assert_eq!(choices[0].label, "No Rating");
        assert_eq!(choices[1].label, "\u{2605}  1 star");
        assert_eq!(
            choices[4].label,
            "\u{2605}\u{2605}\u{2605}\u{2605}  4 stars"
        );
        assert_eq!(choices[4].command, Command::RateSelected(4));
        let ticked: Vec<usize> = choices
            .iter()
            .enumerate()
            .filter(|(_, choice)| choice.checked)
            .map(|(position, _)| position)
            .collect();
        assert_eq!(ticked, [4]);
    }

    #[test]
    fn the_colour_submenu_lists_none_then_the_palette_in_order() {
        let choices = colour_choices(Some(TrackColour::Green));
        let labels: Vec<&str> = choices.iter().map(|choice| choice.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "None", "Pink", "Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple"
            ]
        );
        assert_eq!(choices[0].command, Command::ColourSelected(None));
        assert_eq!(choices[0].swatch, None);
        assert_eq!(
            choices[2].command,
            Command::ColourSelected(Some(TrackColour::Red))
        );
        assert_eq!(choices[2].swatch, Some(TrackColour::Red));
        assert!(choices[5].checked);
        assert_eq!(choices.iter().filter(|choice| choice.checked).count(), 1);
        assert!(colour_choices(None)[0].checked);
    }

    #[test]
    fn the_my_tag_submenu_groups_tags_by_category_and_ticks_the_ones_the_track_has() {
        use dubplate_prep::Tag;

        let category = |id, name: &str, tags: &[(i64, &str)]| TagCategory {
            id: dubplate_prep::TagCategoryId(id),
            name: name.to_owned(),
            tags: tags
                .iter()
                .map(|(tag_id, tag_name)| Tag {
                    id: TagId(*tag_id),
                    name: (*tag_name).to_owned(),
                })
                .collect(),
        };
        let categories = [
            category(1, "Situation", &[(1, "Warm-up"), (2, "Peak")]),
            category(2, "Empty", &[]),
            category(3, "Vibe", &[(3, "Dark")]),
        ];
        let groups = tag_groups(&categories, &[TagId(2)]);
        assert_eq!(groups.len(), 2, "a category without tags is left out");
        assert_eq!(groups[0].category, "Situation");
        assert_eq!(
            groups[0]
                .tags
                .iter()
                .map(|choice| (choice.label.as_str(), choice.checked))
                .collect::<Vec<_>>(),
            [("Warm-up", false), ("Peak", true)]
        );
        assert_eq!(
            groups[1].tags[0].command,
            Command::ToggleSelectedTag(TagId(3))
        );
    }
}
