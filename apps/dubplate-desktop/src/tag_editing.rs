//! What the My Tag manager asks of the store: the edits that make, rename, move and delete the
//! DJ's categories and tags, worked out away from the window so they can be tested without one.

use dubplate_prep::{TagCategory, TagCategoryId, TagId, TrackTagEdit};

use crate::commands::Direction;

/// The longest name a category or tag may have.
pub const MAX_NAME_LENGTH: usize = 40;

/// What the name field is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Editing {
    /// A new category, last among them.
    NewCategory,
    /// A new name for a category.
    RenameCategory(TagCategoryId),
    /// A new tag, last in its category.
    NewTag(TagCategoryId),
    /// A new name for a tag.
    RenameTag(TagId),
}

/// What the DJ is about to delete, waiting for a second click to confirm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Deleting {
    /// A category with its tags.
    Category(TagCategoryId),
    /// A tag.
    Tag(TagId),
}

fn category_of_tag(categories: &[TagCategory], tag: TagId) -> Option<&TagCategory> {
    categories
        .iter()
        .find(|category| category.tags.iter().any(|candidate| candidate.id == tag))
}

/// The name the field starts with: the current name when renaming, nothing for a new one.
pub fn initial_text(editing: Editing, categories: &[TagCategory]) -> String {
    match editing {
        Editing::NewCategory | Editing::NewTag(_) => String::new(),
        Editing::RenameCategory(id) => categories
            .iter()
            .find(|category| category.id == id)
            .map(|category| category.name.clone())
            .unwrap_or_default(),
        Editing::RenameTag(id) => category_of_tag(categories, id)
            .and_then(|category| category.tags.iter().find(|tag| tag.id == id))
            .map(|tag| tag.name.clone())
            .unwrap_or_default(),
    }
}

fn taken_among<'name>(others: impl Iterator<Item = (&'name str, bool)>, name: &str) -> bool {
    others
        .into_iter()
        .any(|(other, is_itself)| !is_itself && same_name(other, name))
}

fn same_name(first: &str, second: &str) -> bool {
    first.to_lowercase() == second.to_lowercase()
}

/// The edit that saves `name` for what is being edited, or what is wrong with the name: it
/// must not be empty or too long, nor repeat another category's name, nor another tag's in the
/// same category.
pub fn commit(
    editing: Editing,
    name: &str,
    categories: &[TagCategory],
) -> Result<TrackTagEdit, &'static str> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Type a name first.");
    }
    if name.chars().count() > MAX_NAME_LENGTH {
        return Err("That name is too long.");
    }
    match editing {
        Editing::NewCategory => {
            if taken_among(
                categories
                    .iter()
                    .map(|category| (category.name.as_str(), false)),
                name,
            ) {
                return Err("A category has that name already.");
            }
            Ok(TrackTagEdit::CreateCategory {
                name: name.to_owned(),
                position: categories.len(),
            })
        }
        Editing::RenameCategory(id) => {
            if taken_among(
                categories
                    .iter()
                    .map(|category| (category.name.as_str(), category.id == id)),
                name,
            ) {
                return Err("A category has that name already.");
            }
            Ok(TrackTagEdit::RenameCategory {
                category: id,
                name: name.to_owned(),
            })
        }
        Editing::NewTag(category) => {
            let Some(found) = categories.iter().find(|candidate| candidate.id == category) else {
                return Err("That category is gone.");
            };
            if taken_among(
                found.tags.iter().map(|tag| (tag.name.as_str(), false)),
                name,
            ) {
                return Err("This category has a tag with that name already.");
            }
            Ok(TrackTagEdit::CreateTag {
                category,
                name: name.to_owned(),
                position: found.tags.len(),
            })
        }
        Editing::RenameTag(tag) => {
            let Some(found) = category_of_tag(categories, tag) else {
                return Err("That tag is gone.");
            };
            if taken_among(
                found
                    .tags
                    .iter()
                    .map(|other| (other.name.as_str(), other.id == tag)),
                name,
            ) {
                return Err("This category has a tag with that name already.");
            }
            Ok(TrackTagEdit::RenameTag {
                tag,
                name: name.to_owned(),
            })
        }
    }
}

/// The place a row at `position` among `count` moves to, or `None` at the end it cannot pass.
fn moved(position: usize, count: usize, direction: Direction) -> Option<usize> {
    match direction {
        Direction::Up => position.checked_sub(1),
        Direction::Down => (position + 1 < count).then_some(position + 1),
    }
}

/// The edit that moves a category one place up or down, or `None` at either end.
pub fn move_category(
    categories: &[TagCategory],
    category: TagCategoryId,
    direction: Direction,
) -> Option<TrackTagEdit> {
    let position = categories.iter().position(|other| other.id == category)?;
    Some(TrackTagEdit::MoveCategory {
        category,
        position: moved(position, categories.len(), direction)?,
    })
}

/// The edit that moves a tag one place up or down inside its category, or `None` at either
/// end.
pub fn move_tag(
    categories: &[TagCategory],
    tag: TagId,
    direction: Direction,
) -> Option<TrackTagEdit> {
    let category = category_of_tag(categories, tag)?;
    let position = category.tags.iter().position(|other| other.id == tag)?;
    Some(TrackTagEdit::MoveTag {
        tag,
        position: moved(position, category.tags.len(), direction)?,
    })
}

/// The edit that carries out a confirmed delete.
pub fn delete_edit(deleting: Deleting) -> TrackTagEdit {
    match deleting {
        Deleting::Category(category) => TrackTagEdit::DeleteCategory(category),
        Deleting::Tag(tag) => TrackTagEdit::DeleteTag(tag),
    }
}

/// How many tags a category holds, for the delete confirmation to say what goes with it.
pub fn tag_count(categories: &[TagCategory], category: TagCategoryId) -> usize {
    categories
        .iter()
        .find(|other| other.id == category)
        .map_or(0, |found| found.tags.len())
}

#[cfg(test)]
mod tests {
    use dubplate_prep::Tag;

    use super::*;

    fn categories() -> Vec<TagCategory> {
        let tag = |id, name: &str| Tag {
            id: TagId(id),
            name: name.to_owned(),
        };
        vec![
            TagCategory {
                id: TagCategoryId(1),
                name: "Situation".to_owned(),
                tags: vec![tag(1, "Warm-up"), tag(2, "Peak"), tag(3, "Closing")],
            },
            TagCategory {
                id: TagCategoryId(2),
                name: "Vibe".to_owned(),
                tags: vec![tag(4, "Warm-up")],
            },
        ]
    }

    #[test]
    fn a_new_category_goes_last_and_a_new_tag_goes_last_in_its_category() {
        let found = categories();
        assert_eq!(
            commit(Editing::NewCategory, "  Energy ", &found),
            Ok(TrackTagEdit::CreateCategory {
                name: "Energy".to_owned(),
                position: 2
            })
        );
        assert_eq!(
            commit(Editing::NewTag(TagCategoryId(1)), "Sunrise", &found),
            Ok(TrackTagEdit::CreateTag {
                category: TagCategoryId(1),
                name: "Sunrise".to_owned(),
                position: 3
            })
        );
    }

    #[test]
    fn names_are_checked_before_the_store_sees_them() {
        let found = categories();
        assert!(commit(Editing::NewCategory, "   ", &found).is_err());
        assert!(commit(Editing::NewCategory, &"x".repeat(41), &found).is_err());
        assert!(commit(Editing::NewCategory, "vibe", &found).is_err());
        assert!(commit(Editing::NewTag(TagCategoryId(1)), "PEAK", &found).is_err());
        assert!(
            commit(Editing::NewTag(TagCategoryId(2)), "Peak", &found).is_ok(),
            "another category may repeat the name"
        );
        assert!(
            commit(Editing::RenameTag(TagId(2)), "peak", &found).is_ok(),
            "a tag may change only the case of its own name"
        );
        assert!(commit(Editing::RenameTag(TagId(2)), "Closing", &found).is_err());
        assert_eq!(
            commit(Editing::RenameCategory(TagCategoryId(2)), "Mood", &found),
            Ok(TrackTagEdit::RenameCategory {
                category: TagCategoryId(2),
                name: "Mood".to_owned()
            })
        );
    }

    #[test]
    fn renaming_starts_from_the_current_name() {
        let found = categories();
        assert_eq!(
            initial_text(Editing::RenameTag(TagId(3)), &found),
            "Closing"
        );
        assert_eq!(
            initial_text(Editing::RenameCategory(TagCategoryId(1)), &found),
            "Situation"
        );
        assert_eq!(initial_text(Editing::NewCategory, &found), "");
    }

    #[test]
    fn rows_move_one_place_and_stop_at_the_ends() {
        let found = categories();
        assert_eq!(
            move_category(&found, TagCategoryId(1), Direction::Down),
            Some(TrackTagEdit::MoveCategory {
                category: TagCategoryId(1),
                position: 1
            })
        );
        assert_eq!(move_category(&found, TagCategoryId(1), Direction::Up), None);
        assert_eq!(
            move_category(&found, TagCategoryId(2), Direction::Down),
            None
        );
        assert_eq!(
            move_tag(&found, TagId(2), Direction::Up),
            Some(TrackTagEdit::MoveTag {
                tag: TagId(2),
                position: 0
            })
        );
        assert_eq!(move_tag(&found, TagId(3), Direction::Down), None);
        assert_eq!(move_tag(&found, TagId(4), Direction::Up), None);
    }

    #[test]
    fn deleting_names_what_goes() {
        assert_eq!(
            delete_edit(Deleting::Category(TagCategoryId(1))),
            TrackTagEdit::DeleteCategory(TagCategoryId(1))
        );
        assert_eq!(
            delete_edit(Deleting::Tag(TagId(4))),
            TrackTagEdit::DeleteTag(TagId(4))
        );
        assert_eq!(tag_count(&categories(), TagCategoryId(1)), 3);
    }
}
