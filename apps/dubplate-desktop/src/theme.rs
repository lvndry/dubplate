//! The Graphite look: every colour, font and size the views draw with, defined once.

use dubplate_engine::DeckId;
use gpui::component::{Theme as ComponentTheme, ThemeMode};
use gpui::{App, Pixels, Rgba, px};

use crate::fonts::{MONO_FAMILY, UI_FAMILY};

/// An opaque colour from its `0xRRGGBB` value.
const fn hex(value: u32) -> Rgba {
    with_alpha(value, 1.0)
}

/// A colour from its `0xRRGGBB` value and an opacity from 0.0 to 1.0.
const fn with_alpha(value: u32, alpha: f32) -> Rgba {
    Rgba {
        r: ((value >> 16) & 0xff) as f32 / 255.0,
        g: ((value >> 8) & 0xff) as f32 / 255.0,
        b: (value & 0xff) as f32 / 255.0,
        a: alpha,
    }
}

/// White at an opacity, for hairlines and tracks drawn over the dark surfaces.
const fn white_alpha(alpha: f32) -> Rgba {
    with_alpha(0xffffff, alpha)
}

/// Behind the waveforms, the mixer and the browser.
pub const BACKGROUND: Rgba = hex(0x0B0C0E);
/// Behind the deck panels and the search field.
pub const PANEL: Rgba = hex(0x111215);
/// Buttons, pads and the selected crate.
pub const RAISED: Rgba = hex(0x191A1E);
/// Hairline borders.
pub const LINE: Rgba = white_alpha(0.07);
/// Primary text.
pub const TEXT: Rgba = hex(0xEEEFF1);
/// Labels, artists and secondary numbers.
pub const MUTED: Rgba = hex(0x8A8F98);
/// The playhead across both zoomed waveforms.
pub const PLAYHEAD: Rgba = hex(0xFFFFFF);
/// Beat lines on the zoomed waveforms.
pub const BEAT_LINE: Rgba = white_alpha(0.05);
/// Downbeat lines on the zoomed waveforms.
pub const DOWNBEAT_LINE: Rgba = white_alpha(0.1);
/// How much of its colour the part of a waveform already played keeps.
pub const PLAYED_OPACITY: f32 = 0.32;
/// The low band of a 3-band waveform.
pub const BAND_LOW: Rgba = hex(0x6E86E8);
/// The mid band of a 3-band waveform.
pub const BAND_MID: Rgba = hex(0xD7AE6E);
/// The high band of a 3-band waveform.
pub const BAND_HIGH: Rgba = hex(0xECEAE4);
/// Text on a deck colour, such as the deck badge.
pub const ON_DECK: Rgba = hex(0x0B0C0E);
/// Things that are fine, such as a library that loaded.
pub const OK: Rgba = hex(0x7FD1A4);
/// Things that need the DJ's attention before the set is ready, such as a missing cue.
pub const ATTENTION: Rgba = hex(0xF2C46D);
/// Problems the DJ has to act on.
pub const ALERT: Rgba = hex(0xF07167);
/// The track of sliders and the empty part of the effect amount.
pub const TRACK: Rgba = white_alpha(0.1);
/// Slider and fader thumbs.
pub const THUMB: Rgba = hex(0xE6E7E9);
/// A lit meter segment.
pub const METER_ON: Rgba = white_alpha(0.72);
/// An unlit meter segment.
pub const METER_OFF: Rgba = white_alpha(0.07);
/// A lit meter segment near clipping.
pub const METER_HOT: Rgba = hex(0xF5B78E);
/// Behind the selected browser row.
pub const SELECTED_ROW: Rgba = white_alpha(0.05);
/// Between browser rows.
pub const ROW_LINE: Rgba = white_alpha(0.045);
/// Behind the cue letters on the zoomed waveforms.
pub const CUE_FLAG: Rgba = hex(0xEEEFF1);
/// Behind the help overlay.
pub const SCRIM: Rgba = with_alpha(0x000000, 0.55);

/// The colours of the hot cue pads A to H.
pub const HOT_CUE_TINTS: [Rgba; 8] = [
    hex(0xF5B78E),
    hex(0xF2D48A),
    hex(0xB9DE9E),
    hex(0x8FD3C8),
    hex(0xA3B6FF),
    hex(0xC7B1F5),
    hex(0xF0A8C8),
    hex(0xD7D9DD),
];

/// The colour that marks everything belonging to a deck.
pub const fn deck_colour(deck: DeckId) -> Rgba {
    match deck {
        DeckId::A => hex(0xF5B78E),
        DeckId::B => hex(0xA3B6FF),
    }
}

/// `colour` at `opacity` times its own opacity.
pub fn faded(colour: Rgba, opacity: f32) -> Rgba {
    Rgba {
        a: colour.a * opacity,
        ..colour
    }
}

/// Body text size.
pub const TEXT_SIZE: Pixels = px(13.0);
/// Size of labels and table headers.
pub const SMALL_TEXT_SIZE: Pixels = px(12.0);
/// Size of the smallest labels, such as counts and pad times.
pub const TINY_TEXT_SIZE: Pixels = px(11.0);
/// Corner radius of buttons and fields.
pub const CONTROL_RADIUS: Pixels = px(7.0);
/// Corner radius of pads and small chips.
pub const SMALL_RADIUS: Pixels = px(6.0);
/// Space between a panel's edge and its content, left and right.
pub const GUTTER: Pixels = px(14.0);

/// Gives the component library the Graphite colours and fonts, so the search field matches the
/// rest of the window.
pub fn apply_to_components(cx: &mut App) {
    ComponentTheme::change(ThemeMode::Dark, None, cx);
    ComponentTheme::update(cx, |theme| {
        theme.font_family = UI_FAMILY.into();
        theme.mono_font_family = MONO_FAMILY.into();
        theme.font_size = TEXT_SIZE;
        theme.radius = CONTROL_RADIUS;
        theme.shadow = false;
        let colours = &mut theme.colors;
        colours.background = BACKGROUND.into();
        colours.foreground = TEXT.into();
        colours.border = LINE.into();
        colours.input = LINE.into();
        colours.muted = RAISED.into();
        colours.muted_foreground = MUTED.into();
        colours.popover = PANEL.into();
        colours.popover_foreground = TEXT.into();
        colours.accent = RAISED.into();
        colours.accent_foreground = TEXT.into();
        colours.ring = LINE.into();
        colours.caret = TEXT.into();
        colours.selection = faded(deck_colour(DeckId::B), 0.35).into();
        colours.scrollbar = gpui::transparent_black();
        colours.scrollbar_thumb = faded(MUTED, 0.45).into();
        colours.scrollbar_thumb_hover = faded(MUTED, 0.7).into();
    });
}
