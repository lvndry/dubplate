//! Notices: the short messages about something the DJ asked for that did not happen, at the
//! bottom of the window or on a deck, some with a button that fixes the cause.

use std::time::Duration;

use dubplate_engine::DeckId;
use gpui::prelude::*;
use gpui::{ClickEvent, Context, Pixels, SharedString, div, px};

use super::DubplateApp;
use crate::collection::TrackIndex;
use crate::theme;
use crate::widgets::{Emphasis, button};

/// How long a notice stays on screen.
const NOTICE_DURATION: Duration = Duration::from_secs(6);
/// How long a notice that offers an action stays, long enough to reach for its button.
const ACTION_NOTICE_DURATION: Duration = Duration::from_secs(12);
/// How wide a notice grows before its text wraps.
const NOTICE_MAX_WIDTH: Pixels = px(560.0);

/// Where a notice shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NoticePlace {
    /// At the bottom of the window.
    Window,
    /// On a deck.
    Deck(DeckId),
}

/// What a notice offers to do about what it says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NoticeAction {
    /// Look for the moved file of one missing track, or of every missing track.
    Relink(Option<TrackIndex>),
}

impl NoticeAction {
    fn label(self) -> &'static str {
        match self {
            Self::Relink(_) => "Relink",
        }
    }
}

/// The notice showing, if any. A newer notice replaces an older one, and each one only clears
/// itself when its time is up, never its successor.
#[derive(Debug, Default)]
pub(super) struct Notices {
    current: Option<Notice>,
    posted: u64,
}

#[derive(Debug)]
struct Notice {
    id: u64,
    place: NoticePlace,
    text: SharedString,
    action: Option<NoticeAction>,
}

impl Notices {
    /// Shows a notice in place of the one showing, and returns its id for [`Self::expire`].
    fn post(
        &mut self,
        place: NoticePlace,
        text: SharedString,
        action: Option<NoticeAction>,
    ) -> u64 {
        self.posted += 1;
        self.current = Some(Notice {
            id: self.posted,
            place,
            text,
            action,
        });
        self.posted
    }

    /// Clears the notice `id` if it is still the one showing. Returns whether it was.
    fn expire(&mut self, id: u64) -> bool {
        let showing = self.current.as_ref().is_some_and(|notice| notice.id == id);
        if showing {
            self.current = None;
        }
        showing
    }

    /// Whether a notice is showing.
    pub(super) fn is_showing(&self) -> bool {
        self.current.is_some()
    }

    /// Clears the notice showing.
    pub(super) fn dismiss(&mut self) {
        self.current = None;
    }

    /// The text of the notice showing at `place`.
    fn text_at(&self, place: NoticePlace) -> Option<&SharedString> {
        self.current
            .as_ref()
            .filter(|notice| notice.place == place)
            .map(|notice| &notice.text)
    }
}

impl DubplateApp {
    /// Shows a notice for a few seconds, or until Escape closes it.
    pub(super) fn show_notice(
        &mut self,
        place: NoticePlace,
        text: impl Into<SharedString>,
        cx: &mut Context<'_, Self>,
    ) {
        self.post_notice(place, text.into(), None, NOTICE_DURATION, cx);
    }

    /// Shows a notice for `duration`, or until Escape closes it.
    pub(super) fn show_notice_for(
        &mut self,
        place: NoticePlace,
        text: impl Into<SharedString>,
        duration: Duration,
        cx: &mut Context<'_, Self>,
    ) {
        self.post_notice(place, text.into(), None, duration, cx);
    }

    /// The message showing on `deck`, if any.
    pub(super) fn deck_notice(&self, deck: DeckId) -> Option<&SharedString> {
        self.notices.text_at(NoticePlace::Deck(deck))
    }

    /// Says that a library track was not loaded because its file is missing, with a Relink
    /// button when its file moved and Dubplate can look for it.
    pub(super) fn report_missing_file(
        &mut self,
        text: impl Into<SharedString>,
        tracks: &[TrackIndex],
        cx: &mut Context<'_, Self>,
    ) {
        let mut relinkable = tracks
            .iter()
            .copied()
            .filter(|track| self.missing.iter().any(|missing| missing.track == *track));
        let action = match (relinkable.next(), relinkable.next()) {
            (None, _) => None,
            (Some(track), None) => Some(NoticeAction::Relink(Some(track))),
            (Some(_), Some(_)) => Some(NoticeAction::Relink(None)),
        };
        let text: SharedString = text.into();
        match action {
            Some(action) => self.post_notice(
                NoticePlace::Window,
                text,
                Some(action),
                ACTION_NOTICE_DURATION,
                cx,
            ),
            None => self.show_notice(
                NoticePlace::Window,
                format!(
                    "{text} Add the folder it is in to the library, or take it out of the set."
                ),
                cx,
            ),
        }
    }

    fn post_notice(
        &mut self,
        place: NoticePlace,
        text: SharedString,
        action: Option<NoticeAction>,
        duration: Duration,
        cx: &mut Context<'_, Self>,
    ) {
        let id = self.notices.post(place, text, action);
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(duration).await;
            this.update(cx, |this, cx| {
                if this.notices.expire(id) {
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Carries out what a notice's button offers and closes the notice.
    fn take_notice_action(&mut self, action: NoticeAction, cx: &mut Context<'_, Self>) {
        self.notices.dismiss();
        match action {
            NoticeAction::Relink(track) => self.start_relink(track, cx),
        }
        cx.notify();
    }
}

/// The notice at the bottom of the window, with its button.
pub(super) fn render_toast(
    app: &DubplateApp,
    cx: &mut Context<'_, DubplateApp>,
) -> Option<impl IntoElement + use<>> {
    let notice = app
        .notices
        .current
        .as_ref()
        .filter(|notice| notice.place == NoticePlace::Window)?;
    let action = notice.action;
    Some(
        div()
            .absolute()
            .bottom(px(16.0))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .max_w(NOTICE_MAX_WIDTH)
                    .px(px(12.0))
                    .py(px(8.0))
                    .rounded(px(8.0))
                    .border_1()
                    .border_color(theme::LINE)
                    .bg(theme::RAISED)
                    .text_color(theme::TEXT)
                    .child(div().flex_1().min_w_0().child(notice.text.clone()))
                    .children(action.map(|action| {
                        button("notice-action", action.label(), Emphasis::Primary).on_click(
                            cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.take_notice_action(action, cx);
                            }),
                        )
                    })),
            ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_old_notice_running_out_never_clears_a_newer_one() {
        let mut notices = Notices::default();
        let first = notices.post(NoticePlace::Window, "First".into(), None);
        let second = notices.post(NoticePlace::Deck(DeckId::A), "Second".into(), None);
        assert!(!notices.expire(first));
        assert_eq!(
            notices
                .text_at(NoticePlace::Deck(DeckId::A))
                .map(AsRef::as_ref),
            Some("Second")
        );
        assert_eq!(notices.text_at(NoticePlace::Window), None);
        assert!(notices.expire(second));
        assert!(!notices.is_showing());
    }
}
