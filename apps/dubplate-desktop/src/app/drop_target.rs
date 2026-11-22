//! Decks take tracks dragged from the browser and audio files dragged from Finder, anywhere on
//! their panel or waveform lane.

use dubplate_engine::DeckId;
use gpui::prelude::*;
use gpui::{Context, Div, ExternalPaths, Stateful};

use super::DubplateApp;
use crate::browser::DraggedTrack;
use crate::loading::LoadSource;
use crate::theme;

/// How strongly a deck lights up in its colour while a track is dragged over it.
const HIGHLIGHT_OPACITY: f32 = 0.16;

/// Makes `element` load what is dropped on it on `deck`, lit in the deck's colour while a track
/// or file is held over it.
pub(super) fn accept_drops(
    element: Stateful<Div>,
    deck: DeckId,
    cx: &mut Context<'_, DubplateApp>,
) -> Stateful<Div> {
    let highlight = theme::faded(theme::deck_colour(deck), HIGHLIGHT_OPACITY);
    element
        .drag_over::<DraggedTrack>(move |style, _, _, _| style.bg(highlight))
        .drag_over::<ExternalPaths>(move |style, _, _, _| style.bg(highlight))
        .on_drop(cx.listener(move |this, dragged: &DraggedTrack, _, cx| {
            this.load(deck, LoadSource::Collection(dragged.track), cx);
        }))
        .on_drop(cx.listener(move |this, paths: &ExternalPaths, _, cx| {
            let paths = paths.paths();
            if paths.len() > 1 {
                this.show_notice(
                    super::notice::NoticePlace::Window,
                    format!("Only the first of the {} dropped files was loaded, the rest were not", paths.len()),
                    cx,
                );
            }
            if let Some(path) = paths.first() {
                this.load(deck, LoadSource::File(path.clone()), cx);
            }
        }))
}
