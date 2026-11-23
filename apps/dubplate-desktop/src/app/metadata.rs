//! The window's side of the library's stored metadata: one query of the set store when the
//! library loads or a set edit lands, cached beside the collection for the search filters.

use std::collections::HashMap;
use std::sync::Arc;

use dubplate_prep::{PlaylistTree, StoreError, TrackIdentity, TrackMetadata};
use gpui::Context;

use super::notice::NoticePlace;

use super::DubplateApp;
use crate::collection::Collection;
use crate::track_metadata::TrackMetadataCache;

impl DubplateApp {
    /// Reads the library's stored metadata in one query on the set worker and caches it
    /// beside the collection for the browser's filters. A track nothing is stored for keeps
    /// what rekordbox has for it.
    pub(super) fn refresh_metadata(
        &mut self,
        collection: Arc<Collection>,
        cx: &mut gpui::Context<'_, Self>,
    ) {
        self.track_metadata_generation += 1;
        let generation = self.track_metadata_generation;
        let Some(session) = self.set_editor.as_ref() else {
            return;
        };
        let job = session.submit(move |editor| {
            let identities: Vec<TrackIdentity> = collection
                .indexed_tracks()
                .map(|(_, track)| track.identity())
                .collect();
            let mut seeded: HashMap<TrackIdentity, TrackMetadata> = collection
                .indexed_tracks()
                .map(|(_, track)| {
                    (
                        track.identity(),
                        TrackMetadata::from_rekordbox(&track.track),
                    )
                })
                .collect();
            seeded.extend(editor.track_metadata_many(&identities)?);
            let categories = editor.tag_categories()?;
            Ok((
                TrackMetadataCache::from_library(seeded, &categories),
                editor.playlists(),
            ))
        });
        let job = match job {
            Ok(job) => job,
            Err(error) => {
                self.show_notice(
                    NoticePlace::Window,
                    format!("Could not read library metadata: {error}"),
                    cx,
                );
                return;
            }
        };
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    job.wait()
                        .map_err(|err| err.to_string())?
                        .outcome
                        .map_err(|err| err.to_string())
                })
                .await;
            this.update(cx, |this, cx| {
                if this.track_metadata_generation != generation {
                    return;
                }
                match result {
                    Ok((cache, tree)) => {
                        this.apply_playlists(tree, cx);
                        this.apply_track_metadata(cache, cx);
                    }
                    Err(error) => this.show_notice(
                        NoticePlace::Window,
                        format!("Could not read library metadata: {error}"),
                        cx,
                    ),
                }
            })
            .ok();
        })
        .detach();
    }

    /// Applies the latest saved playlists or reports a playlist-only read failure.
    pub(super) fn apply_playlists(
        &mut self,
        tree: Result<PlaylistTree, StoreError>,
        cx: &mut Context<'_, Self>,
    ) {
        match tree {
            Ok(tree) => self
                .browser
                .update(cx, |browser, cx| browser.set_playlists(tree, cx)),
            Err(error) => self.show_notice(
                NoticePlace::Window,
                format!("Could not read playlists: {error}"),
                cx,
            ),
        }
    }

    /// Gives the metadata read from the set store to the browser's filters.
    fn apply_track_metadata(
        &mut self,
        cache: TrackMetadataCache,
        cx: &mut gpui::Context<'_, Self>,
    ) {
        self.track_metadata = Arc::new(cache);
        self.browser.update(cx, |browser, cx| {
            browser.set_track_metadata(Arc::clone(&self.track_metadata), cx)
        });
    }
}
