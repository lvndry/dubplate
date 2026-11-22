//! The window's side of relinking moved files: the tracks missing since rekordbox last saw them,
//! looking for them among the library's files and then in a folder the DJ points at, applying
//! sure matches at once and asking about the others, and the review card that does the asking.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dubplate_library::{
    LibraryTrack, MissingTrack, Relocation, RelocationConfidence, candidates_from_paths,
    find_relocations, search_folders_for,
};
use gpui::prelude::*;
use gpui::{ClickEvent, Context, FontWeight, MouseButton, PathPromptOptions, div, px};

use super::{DubplateApp, NoticePlace};
use crate::collection::TrackIndex;
use crate::relink::{Missing, with_relocations};
use crate::theme;
use crate::widgets::{Emphasis, button};

/// What a relink found that needs the DJ: matches by length only, and files not found at all.
#[derive(Debug, Default)]
pub(super) struct RelinkReview {
    /// Files with the same name and length but another size, waiting for a yes or a no.
    pending: Vec<Relocation>,
    /// Files found nowhere yet.
    not_found: Vec<MissingTrack>,
    /// How many files this relink has put back so far.
    relinked: usize,
}

impl DubplateApp {
    /// Shows the library which tracks have lost their files.
    pub(super) fn show_missing(&mut self, missing: Vec<Missing>, cx: &mut Context<'_, Self>) {
        self.missing = Arc::new(missing);
        self.refresh_missing_marks(cx);
    }

    fn refresh_missing_marks(&mut self, cx: &mut Context<'_, Self>) {
        let marks: HashSet<TrackIndex> = self.missing.iter().map(|missing| missing.track).collect();
        let relinking = self.relinking;
        self.browser.update(cx, |browser, cx| {
            browser.set_missing(Arc::new(marks), relinking, cx)
        });
    }

    /// Looks for the new place of every missing file, or of one track's, among the library's
    /// files.
    pub(super) fn start_relink(&mut self, only: Option<TrackIndex>, cx: &mut Context<'_, Self>) {
        if self.relinking {
            return;
        }
        let targets: Vec<MissingTrack> = self
            .missing
            .iter()
            .filter(|missing| only.is_none_or(|track| missing.track == track))
            .map(|missing| missing.file.clone())
            .collect();
        if targets.is_empty() {
            return;
        }
        let candidates: Vec<LibraryTrack> = self
            .sources
            .chosen_scans()
            .into_iter()
            .flat_map(|scan| scan.tracks)
            .collect();
        self.relink_review = Some(RelinkReview::default());
        self.search(targets, move || candidates, cx);
    }

    /// Asks for a folder and looks there for the files still not found.
    pub(super) fn search_folder_for_missing(&mut self, cx: &mut Context<'_, Self>) {
        let Some(targets) = self
            .relink_review
            .as_ref()
            .map(|review| review.not_found.clone())
            .filter(|targets| !targets.is_empty())
        else {
            return;
        };
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Search here".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(folders))) = chosen.await else {
                return;
            };
            this.update(cx, |this, cx| {
                let names: HashSet<String> = targets
                    .iter()
                    .filter_map(|missing| missing.path.file_name()?.to_str())
                    .map(str::to_lowercase)
                    .collect();
                this.search(
                    targets,
                    move || candidates_from_paths(&search_folders_for(&names, &folders)),
                    cx,
                );
            })
            .ok();
        })
        .detach();
    }

    /// Finds `targets` among the candidates `gather` reads, off the interface thread.
    fn search(
        &mut self,
        targets: Vec<MissingTrack>,
        gather: impl FnOnce() -> Vec<LibraryTrack> + Send + 'static,
        cx: &mut Context<'_, Self>,
    ) {
        self.relinking = true;
        self.refresh_missing_marks(cx);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let found = cx
                .background_executor()
                .spawn(async move {
                    let candidates = gather();
                    let relocations = find_relocations(&targets, &candidates);
                    (targets, relocations)
                })
                .await;
            this.update(cx, |this, cx| {
                let (targets, relocations) = found;
                this.finish_search(targets, relocations, cx);
            })
            .ok();
        })
        .detach();
    }

    fn finish_search(
        &mut self,
        targets: Vec<MissingTrack>,
        relocations: Vec<Relocation>,
        cx: &mut Context<'_, Self>,
    ) {
        self.relinking = false;
        let (sure, by_length): (Vec<Relocation>, Vec<Relocation>) = relocations
            .into_iter()
            .partition(|relocation| relocation.confidence == RelocationConfidence::SameSize);
        let found: HashSet<&PathBuf> = sure
            .iter()
            .chain(&by_length)
            .map(|relocation| &relocation.missing)
            .collect();
        let not_found: Vec<MissingTrack> = targets
            .iter()
            .filter(|missing| !found.contains(&missing.path))
            .cloned()
            .collect();
        let review = self.relink_review.get_or_insert_with(RelinkReview::default);
        review.relinked += sure.len();
        review.pending.extend(by_length);
        review.not_found = not_found;
        let quiet = review.pending.is_empty() && review.not_found.is_empty();
        let relinked = review.relinked;
        self.apply_relocations(&sure, cx);
        if quiet {
            self.relink_review = None;
            self.show_notice(
                NoticePlace::Window,
                format!(
                    "Relinked {relinked} track{}.",
                    if relinked == 1 { "" } else { "s" }
                ),
                cx,
            );
        }
        self.refresh_missing_marks(cx);
        cx.notify();
    }

    /// Accepts or turns down a match by length.
    pub(super) fn answer_relink(
        &mut self,
        position: usize,
        accept: bool,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(review) = &mut self.relink_review else {
            return;
        };
        if position >= review.pending.len() {
            return;
        }
        let relocation = review.pending.remove(position);
        if accept {
            review.relinked += 1;
            self.apply_relocations(&[relocation], cx);
        } else if let Some(missing) = self
            .missing
            .iter()
            .find(|missing| missing.file.path == relocation.missing)
        {
            review.not_found.push(missing.file.clone());
        }
        cx.notify();
    }

    /// Accepts every match by length.
    pub(super) fn accept_all_relinks(&mut self, cx: &mut Context<'_, Self>) {
        let Some(review) = &mut self.relink_review else {
            return;
        };
        let pending = std::mem::take(&mut review.pending);
        review.relinked += pending.len();
        self.apply_relocations(&pending, cx);
        cx.notify();
    }

    /// Closes the review.
    pub(super) fn close_relink_review(&mut self, cx: &mut Context<'_, Self>) {
        self.relink_review = None;
        cx.notify();
    }

    /// Keeps the new places in Dubplate's settings, points the set at them, and builds the
    /// library again so the tracks play, get their energy and lose "File missing".
    fn apply_relocations(&mut self, accepted: &[Relocation], cx: &mut Context<'_, Self>) {
        if accepted.is_empty() {
            return;
        }
        self.settings.library.relinks = with_relocations(&self.settings.library.relinks, accepted);
        self.save_settings(cx);
        let mut problems = Vec::new();
        if let Some(editor) = &mut self.set_editor {
            for relocation in accepted {
                if let Err(error) = editor.relink(&relocation.missing, &relocation.found) {
                    problems.push(error.to_string());
                }
            }
        }
        if let Some(problem) = problems.into_iter().next() {
            self.show_notice(
                NoticePlace::Window,
                format!("The set could not follow a relinked file: {problem}"),
                cx,
            );
        }
        self.rebuild_collection(cx);
    }
}

/// The review card over the window.
pub(super) fn render(
    app: &DubplateApp,
    review: &RelinkReview,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement {
    let mut card = div()
        .id("relink-card")
        .flex()
        .flex_col()
        .gap(px(12.0))
        .w(px(560.0))
        .max_h(px(620.0))
        .overflow_y_scroll()
        .p(px(20.0))
        .rounded(px(10.0))
        .border_1()
        .border_color(theme::LINE)
        .bg(theme::PANEL)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(|_, _, cx| cx.stop_propagation())
        .child(
            div()
                .text_size(px(15.0))
                .font_weight(FontWeight::MEDIUM)
                .child("Relink moved files"),
        )
        .child(
            div()
                .text_size(theme::SMALL_TEXT_SIZE)
                .text_color(theme::MUTED)
                .child(format!(
                    "{} Dubplate keeps the new places itself and never changes rekordbox.",
                    match review.relinked {
                        0 => "Nothing put back yet.".to_owned(),
                        relinked => format!("{} put back.", count_label(relinked, "track")),
                    }
                )),
        );
    if !review.pending.is_empty() {
        card = card
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_size(theme::SMALL_TEXT_SIZE)
                            .child("Same name and length, but another size. Use these?"),
                    )
                    .child(
                        button("relink-use-all", "Use all", Emphasis::Secondary).on_click(
                            cx.listener(|this, _: &ClickEvent, _, cx| this.accept_all_relinks(cx)),
                        ),
                    ),
            )
            .children(
                review
                    .pending
                    .iter()
                    .enumerate()
                    .map(|(position, relocation)| pending_row(position, relocation, cx)),
            );
    }
    if !review.not_found.is_empty() {
        card = card.child(
            div()
                .flex()
                .justify_between()
                .items_center()
                .gap(px(12.0))
                .child(
                    div()
                        .text_size(theme::SMALL_TEXT_SIZE)
                        .text_color(theme::ATTENTION)
                        .child(format!(
                            "{} not found in your music folders.",
                            count_label(review.not_found.len(), "file")
                        )),
                )
                .child(if app.relinking {
                    div()
                        .text_size(theme::SMALL_TEXT_SIZE)
                        .text_color(theme::MUTED)
                        .child("Looking\u{2026}")
                        .into_any_element()
                } else {
                    button(
                        "relink-search",
                        "Search a folder\u{2026}",
                        Emphasis::Primary,
                    )
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.search_folder_for_missing(cx);
                    }))
                    .into_any_element()
                }),
        );
    }
    card = card.child(
        div().flex().justify_end().child(
            button("relink-done", "Done", Emphasis::Secondary)
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.close_relink_review(cx))),
        ),
    );
    div()
        .id("relink-review")
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(theme::SCRIM)
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.close_relink_review(cx)))
        .child(card)
}

fn pending_row(
    position: usize,
    relocation: &Relocation,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<> {
    div()
        .flex()
        .items_center()
        .gap(px(10.0))
        .py(px(6.0))
        .border_t_1()
        .border_color(theme::ROW_LINE)
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .child(div().truncate().child(file_name(&relocation.found)))
                .child(
                    div()
                        .truncate()
                        .text_size(theme::TINY_TEXT_SIZE)
                        .text_color(theme::MUTED)
                        .child(folder_of(&relocation.found)),
                ),
        )
        .child(
            button(("relink-use", position), "Use", Emphasis::Primary).on_click(cx.listener(
                move |this, _: &ClickEvent, _, cx| this.answer_relink(position, true, cx),
            )),
        )
        .child(
            button(("relink-skip", position), "Skip", Emphasis::Secondary).on_click(cx.listener(
                move |this, _: &ClickEvent, _, cx| this.answer_relink(position, false, cx),
            )),
        )
}

fn count_label(count: usize, noun: &str) -> String {
    format!("{count} {noun}{}", if count == 1 { "" } else { "s" })
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn folder_of(path: &Path) -> String {
    path.parent()
        .map(|folder| folder.display().to_string())
        .unwrap_or_default()
}
