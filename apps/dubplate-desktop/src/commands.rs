//! What the DJ can ask of Dubplate, whatever they asked with: the keyboard, the mouse or a
//! controller. Every input turns into a [`Command`], and the window carries each one out in
//! one place.

use dubplate_engine::{
    ChannelFaderCurve, CrossfaderAssign, CrossfaderCurve, DeckId, EffectKind, EffectTarget,
    EffectUnitId, SmartCfx, SmartFaderSource, Stem, StemSettings, SynthSound,
};
use dubplate_midi::{FaderStart, PadMode};
use dubplate_prep::{GridEdit, TagId, TrackColour};

use crate::controls::Control;
use crate::pad_fx::{PadFxLayer, PadFxPreset};
use crate::theme::ThemeId;

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

/// Which channels the Sound Color FX picker sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CfxScope {
    /// Both channels at once.
    #[default]
    Both,
    /// One channel.
    Deck(DeckId),
}

impl CfxScope {
    /// The channels it sets.
    pub fn decks(self) -> &'static [DeckId] {
        match self {
            CfxScope::Both => &DeckId::ALL,
            CfxScope::Deck(DeckId::A) => &[DeckId::A],
            CfxScope::Deck(DeckId::B) => &[DeckId::B],
        }
    }
}

/// A change in the Pad FX editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadFxEdit {
    /// Open the editor on pad 1 of a layer, or close it when it is open.
    Toggle(PadFxLayer),
    /// Close the editor.
    Close,
    /// Edit pad `1` of layer `0`, pad 0 first.
    Select(PadFxLayer, usize),
    /// Step the pad's sound through the list.
    Sound(i32),
    /// Step the pad's beat or parameter.
    Parameter(i32),
    /// Step the pad's depth by 5%.
    Depth(i32),
    /// Switch the pad between held and latched.
    Trigger,
    /// Switch what letting go does to the pad's trail.
    Release,
    /// Put the pad back as its preset has it.
    ResetPad,
    /// Replace all 32 pads with a preset's.
    Preset(PadFxPreset),
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
    /// Change the selected set track's grid, or in Perform the grid of the deck whose grid bar
    /// is open.
    Grid(GridEdit),
    /// Put the selected set track's mix-in on the bar nearest the playhead.
    MixInAtPlayhead,
    /// Put the selected set track's mix-out on the bar nearest the playhead.
    MixOutAtPlayhead,
}

/// One of the two Auto Cue settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoCueChoice {
    /// Cue at the earliest memory or hot cue.
    SavedCuesFirst,
    /// Cue at the first sound.
    FirstSound,
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
    /// Step the effect's parameter up or down, as the BEAT buttons do: the next beat length,
    /// percentage or pitch the effect offers.
    Parameter(Direction),
    /// Put the unit on a deck or the master.
    Target(EffectTarget),
    /// Turn the level and depth knob, from 0.0 to 1.0.
    Depth(f32),
    /// Switch the unit on or off.
    Toggle,
    /// Hold (`true`) or let go of the unit's Release FX, as SHIFT + BEAT FX ON does.
    Release(bool),
    /// Choose the next Release FX: echo, back spin, vinyl brake.
    NextRelease,
}

/// What a stems control does on a deck.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StemAction {
    /// Mute the stem, or bring it back.
    Toggle(Stem),
    /// The vocals out and every other stem in, for an instrumental; back to every stem when the
    /// deck plays one already.
    Instrumental,
    /// Only the vocals, for an acapella; back to every stem when the deck plays one already.
    Acapella,
    /// Only the drums and the bass, a groove to ride under another track; back to every stem
    /// when the deck plays it already.
    Groove,
    /// Every stem back.
    AllStems,
}

impl StemAction {
    /// The stem settings this action leaves after `settings`. Muting keeps each stem's level.
    pub fn apply(self, settings: StemSettings) -> StemSettings {
        let every_stem = Stem::ALL
            .into_iter()
            .fold(settings, |settings, stem| settings.with_muted(stem, false));
        match self {
            StemAction::Toggle(stem) => settings.toggled(stem),
            StemAction::Instrumental if settings.is_without(Stem::Vocals) => every_stem,
            StemAction::Instrumental => settings.without(Stem::Vocals),
            StemAction::Acapella if settings.is_solo(Stem::Vocals) => every_stem,
            StemAction::Acapella => settings.solo(Stem::Vocals),
            StemAction::Groove if is_groove(settings) => every_stem,
            StemAction::Groove => [Stem::Melody, Stem::Vocals]
                .into_iter()
                .fold(every_stem, |settings, stem| settings.with_muted(stem, true)),
            StemAction::AllStems => every_stem,
        }
    }
}

/// Whether only the drums and the bass play.
pub fn is_groove(settings: StemSettings) -> bool {
    Stem::ALL
        .into_iter()
        .all(|stem| settings.is_muted(stem) == matches!(stem, Stem::Melody | Stem::Vocals))
}

/// Something the DJ asks for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Command {
    /// Play or pause a deck.
    TogglePlay(DeckId),
    /// Play or pause the deck the room can hear, as Space does: see [`crate::space`].
    TogglePlayOnAir,
    /// The CDJ cue button of a deck.
    Cue(DeckId, Press),
    /// Fader start: a fader leaving its closed end plays the deck, reaching it cues the deck
    /// back.
    FaderStart(DeckId, FaderStart),
    /// Go back to the start of the track, as SHIFT + CUE does.
    JumpToStart(DeckId),
    /// A performance pad, doing what its pad mode says.
    Pad(PadPress),
    /// Load a built-in sound into an absolute sampler slot.
    LoadBuiltInSample(usize, SynthSound),
    /// Put a deck's pads in a pad mode.
    SetPadMode(DeckId, PadMode),
    ControllerPadMode(DeckId, PadMode),
    CycleSlicer(DeckId),
    /// Put a deck's pads in the next pad mode.
    CyclePadMode(DeckId),
    /// SLIP: scratches and loops let the track play on underneath, and the deck lands back on it.
    ToggleSlip(DeckId),
    /// Lock playing deck: a playing deck refuses a new track, as rekordbox's Eject/Load Lock.
    ToggleLockPlayingDeck,
    /// QUANTIZE: cues, loops, hot cues and beat jumps land on the beat grid, for both decks.
    ToggleQuantize,
    /// VINYL: the jog's top holds the record to scratch, or, off, only nudges as in CDJ mode.
    ToggleVinylMode(DeckId),
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
    /// Open a deck's grid bar to fix its track's beat grid, or close it.
    ToggleGridBar(DeckId),
    /// Change the beat grid of the track on a deck, for every set and deck it is in.
    EditDeckGrid(DeckId, GridEdit),
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
    /// Open or close the Sound Color FX picker of the CFX knobs.
    ToggleCfxMenu,
    /// Choose which channels the Sound Color FX picker sets.
    SetCfxScope(CfxScope),
    /// Give the channels of the picker's scope a Sound Color FX; `None` is the plain filter.
    ChooseCfxColour(Option<SmartCfx>),
    /// SMART FADER: the crossfader moves a shared tempo and swaps the bass.
    ToggleSmartFader,
    /// Play the selected track in the headphones, or stop it: the set track when the keys are on
    /// the set, else the library's.
    PreviewSelected,
    /// Load the selected track, the set track when the keys are on the set, else the library's.
    LoadSelected(DeckId),
    /// Show the selected track's file in Finder, as Finder's Show in Enclosing Folder.
    RevealSelected,
    /// Copy the selected track's file path, as Finder's Copy as Pathname.
    CopySelectedPath,
    /// Separate the selected track's stems now.
    SeparateSelectedStems,
    /// Look for the moved file of the selected track.
    RelinkSelected,
    /// Give the selected track 0 to 5 stars; 0 takes its rating away.
    RateSelected(u8),
    /// Give the selected track a colour label, or take its label away with `None`.
    ColourSelected(Option<TrackColour>),
    /// Put one of the DJ's My Tags on the selected track, or take it off when it has it.
    ToggleSelectedTag(TagId),
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
    /// KEY SYNC: move a deck's key to match the other deck's, or back to the track's own key.
    KeySync(DeckId),
    /// Choose whether a track loaded on a deck starts with key lock on, from Settings.
    SetKeyLockOnNewTracks(bool),
    /// Choose whether a channel fader or crossfader closed fast echoes the outgoing deck, from
    /// Settings.
    SetFaderEcho(bool),
    /// Choose whether fader start plays and cues the decks, as rekordbox's Fader Start.
    SetFaderStart(bool),
    /// Auto Gain on or off, as rekordbox's Preferences > Controller > Mixer > Auto Gain.
    SetAutoGain(bool),
    /// A stems control of a deck: mute or bring back a stem, or a one-touch mix.
    Stems(DeckId, StemAction),
    /// Move a fader or knob to a position from 0.0 to 1.0.
    SetControl(Control, f32),
    /// Move the crossfader one step towards deck A ([`Direction::Down`]) or deck B.
    MoveCrossfader(Direction),
    /// Zoom the scrolling waveforms in ([`Direction::Up`]) or out by one level.
    ZoomLanes(Direction),
    /// Put the scrolling waveforms back at their default zoom.
    ResetLaneZoom,
    /// Put the crossfader in or out of use: out of use, both channels play as if on THRU.
    ToggleCrossfader,
    /// Choose what moves Smart Fader's blend, from Settings.
    SetSmartFaderSource(SmartFaderSource),
    /// Put a deck's channel on side A or B of the crossfader, or THRU, off it.
    SetCrossfaderAssign(DeckId, CrossfaderAssign),
    /// Move a deck's crossfader assign switch to its next position: A, THRU, B.
    CycleCrossfaderAssign(DeckId),
    /// Choose how the crossfader blends its sides.
    SetCrossfaderCurve(CrossfaderCurve),
    /// Choose how the channel faders' positions become their levels.
    SetChannelFaderCurve(ChannelFaderCurve),
    /// Select the row above or below in the browser.
    MoveSelection(Direction),
    /// Select the row above or below as well, from where the selection started: Shift plus an
    /// arrow in the library.
    ExtendSelection(Direction),
    /// Select every track the library shows.
    SelectAllTracks,
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
    /// Export the set to a USB stick or folder for CDJs and XDJs.
    ExportSetToUsb,
    /// Show or hide the list of shortcuts.
    ToggleHelp,
    /// Switch one of the Auto Cue settings on or off.
    ToggleAutoCue(AutoCueChoice),
    /// A deck's SHIFT on the controller went down (`true`) or up.
    ShiftHeld(DeckId, bool),
    /// Show the pads of a deck's Pad FX mode with or without SHIFT on screen, so a click plays
    /// that layer.
    TogglePadLayer(DeckId),
    /// Open, change or close the Pad FX editor.
    PadFx(PadFxEdit),
    /// Show or hide the library's settings: the music folders and rekordbox.
    ToggleLibrary,
    /// Wear a theme, from Settings.
    ChooseTheme(ThemeId),
    /// Bring the rekordbox collection into the library.
    ImportRekordbox,
    /// Look for the files of the tracks that moved, in the music folders.
    RelinkMissingFiles,
    /// Open the My Tag manager, to make, rename, reorder and delete categories and tags.
    OpenTagManager,
    /// Open the folder picker to add music folders to the library.
    AddMusicFolders,
    /// Close the help or leave the search field.
    Dismiss,
}
