//! Rating, colouring and tagging the selected track: each asks the set worker to store the
//! change, and the metadata cache is read again once it lands, so the table and the menu show
//! it at once.

use std::collections::HashSet;

use dubplate_prep::{TagId, TrackColour, TrackIdentity, TrackTagEdit};
use gpui::Context;

use super::DubplateApp;
use crate::track_edits::{
    TrackTarget, colour_edits, rating_edits, seeding_edits, tag_toggle_edits,
};

impl DubplateApp {
    /// The tracks a rating, colour or tag applies to: the selected track, the set's when the
    /// keys are on the set, else the library's.
    fn selected_targets(&self, cx: &gpui::App) -> Vec<TrackTarget> {
        let (Some(collection), Some(track)) = (self.collection(), self.selected_track(cx)) else {
            return Vec::new();
        };
        vec![TrackTarget::of(collection.track(track))]
    }

    /// Gives the selected track `rating` stars, 0 for unrated.
    pub(super) fn rate_selected(&mut self, rating: u8, cx: &mut Context<'_, Self>) {
        let targets = self.selected_targets(cx);
        let edits = rating_edits(&targets, rating);
        self.store_track_edits(targets, edits, cx);
    }

    /// Gives the selected track a colour label, or takes its label away with `None`.
    pub(super) fn colour_selected(
        &mut self,
        colour: Option<TrackColour>,
        cx: &mut Context<'_, Self>,
    ) {
        let targets = self.selected_targets(cx);
        let edits = colour_edits(&targets, colour);
        self.store_track_edits(targets, edits, cx);
    }

    /// Puts a My Tag on the selected track, or takes it off when the track carries it.
    pub(super) fn toggle_selected_tag(&mut self, tag: TagId, cx: &mut Context<'_, Self>) {
        let targets = self.selected_targets(cx);
        let edits = tag_toggle_edits(&targets, tag, &self.track_metadata);
        self.store_track_edits(targets, edits, cx);
    }

    fn store_track_edits(
        &mut self,
        targets: Vec<TrackTarget>,
        edits: Vec<TrackTagEdit>,
        cx: &mut Context<'_, Self>,
    ) {
        if edits.is_empty() {
            return;
        }
        self.queue_set_edit(
            move |editor| {
                let identities: Vec<TrackIdentity> = targets
                    .iter()
                    .map(|target| target.identity.clone())
                    .collect();
                let stored: HashSet<TrackIdentity> = editor
                    .track_metadata_many(&identities)?
                    .into_keys()
                    .collect();
                for target in targets
                    .iter()
                    .filter(|target| !stored.contains(&target.identity))
                {
                    for seeding in seeding_edits(target) {
                        editor.edit_track_tags(seeding)?;
                    }
                }
                for edit in edits {
                    editor.edit_track_tags(edit)?;
                }
                Ok(())
            },
            |_, (), _| {},
            cx,
        );
    }
}
