//! A library row's Match cell: where the track ranks after the track the room follows, how its
//! key follows as a coloured chip, the tempo change and the energy change.

use std::path::Path;

use gpui::prelude::*;
use gpui::{AnyElement, Pixels, Rgba, div, px};

use crate::fonts::MONO_FAMILY;
use crate::library_match::{KeyTone, LibraryMatches, TrackMatch};
use crate::theme;

const RANK_WIDTH: Pixels = px(24.0);
const TEMPO_WIDTH: Pixels = px(46.0);
const CHIP_DOT: Pixels = px(6.0);

/// What one row's Match cell shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MatchCell {
    /// The track the others are matched against.
    Reference,
    /// How the track mixes after it.
    Track(TrackMatch),
    /// Nothing to show: no track to match against, or one the catalogue does not hold.
    Empty,
}

impl MatchCell {
    /// The cell of the track whose file is `path`.
    pub fn of(matches: &LibraryMatches, path: &Path) -> Self {
        if matches.is_reference(path) {
            return Self::Reference;
        }
        matches.get(path).copied().map_or(Self::Empty, Self::Track)
    }

    /// The cell in words, for a screen reader.
    pub fn description(self, played: bool) -> String {
        let said = match self {
            Self::Reference => "The track matched against".to_owned(),
            Self::Track(found) => found.description(),
            Self::Empty => String::new(),
        };
        if played {
            format!("{said}. Played this session")
        } else {
            said
        }
    }

    /// The cell's contents.
    pub fn render(self) -> AnyElement {
        match self {
            Self::Reference => div()
                .text_size(theme::TINY_TEXT_SIZE)
                .text_color(theme::MUTED)
                .child("On deck \u{b7} matched against")
                .into_any_element(),
            Self::Track(found) => track_match(found).into_any_element(),
            Self::Empty => div().into_any_element(),
        }
    }
}

fn track_match(found: TrackMatch) -> impl IntoElement {
    let rank = found.rank.map(|rank| rank.to_string()).unwrap_or_default();
    let tempo = found
        .tempo_change_percent
        .map(|percent| format!("{percent:+.1}%").replace('-', "\u{2212}"))
        .unwrap_or_default();
    let energy = found
        .energy_change
        .map(|change| format!("E{change:+.0}").replace('-', "\u{2212}"))
        .unwrap_or_default();
    div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .min_w_0()
        .font_family(MONO_FAMILY)
        .text_size(theme::TINY_TEXT_SIZE)
        .text_color(theme::MUTED)
        .child(div().flex_none().w(RANK_WIDTH).child(rank))
        .children(found.key.map(|key| {
            div()
                .flex()
                .flex_1()
                .items_center()
                .gap(px(5.0))
                .min_w_0()
                .font_family(crate::fonts::UI_FAMILY)
                .text_color(theme::TEXT)
                .child(
                    div()
                        .flex_none()
                        .size(CHIP_DOT)
                        .rounded_full()
                        .bg(tone_colour(key.tone())),
                )
                .child(div().truncate().min_w_0().child(key.label()))
        }))
        .child(
            div()
                .flex_none()
                .w(TEMPO_WIDTH)
                .flex()
                .justify_end()
                .when(found.is_far_in_tempo(), |tempo| {
                    tempo.text_color(theme::ATTENTION)
                })
                .child(tempo),
        )
        .child(div().flex_none().child(energy))
}

fn tone_colour(tone: KeyTone) -> Rgba {
    match tone {
        KeyTone::Smooth => theme::KEY_SMOOTH,
        KeyTone::Lift => theme::KEY_LIFT,
        KeyTone::Clash => theme::KEY_CLASH,
    }
}
