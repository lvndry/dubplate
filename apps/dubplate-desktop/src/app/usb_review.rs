//! Review player histories and cue edits before copying them into Dubplate.
use std::{collections::HashMap, path::Path, sync::Arc};

use dubplate_prep::{ImportedHistory, TrackIdentity};
use dubplate_rekordbox::{CueChange, TrackCues, stick_import::stick_catalog_and_cues};
use gpui::{ClickEvent, Context, PathPromptOptions, base::FocusTrapElement, div, prelude::*, px};

use super::{DubplateApp, notice::NoticePlace};
use crate::{
    collection::{Collection, CollectionTrack},
    theme,
    widgets::{Emphasis, button},
};

struct TrackReview {
    track: Arc<CollectionTrack>,
    before: Arc<TrackCues>,
    changes: Vec<(CueChange, bool)>,
}

pub(super) struct Review {
    page: usize,
    histories: Vec<(ImportedHistory, bool)>,
    tracks: Vec<TrackReview>,
    skipped: Vec<String>,
}

fn read(
    root: &Path,
    collection: &Collection,
    saved: &HashMap<TrackIdentity, Arc<TrackCues>>,
) -> Result<Review, String> {
    let (catalog, cues) = stick_catalog_and_cues(root).map_err(|e| e.to_string())?;
    let mut paths: HashMap<String, Vec<&Arc<CollectionTrack>>> = HashMap::new();
    for (_, local) in collection.indexed_tracks() {
        if let Some(name) = local.track.file_path.as_ref().and_then(|p| p.file_name()) {
            paths
                .entry(dubplate_usb::contents_path(
                    local.track.artist.as_deref(),
                    local.track.album.as_deref(),
                    &name.to_string_lossy(),
                ))
                .or_default()
                .push(local);
        }
    }
    let stick = root.file_name().unwrap_or_default().to_string_lossy();
    let by_id: HashMap<_, _> = catalog.tracks.iter().map(|t| (t.id, t)).collect();
    let artists: HashMap<_, _> = catalog
        .artists
        .iter()
        .map(|a| (a.id, a.name.as_str()))
        .collect();
    let histories = catalog
        .histories()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|history| {
            let tracks = history
                .tracks
                .iter()
                .map(|id| match by_id.get(id) {
                    Some(t) => (
                        t.artist
                            .and_then(|id| artists.get(&id).copied())
                            .unwrap_or_default()
                            .to_owned(),
                        t.title.clone(),
                    ),
                    None => (String::new(), format!("Unknown track {id}")),
                })
                .collect();
            (
                ImportedHistory {
                    library: catalog.library_id.0,
                    history: history.id.get(),
                    label: format!("{} · {stick}", history.name),
                    tracks,
                },
                false,
            )
        })
        .collect();
    let mut review = Review {
        page: 0,
        histories,
        tracks: Vec::new(),
        skipped: Vec::new(),
    };
    for device in &catalog.tracks {
        let original = dubplate_usb::unnumbered_path(&device.file_path);
        let candidates = paths
            .get(&device.file_path)
            .into_iter()
            .chain(original.as_ref().and_then(|p| paths.get(p)))
            .flatten()
            .copied()
            .collect::<Vec<_>>();
        let track = match candidates.as_slice() {
            [track]
                if original.is_none()
                    && track.track.file_size.is_none_or(|size| {
                        size.min(u64::from(u32::MAX)) == u64::from(device.file_size)
                    }) =>
            {
                Some(*track)
            }
            _ => {
                // A candidate only comes from the exporter's sanitized path. Compare audio when
                // different local files could have produced this numbered name.
                let audio = root.join(device.file_path.trim_start_matches('/'));
                let matches = candidates
                    .into_iter()
                    .filter(|t| {
                        t.track.file_path.as_ref().is_some_and(|p| {
                            dubplate_usb::same_contents(p, &audio).unwrap_or(false)
                        })
                    })
                    .collect::<Vec<_>>();
                match matches.as_slice() {
                    [only] => Some(*only),
                    _ => None,
                }
            }
        };
        let Some(track) = track else {
            review.skipped.push(format!(
                "{} · No unique library match ({})",
                device.title, device.file_path
            ));
            continue;
        };
        let Some(imported) = cues.get(device.id).filter(|c| !c.missing) else {
            let reason = cues
                .unreadable()
                .iter()
                .find(|(id, _)| *id == device.id)
                .map_or("Missing analysis", |(_, why)| why);
            review.skipped.push(format!("{} · {reason}", device.title));
            continue;
        };
        let before = saved.get(&track.identity()).unwrap_or(&track.cues);
        let changes = imported.cues.changes_from(before);
        if !changes.is_empty() {
            review.tracks.push(TrackReview {
                track: Arc::clone(track),
                before: Arc::clone(before),
                changes: changes.into_iter().map(|c| (c, false)).collect(),
            });
        }
    }
    Ok(review)
}

fn apply(cues: &mut TrackCues, change: &CueChange) {
    match change {
        CueChange::HotCueSet { pad, cue }
        | CueChange::HotCueChanged {
            pad, after: cue, ..
        } => cues.set_hot_cue(*pad, cue.clone()),
        CueChange::HotCueCleared { pad, .. } => cues.delete_hot_cue(*pad),
        CueChange::MemoryCueAdded(cue) => cues.insert_memory_cue(cue.clone()),
        CueChange::MemoryCueRemoved(cue) | CueChange::MemoryCueChanged { before: cue, .. } => {
            let mut removed = false;
            cues.remove_memory_cues(|c| {
                let matches = !removed && c == cue;
                removed |= matches;
                matches
            });
            if let CueChange::MemoryCueChanged { after, .. } = change {
                cues.insert_memory_cue(after.clone());
            }
        }
    }
}

impl DubplateApp {
    pub(super) fn choose_usb(&mut self, cx: &mut Context<'_, Self>) {
        let Some(collection) = self.collection().cloned() else {
            self.show_notice(
                NoticePlace::Window,
                "Wait for the music library to open.",
                cx,
            );
            return;
        };
        let saved = self.track_cues.clone();
        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Read USB export".into()),
        });
        cx.spawn(async move |this, cx| {
            let chosen = match picker.await {
                Ok(Ok(Some(paths))) => paths,
                Ok(Err(error)) => {
                    this.update(cx, |app, cx| {
                        app.show_notice(NoticePlace::Window, error.to_string(), cx)
                    })
                    .ok();
                    return;
                }
                _ => return,
            };
            let Some(path) = chosen.into_iter().next() else {
                return;
            };
            let result = cx
                .background_executor()
                .spawn(async move { read(&path, &collection, &saved) })
                .await;
            this.update(cx, |app, cx| match result {
                Ok(review) => {
                    app.library_open = false;
                    app.usb_review = Some(review);
                    cx.notify();
                }
                Err(error) => app.show_notice(NoticePlace::Window, error, cx),
            })
            .ok();
        })
        .detach();
    }

    fn import_selection(&mut self, cx: &mut Context<'_, Self>) {
        let Some(session) = &self.set_editor else {
            self.show_notice(
                NoticePlace::Window,
                "Wait for Dubplate’s database to open.",
                cx,
            );
            return;
        };
        let Some(review) = self.usb_review.as_ref() else {
            return;
        };
        let histories: Vec<_> = review
            .histories
            .iter()
            .filter(|(_, yes)| *yes)
            .map(|(h, _)| h.clone())
            .collect();
        let mut updates = Vec::new();
        let mut count = 0;
        for row in &review.tracks {
            if !row.changes.iter().any(|(_, yes)| *yes) {
                continue;
            }
            if self.cues_of(&row.track) != row.before {
                self.show_notice(
                    NoticePlace::Window,
                    "Local cues changed during review. Reopen the stick to compare again.",
                    cx,
                );
                return;
            }
            let mut cues = (*row.before).clone();
            for (change, _) in row.changes.iter().filter(|(_, yes)| *yes) {
                apply(&mut cues, change);
                count += 1;
            }
            updates.push((
                row.track.identity(),
                row.track.track.file_path.clone(),
                cues,
            ));
        }
        let history_count = histories.len();
        if history_count == 0 && updates.is_empty() {
            self.show_notice(
                NoticePlace::Window,
                "Select a history or cue change to import.",
                cx,
            );
            return;
        }
        let saved = updates.clone();
        let job =
            match session.submit(move |editor| editor.import_usb_selection(&histories, &saved)) {
                Ok(job) => job,
                Err(e) => {
                    self.show_notice(NoticePlace::Window, e.to_string(), cx);
                    return;
                }
            };
        self.usb_review = None;
        self.set_edits_pending += 1;
        let prior: Vec<_> = updates
            .iter()
            .map(|(id, _, _)| (id.clone(), self.track_cues.get(id).cloned()))
            .collect();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { job.wait() })
                .await;
            this.update(cx, |app, cx| {
                app.set_edits_pending = app.set_edits_pending.saturating_sub(1);
                match result
                    .map_err(|e| e.to_string())
                    .and_then(|c| c.outcome.map_err(|e| e.to_string()))
                {
                    Ok(imported) => {
                        let changes = updates
                            .into_iter()
                            .zip(prior)
                            .filter(|((id, _, _), (_, before))| {
                                app.track_cues.get(id) == before.as_ref()
                            })
                            .map(|((id, _, cues), _)| (id, cues))
                            .collect();
                        app.restore_track_cues(changes);
                        app.play_history.imported = imported;
                        app.history_changed(cx);
                        app.show_notice(
                            NoticePlace::Window,
                            format!("Imported {history_count} histories and {count} cue changes."),
                            cx,
                        );
                    }
                    Err(e) => {
                        app.show_notice(
                            NoticePlace::Window,
                            format!("USB import could not be saved. {e}"),
                            cx,
                        );
                    }
                }
                app.finish_quit_if_saved(cx);
            })
            .ok();
        })
        .detach();
    }
}

pub(super) fn render(
    app: &DubplateApp,
    review: &Review,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement {
    let mut card = div()
        .id("usb-review-card")
        .role(gpui::Role::Dialog)
        .aria_label("Import from USB")
        .key_context("Modal")
        .track_focus(&app.modal_focus)
        .tab_group()
        .tab_stop(false)
        .flex()
        .flex_col()
        .gap(px(8.))
        .w(px(580.))
        .max_h(px(490.))
        .overflow_y_scroll()
        .p(px(16.))
        .bg(theme::active().panel)
        .child("Import from USB");
    const PAGE_SIZE: usize = 12;
    let start = review.page * PAGE_SIZE;
    let pages =
        (review.histories.len() + review.tracks.len() + review.skipped.len()).div_ceil(PAGE_SIZE);
    let history_start = start.min(review.histories.len());
    let history_end = (start + PAGE_SIZE).min(review.histories.len());
    let tracks_start = start
        .saturating_sub(review.histories.len())
        .min(review.tracks.len());
    let tracks_end = (start + PAGE_SIZE)
        .saturating_sub(review.histories.len())
        .min(review.tracks.len());
    let skipped_start = start.saturating_sub(review.histories.len() + review.tracks.len());
    let skipped_end = (start + PAGE_SIZE)
        .saturating_sub(review.histories.len() + review.tracks.len())
        .min(review.skipped.len());
    if pages > 1 {
        let mut navigation = div()
            .flex()
            .gap(px(8.))
            .child(format!("Page {} of {pages}", review.page + 1));
        for (id, label, page) in [
            ("usb-prev", "Previous", review.page.checked_sub(1)),
            (
                "usb-next",
                "Next",
                (review.page + 1 < pages).then_some(review.page + 1),
            ),
        ] {
            if let Some(page) = page {
                navigation = navigation.child(button(id, label, Emphasis::Secondary).on_click(
                    cx.listener(move |app, _: &ClickEvent, _, cx| {
                        if let Some(review) = &mut app.usb_review {
                            review.page = page;
                        }
                        cx.notify();
                    }),
                ));
            }
        }
        card = card.child(navigation);
    }
    if history_start < history_end {
        card = card.child("CDJ histories · date unavailable on this export");
    }
    for (index, (history, selected)) in review
        .histories
        .iter()
        .enumerate()
        .take(history_end)
        .skip(history_start)
    {
        card = card.child(
            button(
                ("usb-history", index),
                format!(
                    "{} {} · {} tracks",
                    mark(*selected),
                    history.label,
                    history.tracks.len()
                ),
                Emphasis::Secondary,
            )
            .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                if let Some(r) = &mut app.usb_review {
                    let yes = &mut r.histories[index].1;
                    *yes = !*yes;
                }
                cx.notify();
            })),
        );
    }
    if !review.tracks.is_empty() {
        card = card.child(
            button("usb-all", "Select all cue changes", Emphasis::Secondary).on_click(cx.listener(
                |app, _: &ClickEvent, _, cx| {
                    if let Some(r) = &mut app.usb_review {
                        for t in &mut r.tracks {
                            for (_, yes) in &mut t.changes {
                                *yes = true;
                            }
                        }
                    }
                    cx.notify();
                },
            )),
        );
    }
    let mut row_id = 0_usize;
    for (index, t) in review
        .tracks
        .iter()
        .enumerate()
        .take(tracks_end)
        .skip(tracks_start)
    {
        card = card.child(
            button(
                ("usb-track", index),
                format!("{} · {} changes", t.track.row.title, t.changes.len()),
                Emphasis::Secondary,
            )
            .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                if let Some(r) = &mut app.usb_review {
                    let changes = &mut r.tracks[index].changes;
                    let select = !changes.iter().all(|(_, yes)| *yes);
                    for (_, yes) in changes {
                        *yes = select;
                    }
                }
                cx.notify();
            })),
        );
        for (position, (change, selected)) in t.changes.iter().enumerate() {
            card = card.child(
                button(
                    ("usb-change", row_id),
                    format!("{} {change}", mark(*selected)),
                    Emphasis::Secondary,
                )
                .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                    if let Some(r) = &mut app.usb_review {
                        let yes = &mut r.tracks[index].changes[position].1;
                        *yes = !*yes;
                    }
                    cx.notify();
                })),
            );
            row_id += 1;
        }
    }
    for warning in &review.skipped[skipped_start..skipped_end] {
        card = card.child(
            div()
                .text_color(theme::active().muted)
                .child(warning.clone()),
        );
    }
    if review.tracks.is_empty() && review.histories.is_empty() {
        card = card.child("No histories or cue changes to import.");
    }
    card = card.child(
        div()
            .flex()
            .gap(px(8.))
            .child(
                button("usb-back", "Close", Emphasis::Secondary).on_click(cx.listener(
                    |app, _: &ClickEvent, _, cx| {
                        app.usb_review = None;
                        cx.notify();
                    },
                )),
            )
            .child(
                button("usb-accept", "Import selected", Emphasis::Primary)
                    .on_click(cx.listener(|app, _: &ClickEvent, _, cx| app.import_selection(cx))),
            ),
    );
    div()
        .absolute()
        .inset_0()
        .occlude()
        .flex()
        .items_center()
        .justify_center()
        .bg(theme::active().scrim)
        .child(card.focus_trap("usb-import-focus", &app.modal_focus))
}
fn mark(selected: bool) -> &'static str {
    if selected { "☑" } else { "☐" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dubplate_rekordbox::{HotCue, HotCuePad, MemoryCue};
    use dubplate_usb::{TrackTags, UsbPlaylist, UsbTrack};

    #[test]
    fn exported_tracks_match_and_cue_choices_preserve_unselected_changes() {
        let music = tempfile::tempdir().unwrap();
        let stick = tempfile::tempdir().unwrap();
        let source = music.path().join("track.wav");
        let mut audio = hound::WavWriter::create(
            &source,
            hound::WavSpec {
                channels: 1,
                sample_rate: 44100,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for _ in 0..4410 {
            audio.write_sample(0_i16).unwrap();
        }
        audio.finalize().unwrap();
        let scan = dubplate_library::scan_folder(music.path(), None, &Default::default()).unwrap();
        let collection = Collection::build(None, &[scan]);
        let mut player = TrackCues::default();
        player.set_hot_cue(HotCuePad::A, HotCue::at(25));
        player.insert_memory_cue(MemoryCue::at(50));
        let playlist = UsbPlaylist {
            name: "Evening".into(),
            tracks: vec![UsbTrack {
                source,
                tags: TrackTags::default(),
                bpm: None,
                duration_seconds: None,
                beat_grid: None,
                grid_is_rekordbox_s: false,
                cues: dubplate_usb::analysis_cues(&player),
                rekordbox_analysis: None,
            }],
        };
        dubplate_usb::export_to_usb(
            stick.path(),
            &[playlist],
            jiff::civil::date(2026, 11, 23),
            &mut |_| {},
        )
        .unwrap();
        let review = read(stick.path(), &collection, &HashMap::new()).unwrap();
        assert!(review.skipped.is_empty(), "{:?}", review.skipped);
        let row = &review.tracks[0];
        let mut imported = (*row.before).clone();
        assert_eq!(row.changes.len(), 2);
        apply(&mut imported, &row.changes[0].0);
        assert!(imported.hot_cue(HotCuePad::A).is_some());
        assert!(imported.memory_cues().is_empty());
        apply(&mut imported, &row.changes[1].0);
        assert!(
            read(
                stick.path(),
                &collection,
                &HashMap::from([(row.track.identity(), Arc::new(imported))])
            )
            .unwrap()
            .tracks
            .is_empty()
        );
        let unknown = read(stick.path(), &Collection::build(None, &[]), &HashMap::new()).unwrap();
        assert_eq!(unknown.skipped.len(), 1);
    }
}
