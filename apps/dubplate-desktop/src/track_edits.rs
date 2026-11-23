//! What a rating, a colour or a My Tag toggle asks of the store for the tracks it applies to,
//! worked out away from the window so it can be tested without one.

use std::path::PathBuf;

use dubplate_prep::{TagId, TrackColour, TrackIdentity, TrackMetadata, TrackTagEdit};

use crate::collection::CollectionTrack;
use crate::track_metadata::{MAX_RATING, TrackMetadataCache};

/// A track an edit applies to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackTarget {
    /// What tells the track apart from every other.
    pub identity: TrackIdentity,
    /// Its audio file, when it is known.
    pub path: Option<PathBuf>,
    /// What rekordbox has for it, kept for the track the first time Dubplate stores anything
    /// of its own, so a colour does not wipe the rating it came with.
    pub seed: TrackMetadata,
}

impl TrackTarget {
    /// The target for a track of the library.
    pub fn of(entry: &CollectionTrack) -> Self {
        Self {
            identity: entry.identity(),
            path: entry.track.file_path.clone(),
            seed: TrackMetadata::from_rekordbox(&entry.track),
        }
    }
}

/// The edits that give every target `rating` stars, 0 for unrated. A rating above the most
/// stars is held to it.
pub fn rating_edits(targets: &[TrackTarget], rating: u8) -> Vec<TrackTagEdit> {
    targets
        .iter()
        .map(|target| TrackTagEdit::SetRating {
            track: target.identity.clone(),
            track_path: target.path.clone(),
            rating: rating.min(MAX_RATING),
        })
        .collect()
}

/// The edits that give every target the colour label `colour`, or take it away with `None`.
pub fn colour_edits(targets: &[TrackTarget], colour: Option<TrackColour>) -> Vec<TrackTagEdit> {
    targets
        .iter()
        .map(|target| TrackTagEdit::SetColour {
            track: target.identity.clone(),
            track_path: target.path.clone(),
            colour,
        })
        .collect()
}

/// The edit a click on the tag in the menu makes: it comes off every target when all of them
/// carry it, else it goes on all of them.
pub fn tag_toggle_edits(
    targets: &[TrackTarget],
    tag: TagId,
    cache: &TrackMetadataCache,
) -> Vec<TrackTagEdit> {
    if targets.is_empty() {
        return Vec::new();
    }
    let carries = |target: &TrackTarget| {
        cache
            .for_track(&target.identity)
            .is_some_and(|stored| stored.tag_ids.contains(&tag))
    };
    let tracks = targets
        .iter()
        .map(|target| target.identity.clone())
        .collect();
    vec![if targets.iter().all(carries) {
        TrackTagEdit::RemoveTag { tracks, tag }
    } else {
        TrackTagEdit::AddTag { tracks, tag }
    }]
}

/// The edits that store what rekordbox has for a target before the DJ's own change lands on
/// it: its rating and its comment, when it has any.
pub fn seeding_edits(target: &TrackTarget) -> Vec<TrackTagEdit> {
    let mut edits = Vec::new();
    if target.seed.rating > 0 {
        edits.push(TrackTagEdit::SetRating {
            track: target.identity.clone(),
            track_path: target.path.clone(),
            rating: target.seed.rating,
        });
    }
    if !target.seed.comment.is_empty() {
        edits.push(TrackTagEdit::SetComment {
            track: target.identity.clone(),
            track_path: target.path.clone(),
            comment: target.seed.comment.clone(),
        });
    }
    edits
}

#[cfg(test)]
mod tests {
    use dubplate_prep::{Tag, TagCategory, TagCategoryId};

    use super::*;

    fn target(id: &str, rating: u8, comment: &str) -> TrackTarget {
        TrackTarget {
            identity: TrackIdentity::Rekordbox(id.to_owned()),
            path: Some(PathBuf::from(format!("/music/{id}.mp3"))),
            seed: TrackMetadata {
                track_path: None,
                rating,
                colour: None,
                comment: comment.to_owned(),
                tags: Vec::new(),
            },
        }
    }

    fn peak_tag() -> Tag {
        Tag {
            id: TagId(7),
            name: "Peak".to_owned(),
        }
    }

    fn cache_where(carrying: &[&str]) -> TrackMetadataCache {
        let stored = |id: &str| TrackMetadata {
            tags: if carrying.contains(&id) {
                vec![peak_tag()]
            } else {
                Vec::new()
            },
            ..TrackMetadata::default()
        };
        TrackMetadataCache::from_library(
            ["1", "2"]
                .into_iter()
                .map(|id| (TrackIdentity::Rekordbox(id.to_owned()), stored(id))),
            &[TagCategory {
                id: TagCategoryId(1),
                name: "Situation".to_owned(),
                tags: vec![peak_tag()],
            }],
        )
    }

    #[test]
    fn a_rating_is_set_on_every_selected_track() {
        let edits = rating_edits(&[target("1", 0, ""), target("2", 0, "")], 4);
        assert_eq!(
            edits,
            [
                TrackTagEdit::SetRating {
                    track: TrackIdentity::Rekordbox("1".to_owned()),
                    track_path: Some(PathBuf::from("/music/1.mp3")),
                    rating: 4,
                },
                TrackTagEdit::SetRating {
                    track: TrackIdentity::Rekordbox("2".to_owned()),
                    track_path: Some(PathBuf::from("/music/2.mp3")),
                    rating: 4,
                },
            ]
        );
    }

    #[test]
    fn a_rating_past_five_stars_is_held_to_five() {
        let edits = rating_edits(&[target("1", 0, "")], 9);
        assert!(matches!(
            edits.as_slice(),
            [TrackTagEdit::SetRating { rating: 5, .. }]
        ));
    }

    #[test]
    fn none_takes_the_colour_away() {
        let edits = colour_edits(&[target("1", 0, "")], None);
        assert!(matches!(
            edits.as_slice(),
            [TrackTagEdit::SetColour { colour: None, .. }]
        ));
        let edits = colour_edits(&[target("1", 0, "")], Some(TrackColour::Aqua));
        assert!(matches!(
            edits.as_slice(),
            [TrackTagEdit::SetColour {
                colour: Some(TrackColour::Aqua),
                ..
            }]
        ));
    }

    #[test]
    fn a_tag_goes_on_unless_every_target_carries_it() {
        let targets = [target("1", 0, ""), target("2", 0, "")];
        let both = TrackTagEdit::AddTag {
            tracks: vec![
                TrackIdentity::Rekordbox("1".to_owned()),
                TrackIdentity::Rekordbox("2".to_owned()),
            ],
            tag: TagId(7),
        };
        assert_eq!(
            tag_toggle_edits(&targets, TagId(7), &cache_where(&[])),
            std::slice::from_ref(&both)
        );
        assert_eq!(
            tag_toggle_edits(&targets, TagId(7), &cache_where(&["1"])),
            [both]
        );
        assert_eq!(
            tag_toggle_edits(&targets, TagId(7), &cache_where(&["1", "2"])),
            [TrackTagEdit::RemoveTag {
                tracks: vec![
                    TrackIdentity::Rekordbox("1".to_owned()),
                    TrackIdentity::Rekordbox("2".to_owned()),
                ],
                tag: TagId(7),
            }]
        );
        assert!(tag_toggle_edits(&[], TagId(7), &cache_where(&[])).is_empty());
    }

    #[test]
    fn a_first_edit_keeps_what_rekordbox_had() {
        let seeded = seeding_edits(&target("1", 3, "Closing"));
        assert_eq!(seeded.len(), 2);
        assert!(matches!(
            seeded[0],
            TrackTagEdit::SetRating { rating: 3, .. }
        ));
        assert!(
            matches!(&seeded[1], TrackTagEdit::SetComment { comment, .. } if comment == "Closing")
        );
        assert!(seeding_edits(&target("1", 0, "")).is_empty());
    }
}
