//! The looks Dubplate can wear, Graphite first: every colour the views draw with, one value per
//! theme, and the fonts and sizes they share. The DJ picks a theme in Settings; it switches at
//! once and is remembered.

use std::sync::atomic::{AtomicUsize, Ordering};

use dubplate_engine::DeckId;
use dubplate_rekordbox::{HotCue, HotCuePad, MemoryCue, Rgb};
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

/// How much of its colour the part of a waveform already played keeps.
pub const PLAYED_OPACITY: f32 = 0.32;
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

/// The colours a theme picks by hand; [`Theme`] works out the rest from them.
struct Palette {
    light: bool,
    background: u32,
    panel: u32,
    raised: u32,
    /// What hairlines, beat lines and meters are drawn with, at an opacity.
    ink: u32,
    text: u32,
    muted: u32,
    thumb: u32,
    decks: [u32; 2],
    on_deck: u32,
    ok: u32,
    attention: u32,
    alert: u32,
    bands: [u32; 3],
    cue_point: u32,
    mix_out: u32,
    saved_loop: u32,
    phrase_intro: u32,
    phrase_breakdown: u32,
}

/// Every colour the views draw with, in one theme.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    /// Whether the theme is light, for the component library's own controls.
    pub light: bool,
    /// Behind the waveforms, the mixer and the browser.
    pub background: Rgba,
    /// Behind the deck panels and the search field.
    pub panel: Rgba,
    /// Buttons, pads and the selected crate.
    pub raised: Rgba,
    /// Hairline borders.
    pub line: Rgba,
    /// Primary text.
    pub text: Rgba,
    /// Labels, artists and secondary numbers.
    pub muted: Rgba,
    /// The playhead across both zoomed waveforms.
    pub playhead: Rgba,
    /// Beat lines over the zoomed waveforms.
    pub beat_line: Rgba,
    /// Downbeat lines over the zoomed waveforms, bright enough to judge a bar against its kick.
    pub downbeat_line: Rgba,
    /// Bar lines on a whole-track overview.
    pub bar_line: Rgba,
    /// Phrase lines on a whole-track overview, every eight bars.
    pub phrase_line: Rgba,
    /// The low band of a 3-band waveform.
    pub band_low: Rgba,
    /// The mid band of a 3-band waveform.
    pub band_mid: Rgba,
    /// The high band of a 3-band waveform.
    pub band_high: Rgba,
    /// Text on a deck colour, such as the deck badge.
    pub on_deck: Rgba,
    /// Things that are fine, such as a library that loaded.
    pub ok: Rgba,
    /// Things that need the DJ's attention before the set is ready, such as a missing cue.
    pub attention: Rgba,
    /// Problems the DJ has to act on.
    pub alert: Rgba,
    /// The track of sliders and the empty part of the effect amount.
    pub track: Rgba,
    /// Slider and fader thumbs.
    pub thumb: Rgba,
    /// A lit meter segment.
    pub meter_on: Rgba,
    /// An unlit meter segment.
    pub meter_off: Rgba,
    /// A lit meter segment near clipping.
    pub meter_hot: Rgba,
    /// Behind the selected browser row.
    pub selected_row: Rgba,
    /// The title of a track played this session, dimmed as rekordbox dims played tracks.
    pub played_title: Rgba,
    /// A key that follows smoothly in the library's Match column: same key, one step, relative.
    pub key_smooth: Rgba,
    /// A key move that lifts the energy, such as an energy boost.
    pub key_lift: Rgba,
    /// Keys that clash.
    pub key_clash: Rgba,
    /// Between browser rows.
    pub row_line: Rgba,
    /// Behind the help overlay.
    pub scrim: Rgba,
    /// The CUE button's point on the lanes and the overview, orange as on CDJs.
    pub cue_point: Rgba,
    /// Memory cues on the deck overview, red as on CDJs.
    pub memory_cue: Rgba,
    /// The set's mix-in cue on the deck overview.
    pub mix_in_mark: Rgba,
    /// The set's mix-out cue on the deck overview.
    pub mix_out_mark: Rgba,
    /// Saved loops on the deck overview.
    pub saved_loop: Rgba,
    /// The minute ticks along the deck overview.
    pub minute_tick: Rgba,
    /// Over the last 30 seconds of the deck overview.
    pub ending_tint: Rgba,
    /// Over the part of the deck overview already played.
    pub played_shade: Rgba,
    /// The phrase strip's intro sections.
    pub phrase_intro: Rgba,
    /// The phrase strip's builds.
    pub phrase_build: Rgba,
    /// The phrase strip's drops.
    pub phrase_drop: Rgba,
    /// The phrase strip's breakdowns.
    pub phrase_breakdown: Rgba,
    /// The phrase strip's outros.
    pub phrase_outro: Rgba,
    /// The phrase strip's unnamed sections.
    pub phrase_other: Rgba,
    decks: [Rgba; 2],
}

impl Theme {
    const fn new(palette: &Palette) -> Self {
        // Lines need more ink to show on a light surface.
        let lines = if palette.light { 1.8 } else { 1.0 };
        Self {
            light: palette.light,
            background: hex(palette.background),
            panel: hex(palette.panel),
            raised: hex(palette.raised),
            line: with_alpha(palette.ink, 0.07 * lines),
            text: hex(palette.text),
            muted: hex(palette.muted),
            playhead: hex(palette.ink),
            beat_line: with_alpha(palette.ink, 0.16 * lines),
            downbeat_line: with_alpha(palette.ink, 0.55),
            bar_line: with_alpha(palette.ink, 0.05 * lines),
            phrase_line: with_alpha(palette.ink, 0.1 * lines),
            band_low: hex(palette.bands[0]),
            band_mid: hex(palette.bands[1]),
            band_high: hex(palette.bands[2]),
            on_deck: hex(palette.on_deck),
            ok: hex(palette.ok),
            attention: hex(palette.attention),
            alert: hex(palette.alert),
            track: with_alpha(palette.ink, 0.1 * lines),
            thumb: hex(palette.thumb),
            meter_on: with_alpha(palette.ink, 0.72),
            meter_off: with_alpha(palette.ink, 0.07 * lines),
            meter_hot: hex(palette.decks[0]),
            selected_row: with_alpha(palette.ink, 0.05 * lines),
            played_title: with_alpha(palette.text, 0.45),
            key_smooth: hex(palette.ok),
            key_lift: hex(palette.bands[0]),
            key_clash: hex(palette.alert),
            row_line: with_alpha(palette.ink, 0.045 * lines),
            scrim: with_alpha(0x000000, 0.55),
            cue_point: hex(palette.cue_point),
            memory_cue: hex(palette.alert),
            mix_in_mark: hex(palette.ok),
            mix_out_mark: hex(palette.mix_out),
            saved_loop: hex(palette.saved_loop),
            minute_tick: with_alpha(palette.ink, 0.28),
            ending_tint: with_alpha(palette.attention, 0.14),
            played_shade: with_alpha(palette.background, 0.62),
            phrase_intro: hex(palette.phrase_intro),
            phrase_build: hex(palette.bands[0]),
            phrase_drop: hex(palette.bands[1]),
            phrase_breakdown: hex(palette.phrase_breakdown),
            phrase_outro: hex(palette.muted),
            phrase_other: with_alpha(palette.ink, 0.2),
            decks: [hex(palette.decks[0]), hex(palette.decks[1])],
        }
    }
}

/// A theme the DJ can choose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeId {
    /// Cool dark grey with peach and periwinkle decks: Dubplate's own look.
    #[default]
    Graphite,
    /// Warm grey hardware with orange and sky blue.
    Booth,
    /// Black, white and one acid lime.
    Club,
    /// Near-black with a single red, like a mixing console.
    Console,
    /// Smoked glass with amber and blue.
    Glass,
    /// Warm brown-black with a coral accent.
    Groove,
    /// Matte black with orange, like a turntable.
    Platter,
    /// Midnight blue with neon colours across the spectrum.
    Spectral,
    /// Deep cobalt blue with an electric yellow.
    Cobalt,
    /// Light warm grey panels with orange and red, like studio hardware.
    Chassis,
    /// Light, with a signal orange, like a measuring instrument.
    Instrument,
    /// Off-white paper and ink.
    Paper,
    /// Blue-grey with blue and orange decks, after rekordbox's colours.
    Midnight,
    /// Charcoal with orange and blue decks, after Traktor's colours.
    Ember,
}

impl ThemeId {
    /// Every theme, in the order Settings lists them.
    pub const ALL: [Self; 14] = [
        Self::Graphite,
        Self::Booth,
        Self::Club,
        Self::Console,
        Self::Glass,
        Self::Groove,
        Self::Platter,
        Self::Spectral,
        Self::Cobalt,
        Self::Midnight,
        Self::Ember,
        Self::Chassis,
        Self::Instrument,
        Self::Paper,
    ];

    /// The theme's name, as Settings shows it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Graphite => "Graphite",
            Self::Booth => "Booth",
            Self::Club => "Club",
            Self::Console => "Console",
            Self::Glass => "Glass",
            Self::Groove => "Groove",
            Self::Platter => "Platter",
            Self::Spectral => "Spectral",
            Self::Cobalt => "Cobalt",
            Self::Chassis => "Chassis",
            Self::Instrument => "Instrument",
            Self::Paper => "Paper",
            Self::Midnight => "Midnight",
            Self::Ember => "Ember",
        }
    }

    /// The name the settings keep the theme by.
    pub fn name(self) -> &'static str {
        match self {
            Self::Graphite => "graphite",
            Self::Booth => "booth",
            Self::Club => "club",
            Self::Console => "console",
            Self::Glass => "glass",
            Self::Groove => "groove",
            Self::Platter => "platter",
            Self::Spectral => "spectral",
            Self::Cobalt => "cobalt",
            Self::Chassis => "chassis",
            Self::Instrument => "instrument",
            Self::Paper => "paper",
            Self::Midnight => "midnight",
            Self::Ember => "ember",
        }
    }

    /// The theme the settings name, or `None` for a name this version does not know.
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|theme| theme.name() == name)
    }

    /// Its colours.
    pub fn theme(self) -> &'static Theme {
        match self {
            Self::Graphite => &GRAPHITE,
            Self::Booth => &BOOTH,
            Self::Club => &CLUB,
            Self::Console => &CONSOLE,
            Self::Glass => &GLASS,
            Self::Groove => &GROOVE,
            Self::Platter => &PLATTER,
            Self::Spectral => &SPECTRAL,
            Self::Cobalt => &COBALT,
            Self::Chassis => &CHASSIS,
            Self::Instrument => &INSTRUMENT,
            Self::Paper => &PAPER,
            Self::Midnight => &MIDNIGHT,
            Self::Ember => &EMBER,
        }
    }

    fn position(self) -> usize {
        Self::ALL
            .iter()
            .position(|theme| *theme == self)
            .unwrap_or_default()
    }
}

static GRAPHITE: Theme = Theme::new(&Palette {
    light: false,
    background: 0x0B0C0E,
    panel: 0x111215,
    raised: 0x191A1E,
    ink: 0xFFFFFF,
    text: 0xEEEFF1,
    muted: 0x8A8F98,
    thumb: 0xE6E7E9,
    decks: [0xF5B78E, 0xA3B6FF],
    on_deck: 0x0B0C0E,
    ok: 0x7FD1A4,
    attention: 0xF2C46D,
    alert: 0xF07167,
    bands: [0x6E86E8, 0xD7AE6E, 0xECEAE4],
    cue_point: 0xF59A4B,
    mix_out: 0xC7B1F5,
    saved_loop: 0xF2D48A,
    phrase_intro: 0x5E6B8C,
    phrase_breakdown: 0x8FD3C8,
});

static BOOTH: Theme = Theme::new(&Palette {
    light: false,
    background: 0x0D0E10,
    panel: 0x15171A,
    raised: 0x1E2125,
    ink: 0xFFFFFF,
    text: 0xEDEBE6,
    muted: 0xA9AEB6,
    thumb: 0xC3C7CD,
    decks: [0xF08A4B, 0x4FB3E8],
    on_deck: 0x0D0E10,
    ok: 0x7FD1A4,
    attention: 0xF2C46D,
    alert: 0xF2827A,
    bands: [0x4FB3E8, 0xF08A4B, 0xEDEBE6],
    cue_point: 0xF59A4B,
    mix_out: 0xC7B1F5,
    saved_loop: 0xF2D48A,
    phrase_intro: 0x5E6B8C,
    phrase_breakdown: 0x8FD3C8,
});

static CLUB: Theme = Theme::new(&Palette {
    light: false,
    background: 0x000000,
    panel: 0x0E0E0E,
    raised: 0x1A1A1A,
    ink: 0xFFFFFF,
    text: 0xFFFFFF,
    muted: 0x9A9A9A,
    thumb: 0xBDBDBD,
    decks: [0xD4F53C, 0xA78BFA],
    on_deck: 0x000000,
    ok: 0x6EE7A0,
    attention: 0xF5D33C,
    alert: 0xFF5A5A,
    bands: [0xA78BFA, 0xD4F53C, 0xFFFFFF],
    cue_point: 0xFF9F43,
    mix_out: 0xA78BFA,
    saved_loop: 0xF5D33C,
    phrase_intro: 0x5A5A7A,
    phrase_breakdown: 0x6EE7D0,
});

static CONSOLE: Theme = Theme::new(&Palette {
    light: false,
    background: 0x0C0C0D,
    panel: 0x121214,
    raised: 0x1C1C1F,
    ink: 0xFFFFFF,
    text: 0xEDEDEF,
    muted: 0x8E8E96,
    thumb: 0xC4C4CA,
    decks: [0xFF5A52, 0x7FB2FF],
    on_deck: 0x0C0C0D,
    ok: 0x7FD1A4,
    attention: 0xF2C46D,
    alert: 0xFF7A73,
    bands: [0x7FB2FF, 0xFF5A52, 0xEDEDEF],
    cue_point: 0xF59A4B,
    mix_out: 0xC7B1F5,
    saved_loop: 0xF2D48A,
    phrase_intro: 0x5E6478,
    phrase_breakdown: 0x8FD3C8,
});

static GLASS: Theme = Theme::new(&Palette {
    light: false,
    background: 0x0A0B0F,
    panel: 0x14161C,
    raised: 0x21242C,
    ink: 0xFFFFFF,
    text: 0xF5F6F8,
    muted: 0xA0A4AE,
    thumb: 0xF5F6F8,
    decks: [0xFFB340, 0x5AA9FF],
    on_deck: 0x0A0B0F,
    ok: 0x6FD99A,
    attention: 0xFFC75A,
    alert: 0xFF6A5E,
    bands: [0x5AA9FF, 0xFFB340, 0xF5F6F8],
    cue_point: 0xFF9F43,
    mix_out: 0xC7B1F5,
    saved_loop: 0xFFD98A,
    phrase_intro: 0x5E6B8C,
    phrase_breakdown: 0x7FD8E0,
});

static GROOVE: Theme = Theme::new(&Palette {
    light: false,
    background: 0x121110,
    panel: 0x1A1816,
    raised: 0x2A2724,
    ink: 0xF1ECE4,
    text: 0xF1ECE4,
    muted: 0xA39C92,
    thumb: 0xC9C2B8,
    decks: [0xFF6A3D, 0x8EC5C0],
    on_deck: 0x121110,
    ok: 0x9CCB8A,
    attention: 0xE8B65A,
    alert: 0xFF7A6A,
    bands: [0x8EC5C0, 0xFF6A3D, 0xF1ECE4],
    cue_point: 0xFF9F43,
    mix_out: 0xC9A8E0,
    saved_loop: 0xE8C97A,
    phrase_intro: 0x6E675E,
    phrase_breakdown: 0x8EC5C0,
});

static PLATTER: Theme = Theme::new(&Palette {
    light: false,
    background: 0x050506,
    panel: 0x0E0E10,
    raised: 0x1A1A1D,
    ink: 0xFFFFFF,
    text: 0xECEBE8,
    muted: 0x8F8D89,
    thumb: 0xECEBE8,
    decks: [0xFF6B2C, 0x9FB4C8],
    on_deck: 0x050506,
    ok: 0x7FD1A4,
    attention: 0xF2C46D,
    alert: 0xFF6A5E,
    bands: [0x9FB4C8, 0xFF6B2C, 0xECEBE8],
    cue_point: 0xFF9F43,
    mix_out: 0xC7B1F5,
    saved_loop: 0xF2D48A,
    phrase_intro: 0x5E6470,
    phrase_breakdown: 0x8FD3C8,
});

static SPECTRAL: Theme = Theme::new(&Palette {
    light: false,
    background: 0x0E1220,
    panel: 0x151A2A,
    raised: 0x232A3E,
    ink: 0xDCE3F2,
    text: 0xDCE3F2,
    muted: 0x9AA5BF,
    thumb: 0xDCE3F2,
    decks: [0xF2A33A, 0x3ED6E8],
    on_deck: 0x0E1220,
    ok: 0xB9E04A,
    attention: 0xFFE07A,
    alert: 0xFF6B8A,
    bands: [0x3ED6E8, 0xE04CC8, 0xFFF3C4],
    cue_point: 0xF2A33A,
    mix_out: 0xB58CFF,
    saved_loop: 0xFFF3C4,
    phrase_intro: 0x3B4560,
    phrase_breakdown: 0x3ED6E8,
});

static COBALT: Theme = Theme::new(&Palette {
    light: false,
    background: 0x1430C8,
    panel: 0x1A37D2,
    raised: 0x2240D4,
    ink: 0xFFFFFF,
    text: 0xFFFFFF,
    muted: 0xDCE2FF,
    thumb: 0xFFFFFF,
    decks: [0xE6FF4A, 0x7FE3FF],
    on_deck: 0x0F1A55,
    ok: 0x8CF5B5,
    attention: 0xFFE24A,
    alert: 0xFFB0B0,
    bands: [0x7FE3FF, 0xE6FF4A, 0xFFFFFF],
    cue_point: 0xFFC069,
    mix_out: 0xF0C8FF,
    saved_loop: 0xFFE27A,
    phrase_intro: 0x6F84E8,
    phrase_breakdown: 0x9FF0E0,
});

static MIDNIGHT: Theme = Theme::new(&Palette {
    light: false,
    background: 0x15171B,
    panel: 0x1C1F24,
    raised: 0x272B32,
    ink: 0xFFFFFF,
    text: 0xE8EBEF,
    muted: 0x9AA3AE,
    thumb: 0xD5DAE0,
    decks: [0x3FA9F5, 0xF5A623],
    on_deck: 0x0E1013,
    ok: 0x5ED38A,
    attention: 0xF5C242,
    alert: 0xF2656B,
    bands: [0x2F6BF0, 0xF5A623, 0xFFFFFF],
    cue_point: 0xF59A4B,
    mix_out: 0xB46CF0,
    saved_loop: 0xF2DD3A,
    phrase_intro: 0x4A5568,
    phrase_breakdown: 0x28D2C9,
});

static EMBER: Theme = Theme::new(&Palette {
    light: false,
    background: 0x141414,
    panel: 0x1C1C1C,
    raised: 0x2A2A2A,
    ink: 0xFFFFFF,
    text: 0xEDEDED,
    muted: 0xA3A3A3,
    thumb: 0xD0D0D0,
    decks: [0xFF8C1A, 0x3FA7FF],
    on_deck: 0x141414,
    ok: 0x6CCF6C,
    attention: 0xFFC233,
    alert: 0xFF6161,
    bands: [0x3FA7FF, 0xFF8C1A, 0xF2F2F2],
    cue_point: 0xFFB04D,
    mix_out: 0xB98CFF,
    saved_loop: 0xFFD966,
    phrase_intro: 0x4D5566,
    phrase_breakdown: 0x4DD2C2,
});

static CHASSIS: Theme = Theme::new(&Palette {
    light: true,
    background: 0xF8F7F4,
    panel: 0xEFEDE8,
    raised: 0xE2DFD9,
    ink: 0x000000,
    text: 0x141414,
    muted: 0x57554F,
    thumb: 0x3A3A3A,
    decks: [0xE8571C, 0x5488CB],
    on_deck: 0x141414,
    ok: 0x1E7F45,
    attention: 0x8F5A00,
    alert: 0xB3261E,
    bands: [0x2B59C3, 0xE8571C, 0x3A3A3A],
    cue_point: 0xD9480F,
    mix_out: 0x6A4BC4,
    saved_loop: 0x8F6A00,
    phrase_intro: 0x8A93AA,
    phrase_breakdown: 0x2C8C80,
});

static INSTRUMENT: Theme = Theme::new(&Palette {
    light: true,
    background: 0xF4F3F0,
    panel: 0xEEEDE9,
    raised: 0xE2E0DB,
    ink: 0x000000,
    text: 0x161616,
    muted: 0x57544E,
    thumb: 0x4A4742,
    decks: [0xE5501A, 0x5488CB],
    on_deck: 0x161616,
    ok: 0x1E7F45,
    attention: 0x8F5A00,
    alert: 0xB52E15,
    bands: [0x2F6FD6, 0xF2541B, 0x4A4742],
    cue_point: 0xD9480F,
    mix_out: 0x6A4BC4,
    saved_loop: 0x8F6A00,
    phrase_intro: 0x8F8A82,
    phrase_breakdown: 0x2C8C80,
});

static PAPER: Theme = Theme::new(&Palette {
    light: true,
    background: 0xF5F3EE,
    panel: 0xEFECE6,
    raised: 0xE3DFD6,
    ink: 0x000000,
    text: 0x1A1917,
    muted: 0x58544D,
    thumb: 0x4A4740,
    decks: [0xE5533D, 0x5488CB],
    on_deck: 0x1A1917,
    ok: 0x2E7A3E,
    attention: 0x8A5A00,
    alert: 0xB33A26,
    bands: [0x3A6EA5, 0xE5533D, 0x4A4740],
    cue_point: 0xD9480F,
    mix_out: 0x6A4BC4,
    saved_loop: 0x8A6A00,
    phrase_intro: 0x9A958C,
    phrase_breakdown: 0x2C8C80,
});

/// The theme the views draw with now, as its place in [`ThemeId::ALL`].
static ACTIVE: AtomicUsize = AtomicUsize::new(0);

/// The colours the views draw with now.
pub fn active() -> &'static Theme {
    active_id().theme()
}

/// The theme chosen now.
pub fn active_id() -> ThemeId {
    ThemeId::ALL
        .get(ACTIVE.load(Ordering::Relaxed))
        .copied()
        .unwrap_or_default()
}

/// Wears `theme` from the next frame on: the views read their colours from it, and the
/// component library's controls are given them too.
pub fn wear(theme: ThemeId, cx: &mut App) {
    ACTIVE.store(theme.position(), Ordering::Relaxed);
    apply_to_components(cx);
    cx.refresh_windows();
}

/// A colour rekordbox gave a cue, from its hot cue palette or its memory colours, drawn as
/// rekordbox draws it, whatever the theme.
pub const fn rekordbox_colour(rgb: Rgb) -> Rgba {
    Rgba {
        r: rgb.red as f32 / 255.0,
        g: rgb.green as f32 / 255.0,
        b: rgb.blue as f32 / 255.0,
        a: 1.0,
    }
}

/// The colour of the hot cue on `pad`: its own, else rekordbox's default for the pad, or
/// orange for a hot loop.
pub fn hot_cue_colour(pad: HotCuePad, cue: &HotCue) -> Rgba {
    rekordbox_colour(cue.display_color(pad).rgb())
}

/// The colour of a memory cue's mark: its own, else the theme's memory cue colour, or its
/// saved loop colour for a memory loop without one.
pub fn memory_cue_colour(cue: &MemoryCue) -> Rgba {
    match (cue.color, cue.loop_end_ms) {
        (Some(color), _) => rekordbox_colour(color.rgb()),
        (None, Some(_)) => active().saved_loop,
        (None, None) => active().memory_cue,
    }
}

/// The colour that marks everything belonging to a deck.
pub fn deck_colour(deck: DeckId) -> Rgba {
    active().decks[deck.index()]
}

/// `colour` at `opacity` times its own opacity.
pub fn faded(colour: Rgba, opacity: f32) -> Rgba {
    Rgba {
        a: colour.a * opacity,
        ..colour
    }
}

/// Gives the component library the active theme's colours and fonts, so the search field
/// matches the rest of the window.
pub fn apply_to_components(cx: &mut App) {
    let theme = active();
    let mode = if theme.light {
        ThemeMode::Light
    } else {
        ThemeMode::Dark
    };
    ComponentTheme::change(mode, None, cx);
    ComponentTheme::update(cx, |component| {
        component.font_family = UI_FAMILY.into();
        component.mono_font_family = MONO_FAMILY.into();
        component.font_size = TEXT_SIZE;
        component.radius = CONTROL_RADIUS;
        component.shadow = false;
        let colours = &mut component.colors;
        colours.background = theme.background.into();
        colours.foreground = theme.text.into();
        colours.border = theme.line.into();
        colours.input = theme.line.into();
        colours.muted = theme.raised.into();
        colours.muted_foreground = theme.muted.into();
        colours.popover = theme.panel.into();
        colours.popover_foreground = theme.text.into();
        colours.accent = theme.raised.into();
        colours.accent_foreground = theme.text.into();
        colours.ring = theme.line.into();
        colours.caret = theme.text.into();
        colours.selection = faded(theme.decks[1], 0.35).into();
        colours.scrollbar = gpui::transparent_black();
        colours.scrollbar_thumb = faded(theme.muted, 0.45).into();
        colours.scrollbar_thumb_hover = faded(theme.muted, 0.7).into();
    });
}

/// The contrast ratio between two opaque colours, from 1 to 21, as WCAG measures it.
#[cfg(test)]
fn contrast(first: Rgba, second: Rgba) -> f32 {
    fn channel(value: f32) -> f32 {
        if value <= 0.039_28 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }
    fn luminance(colour: Rgba) -> f32 {
        0.2126 * channel(colour.r) + 0.7152 * channel(colour.g) + 0.0722 * channel(colour.b)
    }
    let (lighter, darker) = {
        let (first, second) = (luminance(first), luminance(second));
        if first > second {
            (first, second)
        } else {
            (second, first)
        }
    };
    (lighter + 0.05) / (darker + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graphite_keeps_its_colours() {
        let graphite = ThemeId::Graphite.theme();
        assert_eq!(graphite.background, hex(0x0B0C0E));
        assert_eq!(graphite.line, with_alpha(0xFFFFFF, 0.07));
        assert_eq!(graphite.memory_cue, graphite.alert);
        assert_eq!(graphite.phrase_build, graphite.band_low);
        assert_eq!(ThemeId::default(), ThemeId::Graphite);
    }

    #[test]
    fn every_theme_reads_clearly() {
        let mut problems = Vec::new();
        let mut check = |theme: ThemeId, what: String, ratio: f32, least: f32| {
            if ratio < least {
                problems.push(format!(
                    "{}: {what} is {ratio:.2}, under {least}",
                    theme.label()
                ));
            }
        };
        for id in ThemeId::ALL {
            let theme = id.theme();
            for (surface_name, surface) in [
                ("background", theme.background),
                ("panel", theme.panel),
                ("raised", theme.raised),
            ] {
                for (name, colour, least) in [
                    ("text", theme.text, 7.0),
                    ("muted", theme.muted, 4.5),
                    ("ok", theme.ok, 3.0),
                    ("attention", theme.attention, 3.0),
                    ("alert", theme.alert, 3.0),
                ] {
                    let what = format!("{name} on {surface_name}");
                    check(id, what, contrast(colour, surface), least);
                }
            }
            for deck in DeckId::ALL {
                let colour = theme.decks[deck.index()];
                let badge = format!("badge text on deck {}", deck.index());
                check(id, badge, contrast(theme.on_deck, colour), 4.5);
                let mark = format!("deck {} on panel", deck.index());
                check(id, mark, contrast(colour, theme.panel), 3.0);
            }
            let playhead = contrast(theme.playhead, theme.background);
            check(id, "playhead".to_owned(), playhead, 3.0);
        }
        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }

    #[test]
    fn a_theme_is_kept_by_its_name() {
        for id in ThemeId::ALL {
            assert_eq!(ThemeId::named(id.name()), Some(id));
        }
        assert_eq!(ThemeId::named("unknown"), None);
    }
}
