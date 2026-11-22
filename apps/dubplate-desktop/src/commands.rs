//! What the DJ can ask of Dubplate, whatever they asked with: the keyboard, the mouse or a
//! controller. Every input turns into a [`Command`], and the window carries each one out in
//! one place.

use dubplate_engine::{DeckId, EffectKind, EffectTarget, EffectUnitId, SmartCfx};
use dubplate_midi::PadMode;

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

/// The two screens: preparing a set, and playing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The library beside the set being prepared.
    Prepare,
    /// The decks and the mixer.
    Perform,
}

/// A change to the set being prepared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetEdit {
    /// Add the track selected in the library at the end of the set.
    Add,
    /// Put the track selected in the library right after the selected set track.
    InsertAfterSelection,
    /// Put a Next suggestion, 0 for the first, right after the selected set track.
    InsertSuggestion(usize),
    /// Move the selected set track up or down.
    Move(Direction),
    /// Take the selected set track out of the set.
    Remove,
    /// Switch the seam after the selected set track between planned and free.
    TogglePlanned,
    /// Accept the mix cues Dubplate proposes for the selected set track.
    AcceptProposedCues,
    /// Keep the track selected in the library as a backup, or stop keeping it.
    ToggleBackup,
    /// Undo the last change to the set.
    Undo,
    /// Confirm the selected set track's grid by ear, or take the confirmation back.
    ConfirmGrid,
    /// Move the selected set track's grid by milliseconds, later when positive.
    NudgeGrid(i32),
    /// Move the selected set track's bars by beats, later when positive.
    ShiftDownbeat(i32),
    /// Put the selected set track's mix-in on the bar nearest the playhead.
    MixInAtPlayhead,
    /// Put the selected set track's mix-out on the bar nearest the playhead.
    MixOutAtPlayhead,
}

/// A press on one of a deck's performance pads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PadPress {
    /// The deck.
    pub deck: DeckId,
    /// The pad, 0 for pad 1.
    pub pad: usize,
    /// The mode the controller sent it in, or `None` for the deck's current mode, as from the
    /// keys and the screen.
    pub mode: Option<PadMode>,
    /// Down, up or both.
    pub press: Press,
    /// Whether SHIFT was held.
    pub shift: bool,
}

/// A loop button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopAction {
    /// LOOP IN: where the loop starts.
    In,
    /// LOOP OUT: where it ends, turning it on.
    Out,
    /// 4 BEAT/EXIT: a four-beat loop, or leave the loop that is on.
    FourBeatOrExit,
}

/// What turning a jog wheel does to the track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JogMotion {
    /// The hand on the platter moves the record.
    Scratch,
    /// The wheel's edge bends the tempo to line up beats.
    Nudge,
    /// With SHIFT held, moves quickly through the track.
    Search,
}

/// A change to an effect unit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EffectEdit {
    /// Choose this effect.
    Kind(EffectKind),
    /// Move through the effects in the picker's order, as FX SELECT does; negative goes back.
    StepKind(i32),
    /// The next longer ([`Direction::Up`]) or shorter beat length.
    Beat(Direction),
    /// Put the unit on a deck or the master.
    Target(EffectTarget),
    /// Turn the level and depth knob, from 0.0 to 1.0.
    Depth(f32),
    /// Switch the unit on or off.
    Toggle,
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
    /// A performance pad, doing what its pad mode says.
    Pad(PadPress),
    /// Put a deck's pads in a pad mode.
    SetPadMode(DeckId, PadMode),
    /// Put a deck's pads in the next pad mode.
    CyclePadMode(DeckId),
    /// SLIP: scratches and loops let the track play on underneath, and the deck lands back on it.
    ToggleSlip(DeckId),
    /// A loop button.
    Loop(DeckId, LoopAction),
    /// CUE/LOOP CALL ◀ ([`Direction::Down`]) or ▶: halve or double the loop that is on, else
    /// call the memory cue before or after the playhead, pausing on it as its cue point.
    CueLoopCall(DeckId, Direction),
    /// MEMORY, as SHIFT + CUE/LOOP CALL ▶: keep the cue point, or the loop that is on, as a
    /// memory cue of the track.
    StoreMemoryCue(DeckId),
    /// DELETE, as SHIFT + CUE/LOOP CALL ◀: delete the memory cue at the cue point.
    DeleteMemoryCue(DeckId),
    /// A channel's headphone CUE button: hear the channel in the headphones, before its fader.
    ToggleHeadphoneCue(DeckId),
    /// MASTER CUE: hear the master in the headphones too.
    ToggleMasterCue,
    /// A tap on the beat, as SHIFT + a channel's CUE does: enough taps give a track without a
    /// beat grid its tempo and beats.
    TapTempo(DeckId),
    /// Hear the seam after the selected set track in the headphones, or stop hearing it.
    AuditionSeam,
    /// Start recording the mix to a file, or stop and close it.
    ToggleRecording,
    /// SMART CFX: both channels' filter knobs drive the filter and the chosen preset together,
    /// or go back to the plain filter.
    ToggleSmartCfx,
    /// SHIFT + SMART CFX: the next Smart CFX preset.
    NextSmartCfx,
    /// Pick a Smart CFX preset, from the screen.
    ChooseSmartCfx(SmartCfx),
    /// SMART FADER: the crossfader moves a shared tempo and swaps the bass.
    ToggleSmartFader,
    /// Load the track selected in the browser.
    LoadSelected(DeckId),
    /// Load the track playing on the other deck at the same place, as pressing LOAD twice does.
    InstantDouble(DeckId),
    /// Change a deck's tempo by one step.
    NudgeTempo(DeckId, Direction),
    /// BEAT SYNC: lock a deck to the tempo master's tempo and beats, or let it go.
    ToggleSync(DeckId),
    /// Make a deck the tempo master that synced decks follow, as holding BEAT SYNC does.
    MakeTempoMaster(DeckId),
    /// Step a deck's tempo range: ±6%, ±10%, ±16%, wide.
    CycleTempoRange(DeckId),
    /// Key lock, a CDJ's MASTER TEMPO: tempo changes keep the track's pitch.
    ToggleKeyLock(DeckId),
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
    /// A jog wheel turned by a number of ticks, positive clockwise.
    Jog(DeckId, JogMotion, i32),
    /// A hand touched or left a jog wheel's platter.
    JogTouch(DeckId, bool),
    /// Change an effect unit.
    Effect(EffectUnitId, EffectEdit),
    /// Choose which effect unit the controller's BEAT FX section drives, a number of units
    /// along; negative goes back.
    FocusEffectUnit(i32),
    /// Show the Prepare or the Perform screen.
    SwitchMode(Mode),
    /// Change the set.
    EditSet(SetEdit),
    /// Move the keyboard to the next part of Prepare, the library, the set and the Next
    /// suggestions in turn, or back with [`Direction::Up`].
    SwitchPrepareFocus(Direction),
    /// Load the set's first track on deck A and its second on deck B, each at its mix-in.
    LoadSet,
    /// Save the set as a rekordbox XML file for rekordbox to import.
    ExportSet,
    /// Show or hide the list of shortcuts.
    ToggleHelp,
    /// Show or hide the library's settings: the music folders and rekordbox.
    ToggleLibrary,
    /// Open the folder picker to add music folders to the library.
    AddMusicFolders,
    /// Close the help or leave the search field.
    Dismiss,
}
