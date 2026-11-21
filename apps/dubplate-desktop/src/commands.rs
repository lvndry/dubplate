//! What the DJ can ask of Dubplate, whatever they asked with: the keyboard, the mouse or a
//! controller. Every input turns into a [`Command`], and the window carries each one out in
//! one place.

use dubplate_engine::DeckId;
use dubplate_rekordbox::HotCuePad;

use crate::controls::Control;

/// Which way a control moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Down, or towards deck A.
    Down,
    /// Up, or towards deck B.
    Up,
}

impl Direction {
    /// -1.0 for down and 1.0 for up.
    pub fn sign(self) -> f64 {
        match self {
            Self::Down => -1.0,
            Self::Up => 1.0,
        }
    }
}

/// How a button was used. A key or a click is a [`Press::Tap`]: down and up at once. A
/// controller sends the two halves apart, which buttons held down, such as CUE, need.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Press {
    /// The button went down.
    Down,
    /// The button came up.
    Up,
    /// The button went down and up.
    Tap,
}

/// Something the DJ asks for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Command {
    /// Play or pause a deck.
    TogglePlay(DeckId),
    /// The CDJ cue button of a deck.
    Cue(DeckId, Press),
    /// Go back to the start of the track, as SHIFT + CUE does.
    JumpToStart(DeckId),
    /// A hot cue pad: jumps to its cue, or sets one where the deck is when the pad is empty.
    HotCue(DeckId, HotCuePad, Press),
    /// Delete a hot cue.
    DeleteHotCue(DeckId, HotCuePad),
    /// Load the track selected in the browser.
    LoadSelected(DeckId),
    /// Load the track playing on the other deck at the same place, as pressing LOAD twice does.
    InstantDouble(DeckId),
    /// Change a deck's tempo by one step.
    NudgeTempo(DeckId, Direction),
    /// Move a fader or knob to a position from 0.0 to 1.0.
    SetControl(Control, f32),
    /// Move the crossfader one step towards deck A ([`Direction::Down`]) or deck B.
    MoveCrossfader(Direction),
    /// Select the row above or below in the browser.
    MoveSelection(Direction),
    /// Move the browser's cursor by a number of rows, in the crates or the tracks, whichever
    /// has the cursor. Negative moves up.
    Browse(i32),
    /// Move the browser's cursor between the crates and the tracks.
    SwitchBrowserPane,
    /// Put the cursor in the search field.
    FocusSearch,
    /// Show or hide the list of shortcuts.
    ToggleHelp,
    /// Close the help or leave the search field.
    Dismiss,
}
