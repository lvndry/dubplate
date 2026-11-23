//! The stems strip of a deck: one row with a toggle and a small level knob for the drums, the
//! bass, the melody and the vocals, and the Instrumental and Acapella shortcuts.

use dubplate_engine::{DeckId, Stem};
use dubplate_stems::StemsStatus;
use gpui::prelude::*;
use gpui::{ClickEvent, Context, MouseButton, Pixels, Rgba, SharedString, div, px};

use super::{DubplateApp, knob};
use crate::commands::{Command, StemAction};
use crate::controls::Control;
use crate::fonts::MONO_FAMILY;
use crate::stems::stems_cell;
use crate::theme;
use crate::widgets::InteractiveControl;

const ROW_HEIGHT: Pixels = px(22.0);

/// How much room the strip takes: a narrow window tightens it so the whole row, Instrumental
/// and Acapella included, stays on the deck.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fit {
    Full,
    Compact,
}

impl Fit {
    fn gap(self) -> Pixels {
        match self {
            Fit::Full => px(6.0),
            Fit::Compact => px(3.0),
        }
    }

    fn knob_size(self) -> Pixels {
        match self {
            Fit::Full => px(18.0),
            Fit::Compact => px(16.0),
        }
    }

    fn status_width(self) -> Pixels {
        match self {
            Fit::Full => px(84.0),
            Fit::Compact => px(58.0),
        }
    }

    fn toggle_padding(self) -> Pixels {
        match self {
            Fit::Full => px(6.0),
            Fit::Compact => px(4.0),
        }
    }
}

/// The strip of `deck`. Without stems it says where they stand and offers to separate the
/// track; the toggles wait, dashed, until the stems play.
pub(super) fn render(
    app: &DubplateApp,
    deck: DeckId,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<> {
    let colour = theme::deck_colour(deck);
    let playing_stems = app.deck_has_stems(deck);
    let settings = app.deck_stems(deck);
    let status = app.deck_stems_status(deck).clone();
    let fit = if app.narrow_window() {
        Fit::Compact
    } else {
        Fit::Full
    };
    div()
        .id(("stems-strip", deck.index()))
        .flex()
        .items_center()
        .gap(fit.gap())
        .h(ROW_HEIGHT)
        .font_family(MONO_FAMILY)
        .text_size(theme::TINY_TEXT_SIZE)
        .child(status_label(app, deck, &status, playing_stems, fit, cx))
        .children(Stem::ALL.map(|stem| {
            div()
                .flex()
                .items_center()
                .gap(fit.gap() / 3.0)
                .child(toggle(
                    deck,
                    ("stem", stem.index(), deck.index()),
                    stem.label(),
                    settings.is_heard(stem),
                    playing_stems,
                    colour,
                    StemAction::Toggle(stem),
                    fit,
                    cx,
                ))
                .child(knob::render(
                    app,
                    Control::StemLevel(deck, stem),
                    fit.knob_size(),
                    colour,
                    cx,
                ))
        }))
        .child(div().flex_1())
        .child(toggle(
            deck,
            ("stems-instrumental", 4, deck.index()),
            "Inst",
            settings.is_without(Stem::Vocals),
            playing_stems,
            colour,
            StemAction::Instrumental,
            fit,
            cx,
        ))
        .child(toggle(
            deck,
            ("stems-acapella", 5, deck.index()),
            "Acap",
            settings.is_solo(Stem::Vocals),
            playing_stems,
            colour,
            StemAction::Acapella,
            fit,
            cx,
        ))
}

/// "Stems" when they play; otherwise where they stand, and a click separates the track first.
fn status_label(
    app: &DubplateApp,
    deck: DeckId,
    status: &StemsStatus,
    playing_stems: bool,
    fit: Fit,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<> {
    let path = app
        .deck(deck)
        .track
        .as_ref()
        .and_then(|track| track.track.file_path.clone());
    let separable = !playing_stems
        && path.is_some()
        && matches!(status, StemsStatus::None | StemsStatus::Failed(_));
    let text: SharedString = if playing_stems {
        "Stems".into()
    } else {
        match status {
            StemsStatus::None | StemsStatus::Failed(_) if path.is_some() => "Separate".into(),
            StemsStatus::None | StemsStatus::Failed(_) => "Stems".into(),
            StemsStatus::Ready(_) => "Loading".into(),
            StemsStatus::Queued | StemsStatus::Separating(_) => {
                format!("Stems {}", stems_cell(status)).into()
            }
        }
    };
    div()
        .id(("stems-status", deck.index()))
        .w(fit.status_width())
        .flex_none()
        .truncate()
        .text_color(theme::active().muted)
        .when_some(path.filter(|_| separable), |label, path| {
            label
                .cursor_pointer()
                .hover(|label| label.text_color(theme::active().text))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.separate_stems_now(path.clone(), cx);
                }))
        })
        .child(text)
}

#[allow(clippy::too_many_arguments)]
fn toggle(
    deck: DeckId,
    id: (&'static str, usize, usize),
    label: &'static str,
    on: bool,
    enabled: bool,
    colour: Rgba,
    action: StemAction,
    fit: Fit,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<> {
    let command = Command::Stems(deck, action);
    div()
        .id(format!("{0}-{1}-{2}", id.0, id.1, id.2))
        .when(enabled, |button| {
            button.accessible_toggle(
                format!("Deck {} {label}", crate::deck::deck_letter(deck)),
                on,
            )
        })
        .flex()
        .items_center()
        .h(ROW_HEIGHT)
        .px(fit.toggle_padding())
        .rounded(px(5.0))
        .border_1()
        .map(|button| {
            if !enabled {
                button
                    .border_dashed()
                    .border_color(theme::active().line)
                    .text_color(theme::active().muted)
            } else if on {
                button.border_color(colour).text_color(theme::active().text)
            } else {
                button
                    .border_color(theme::active().line)
                    .text_color(theme::active().muted)
                    .line_through()
                    .hover(|button| button.text_color(theme::active().text))
            }
        })
        .when(enabled, |button| {
            button
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.perform(command, window, cx)
                }))
        })
        .child(label)
}

#[cfg(test)]
mod tests {
    use dubplate_engine::StemSettings;

    use super::*;

    fn plays_every_stem(settings: StemSettings) -> bool {
        Stem::ALL.into_iter().all(|stem| settings.is_heard(stem))
    }

    #[test]
    fn the_shortcuts_toggle_back_to_every_stem() {
        let instrumental = StemAction::Instrumental.apply(StemSettings::UNTOUCHED);
        assert!(instrumental.is_without(Stem::Vocals));
        assert!(plays_every_stem(
            StemAction::Instrumental.apply(instrumental)
        ));
        let acapella = StemAction::Acapella.apply(instrumental);
        assert!(acapella.is_solo(Stem::Vocals));
        assert!(plays_every_stem(StemAction::Acapella.apply(acapella)));
        let groove = StemAction::Groove.apply(StemSettings::UNTOUCHED);
        assert!(groove.is_heard(Stem::Drums) && groove.is_heard(Stem::Bass));
        assert!(!groove.is_heard(Stem::Vocals) && !groove.is_heard(Stem::Melody));
        assert!(plays_every_stem(StemAction::Groove.apply(groove)));
    }
}
