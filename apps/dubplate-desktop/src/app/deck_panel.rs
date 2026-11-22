//! A deck: what is loaded, the whole-track waveform, the transport and tempo, and the hot cue
//! pads.

use dubplate_engine::{DeckId, SyncMode, TempoState};
use gpui::prelude::*;
use gpui::{ClickEvent, Context, FontWeight, MouseButton, Pixels, SharedString, div, px, svg};

use super::drop_target::accept_drops;
use super::overview::{self, ENDING_SECONDS};
use super::slider::{self, SMALL_THUMB};
use super::{DeckFrame, DubplateApp, pad_grid};
use crate::analysis::{AnalysisState, key_cell, track_key};
use crate::assets::{PAUSE_ICON_PATH, PLAY_ICON_PATH};
use crate::collection::{CollectionTrack, TrackOrigin, clock};
use crate::commands::{Command, Press};
use crate::controls::{Slider, tempo_range_label};
use crate::deck::DeckContent;
use crate::deck::deck_letter;
use crate::fonts::MONO_FAMILY;
use crate::pads::semitones_label;
use crate::theme;
use crate::widgets::{InteractiveControl, tooltip};

const BUTTON_HEIGHT: Pixels = px(34.0);

pub(super) fn render(
    app: &DubplateApp,
    deck: DeckId,
    frame: &DeckFrame,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement {
    let entry = app.deck(deck).track.as_deref();
    let panel = div()
        .id(("deck", deck.index()))
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .gap(px(10.0))
        .px(theme::GUTTER)
        .py(px(12.0))
        .bg(theme::PANEL)
        .child(title_row(app, deck, frame, entry))
        .child(overview::render(app, deck, frame, cx))
        .child(transport(app, deck, frame, cx))
        .child(pad_grid::render(app, deck, cx));
    accept_drops(panel, deck, cx)
}

fn title_row(
    app: &DubplateApp,
    deck: DeckId,
    frame: &DeckFrame,
    entry: Option<&CollectionTrack>,
) -> impl IntoElement {
    let notice = app.deck_notice(deck);
    let (title, subtitle, subtitle_colour): (SharedString, SharedString, _) =
        match (&app.deck(deck).content, entry) {
            (_, entry) if notice.is_some() => (
                entry.map_or_else(|| "No track".into(), |entry| entry.row.title.clone()),
                notice.cloned().unwrap_or_default(),
                theme::ALERT,
            ),
            (DeckContent::Failed(message), Some(entry)) => (
                entry.row.title.clone(),
                message.clone().into(),
                theme::ALERT,
            ),
            (DeckContent::Loading, Some(entry)) => (
                entry.row.title.clone(),
                "Loading\u{2026}".into(),
                theme::MUTED,
            ),
            (_, Some(entry)) if entry.origin == TrackOrigin::File => (
                entry.row.title.clone(),
                "Audio file, not in rekordbox".into(),
                theme::MUTED,
            ),
            (_, Some(entry)) => (
                entry.row.title.clone(),
                entry.row.artist.clone(),
                theme::MUTED,
            ),
            (_, None) => ("No track".into(), "".into(), theme::MUTED),
        };
    let tempo = frame.state.tempo;
    let playing_bpm = app
        .tempo_state(deck)
        .and_then(|state| state.bpm)
        .or_else(|| {
            entry
                .and_then(|entry| entry.track.bpm)
                .map(|bpm| bpm * tempo)
        });
    let analysing = playing_bpm.is_none()
        && entry
            .and_then(|entry| entry.track.file_path.as_deref())
            .is_some_and(|path| matches!(app.analyses.state(path), AnalysisState::Waiting));
    let bpm = playing_bpm.map_or_else(|| "--.--".to_owned(), |bpm| format!("{bpm:.2}"));
    let pitch = pitch_label(tempo);
    let ending = frame
        .duration_seconds
        .is_some_and(|duration| duration - frame.playhead_seconds <= ENDING_SECONDS);
    let remaining = frame.duration_seconds.map_or_else(
        || "\u{2212}-:--".to_owned(),
        |duration| {
            format!(
                "\u{2212}{}",
                clock((duration - frame.playhead_seconds).max(0.0))
            )
        },
    );
    let key_shift = app.key_shift(deck);
    let key: SharedString = match entry {
        Some(entry) => {
            let analysis = entry
                .track
                .file_path
                .as_deref()
                .map_or(AnalysisState::Waiting, |path| app.analyses.state(path));
            if key_shift == 0 {
                key_cell(entry, analysis)
            } else {
                let shifted = track_key(entry, analysis)
                    .map(|key| key.transposed(key_shift as i8).camelot())
                    .unwrap_or_default();
                format!("{shifted} {}", semitones_label(key_shift)).into()
            }
        }
        None => SharedString::default(),
    };
    div()
        .id(("deck-metadata", deck.index()))
        .role(gpui::Role::Group)
        .aria_label(format!("Deck {}: {title}. {subtitle}. {bpm} BPM. Key {key}. Pitch {pitch}. Remaining {remaining}.", deck_letter(deck)))
        .flex()
        .justify_between()
        .items_start()
        .gap(px(12.0))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(10.0))
                .min_w_0()
                .child(deck_badge(deck))
                .when(app.space_deck() == deck, |row| {
                    row.child(
                        div()
                            .id(("space-deck", deck.index()))
                            .flex_none()
                            .px(px(4.0))
                            .rounded(px(3.0))
                            .border_1()
                            .border_color(theme::LINE)
                            .font_family(MONO_FAMILY)
                            .text_size(theme::TINY_TEXT_SIZE)
                            .text_color(theme::MUTED)
                            .tooltip(tooltip(
                                "Space plays or pauses this deck: the one you can hear",
                            ))
                            .child("Space"),
                    )
                })
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .min_w_0()
                        .child(
                            div()
                                .text_size(px(14.0))
                                .font_weight(FontWeight::MEDIUM)
                                .child(title),
                        )
                        .child(div().text_color(subtitle_colour).child(subtitle)),
                ),
        )
        .child(
            div()
                .flex()
                .flex_none()
                .items_baseline()
                .gap(px(12.0))
                .font_family(MONO_FAMILY)
                .text_size(theme::SMALL_TEXT_SIZE)
                .text_color(theme::MUTED)
                .child(key)
                .child(pitch)
                .child(
                    div()
                        .when(ending, |time| time.text_color(theme::ATTENTION))
                        .child(remaining),
                )
                .child(if analysing {
                    div()
                        .font_family(crate::fonts::UI_FAMILY)
                        .text_size(theme::SMALL_TEXT_SIZE)
                        .text_color(theme::MUTED)
                        .child("Analysing\u{2026}")
                } else {
                    div()
                        .font_family(crate::fonts::UI_FAMILY)
                        .text_size(px(26.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme::TEXT)
                        .child(bpm)
                }),
        )
}

/// The tempo as a CDJ shows it, to the hundredth of a percent with a true minus sign, so two
/// decks can be matched by their numbers: 0.1% at 122 BPM is already 0.12 BPM.
fn pitch_label(tempo: f64) -> String {
    let percent = (tempo - 1.0) * 100.0;
    let percent = if percent.abs() < 0.005 { 0.0 } else { percent };
    format!("{percent:+.2}%").replace('-', "\u{2212}")
}

fn deck_badge(deck: DeckId) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(24.0))
        .rounded(theme::SMALL_RADIUS)
        .bg(theme::deck_colour(deck))
        .text_color(theme::ON_DECK)
        .font_family(MONO_FAMILY)
        .text_size(theme::SMALL_TEXT_SIZE)
        .font_weight(FontWeight::SEMIBOLD)
        .child(deck_letter(deck))
}

fn transport(
    app: &DubplateApp,
    deck: DeckId,
    frame: &DeckFrame,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement {
    let playing = frame.state.playing;
    let colour = theme::deck_colour(deck);
    let tempo = app.tempo_state(deck);
    div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(
            button(("cue", deck.index()))
                .px(px(12.0))
                .bg(theme::RAISED)
                .border_1()
                .border_color(theme::LINE)
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.perform(Command::Cue(deck, Press::Tap), window, cx);
                }))
                .child("Cue"),
        )
        .child(
            button(("play", deck.index()))
                .accessible_toggle(format!("Deck {} Play or pause", deck_letter(deck)), playing)
                .w(px(44.0))
                .when(playing, |play| play.bg(colour).text_color(theme::ON_DECK))
                .when(!playing, |play| {
                    play.bg(theme::RAISED).border_1().border_color(theme::LINE)
                })
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.perform(Command::TogglePlay(deck), window, cx);
                }))
                .child(
                    svg()
                        .path(if playing {
                            PAUSE_ICON_PATH
                        } else {
                            PLAY_ICON_PATH
                        })
                        .size(px(13.0))
                        .text_color(if playing { theme::ON_DECK } else { theme::TEXT }),
                ),
        )
        .child(toggle(
            ("sync", deck.index()),
            "Sync",
            sync_lit(tempo),
            colour,
            Command::ToggleSync(deck),
            cx,
        ))
        .child(toggle(
            ("master", deck.index()),
            "Master",
            Lit::when(tempo.is_some_and(|state| state.is_master)),
            colour,
            Command::MakeTempoMaster(deck),
            cx,
        ))
        .child(toggle(
            ("key-lock", deck.index()),
            "Key lock",
            Lit::when(app.key_lock(deck)),
            colour,
            Command::ToggleKeyLock(deck),
            cx,
        ))
        .child(toggle(
            ("slip", deck.index()),
            "Slip",
            Lit::when(app.slip(deck)),
            colour,
            Command::ToggleSlip(deck),
            cx,
        ))
        .child(
            div()
                .flex()
                .flex_1()
                .items_center()
                .gap(px(10.0))
                .ml(px(10.0))
                .child({
                    let waiting = app.tempo_slider_view(deck).waiting_fader.is_some();
                    div()
                        .text_size(theme::SMALL_TEXT_SIZE)
                        .text_color(if waiting {
                            theme::ATTENTION
                        } else {
                            theme::MUTED
                        })
                        .child(if waiting {
                            "Move to take over"
                        } else {
                            "Tempo"
                        })
                })
                .child(
                    slider::render(
                        app,
                        Slider::Tempo(deck),
                        SMALL_THUMB,
                        Some((0.5, colour)),
                        cx,
                    )
                    .flex_1(),
                )
                .child(
                    div()
                        .w(px(52.0))
                        .flex()
                        .justify_end()
                        .font_family(MONO_FAMILY)
                        .text_size(theme::SMALL_TEXT_SIZE)
                        .text_color(theme::MUTED)
                        .child(tempo_range_label(app.tempo_range(deck))),
                ),
        )
}

/// How a transport button is lit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lit {
    /// Off.
    Off,
    /// On but waiting, as Sync is until a tempo master plays: outlined faintly, its label in
    /// the deck's colour.
    Armed,
    /// On.
    On,
}

impl Lit {
    fn when(on: bool) -> Self {
        if on { Self::On } else { Self::Off }
    }
}

/// Sync is on while the deck follows the tempo master or is the master, and armed while it
/// waits for a master to play.
fn sync_lit(tempo: Option<TempoState>) -> Lit {
    match tempo {
        Some(state) if state.sync != SyncMode::Off => {
            if state.following || state.is_master {
                Lit::On
            } else {
                Lit::Armed
            }
        }
        _ => Lit::Off,
    }
}

/// A transport button that stays lit in the deck's colour while its setting is on, such as
/// Sync and Master.
fn toggle(
    id: (&'static str, usize),
    label: &'static str,
    lit: Lit,
    colour: gpui::Rgba,
    command: Command,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<> {
    button(id)
        .accessible_toggle(
            format!(
                "Deck {} {label}",
                deck_letter(if id.1 == 0 { DeckId::A } else { DeckId::B })
            ),
            lit != Lit::Off,
        )
        .px(px(10.0))
        .border_1()
        .map(|toggle| match lit {
            Lit::On => toggle.border_color(colour).text_color(theme::TEXT),
            Lit::Armed => toggle
                .border_color(theme::faded(colour, 0.45))
                .text_color(colour),
            Lit::Off => toggle
                .border_color(theme::LINE)
                .text_color(theme::MUTED)
                .hover(|toggle| toggle.text_color(theme::TEXT)),
        })
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            this.perform(command, window, cx);
        }))
        .child(label)
}

fn button(id: (&'static str, usize)) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .accessible_button(format!(
            "Deck {} {}",
            if id.1 == 0 { "A" } else { "B" },
            id.0
        ))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .h(BUTTON_HEIGHT)
        .rounded(theme::CONTROL_RADIUS)
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tempo_reads_to_the_hundredth_of_a_percent() {
        assert_eq!(pitch_label(1.0), "+0.00%");
        assert_eq!(pitch_label(1.0125), "+1.25%");
        assert_eq!(pitch_label(0.9998), "\u{2212}0.02%");
        assert_eq!(pitch_label(0.99999), "+0.00%");
    }
}
