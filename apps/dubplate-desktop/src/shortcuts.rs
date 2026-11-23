//! What the keyboard does, so the decks can be played without a controller. The key bindings
//! and the help overlay both come from [`SHORTCUTS`].

use dubplate_engine::{DeckId, Stem};
use dubplate_prep::GridEdit;
use gpui::{Action, KeyBinding};

use crate::commands::{Command, Direction, LoopAction, Mode, PadPress, Press, SetEdit, StemAction};

/// How far one press of [ or ] moves a track's grid, in milliseconds.
pub const GRID_NUDGE_MS: i32 = 5;

/// The key context of the window. Keys that type text only count outside the search field.
pub const WINDOW_CONTEXT: &str = "Dubplate";
const OUTSIDE_TEXT_CONTEXT: &str = "Dubplate && !Input && !Modal";
const OUTSIDE_CONTROL_CONTEXT: &str = "Dubplate && !Input && !Control && !Modal";

/// The action every shortcut sends: one [`Command`].
#[derive(Debug, Clone, PartialEq, Action)]
#[action(namespace = dubplate, no_json)]
pub struct Perform(pub Command);

/// Where a shortcut works.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Anywhere in the window.
    Window,
    /// Anywhere except a modal dialog.
    OutsideModal,
    /// Outside the search field, where the key would type text.
    OutsideText,
}

/// A key and what it does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shortcut {
    /// The keystroke in GPUI's notation, such as `shift-space`.
    pub keystroke: &'static str,
    /// What it does.
    pub command: Command,
    /// Where it works.
    pub scope: Scope,
}

const fn outside_text(keystroke: &'static str, command: Command) -> Shortcut {
    Shortcut {
        keystroke,
        command,
        scope: Scope::OutsideText,
    }
}

const fn anywhere(keystroke: &'static str, command: Command) -> Shortcut {
    Shortcut {
        keystroke,
        command,
        scope: match command {
            Command::FocusSearch
            | Command::SwitchMode(_)
            | Command::LoadSet
            | Command::ExportSet
            | Command::ImportFromUsb
            | Command::ExportSetToUsb
            | Command::AddMusicFolders => Scope::OutsideModal,
            _ => Scope::Window,
        },
    }
}

const fn suggestion(keystroke: &'static str, rank: usize) -> Shortcut {
    outside_text(keystroke, Command::EditSet(SetEdit::InsertSuggestion(rank)))
}

const fn pad(keystroke: &'static str, deck: DeckId, pad: usize) -> Shortcut {
    outside_text(
        keystroke,
        Command::Pad(PadPress {
            deck,
            pad,
            mode: None,
            press: Press::Tap,
            shift: false,
        }),
    )
}

/// Every keyboard shortcut.
pub const SHORTCUTS: &[Shortcut] = &[
    outside_text("space", Command::TogglePlayOnAir),
    outside_text("shift-space", Command::TogglePlay(DeckId::B)),
    outside_text("c", Command::Cue(DeckId::A, Press::Tap)),
    outside_text("shift-c", Command::Cue(DeckId::B, Press::Tap)),
    pad("1", DeckId::A, 0),
    pad("2", DeckId::A, 1),
    pad("3", DeckId::A, 2),
    pad("4", DeckId::A, 3),
    pad("5", DeckId::A, 4),
    pad("6", DeckId::A, 5),
    pad("7", DeckId::A, 6),
    pad("8", DeckId::A, 7),
    pad("shift-1", DeckId::B, 0),
    pad("shift-2", DeckId::B, 1),
    pad("shift-3", DeckId::B, 2),
    pad("shift-4", DeckId::B, 3),
    pad("shift-5", DeckId::B, 4),
    pad("shift-6", DeckId::B, 5),
    pad("shift-7", DeckId::B, 6),
    pad("shift-8", DeckId::B, 7),
    outside_text("-", Command::NudgeTempo(DeckId::A, Direction::Down)),
    outside_text("=", Command::NudgeTempo(DeckId::A, Direction::Up)),
    outside_text("shift--", Command::NudgeTempo(DeckId::B, Direction::Down)),
    outside_text("shift-=", Command::NudgeTempo(DeckId::B, Direction::Up)),
    outside_text("alt-x", Command::ToggleCrossfader),
    outside_text("z", Command::CycleCrossfaderAssign(DeckId::A)),
    outside_text("shift-z", Command::CycleCrossfaderAssign(DeckId::B)),
    outside_text("left", Command::MoveCrossfader(Direction::Down)),
    outside_text("right", Command::MoveCrossfader(Direction::Up)),
    outside_text("p", Command::PreviewSelected),
    outside_text("alt-cmd-r", Command::RevealSelected),
    outside_text("alt-cmd-c", Command::CopySelectedPath),
    outside_text("ctrl-0", Command::RateSelected(0)),
    outside_text("ctrl-1", Command::RateSelected(1)),
    outside_text("ctrl-2", Command::RateSelected(2)),
    outside_text("ctrl-3", Command::RateSelected(3)),
    outside_text("ctrl-4", Command::RateSelected(4)),
    outside_text("ctrl-5", Command::RateSelected(5)),
    outside_text("shift-left", Command::LoadSelected(DeckId::A)),
    outside_text("shift-right", Command::LoadSelected(DeckId::B)),
    outside_text("up", Command::MoveSelection(Direction::Up)),
    outside_text("down", Command::MoveSelection(Direction::Down)),
    outside_text("shift-up", Command::ExtendSelection(Direction::Up)),
    outside_text("shift-down", Command::ExtendSelection(Direction::Down)),
    outside_text("cmd-a", Command::SelectAllTracks),
    outside_text("/", Command::FocusSearch),
    anywhere("cmd-f", Command::FocusSearch),
    anywhere("ctrl-f", Command::FocusSearch),
    anywhere("cmd-=", Command::ZoomLanes(Direction::Up)),
    anywhere("cmd--", Command::ZoomLanes(Direction::Down)),
    anywhere("cmd-0", Command::ResetLaneZoom),
    anywhere("cmd-1", Command::SwitchMode(Mode::Prepare)),
    anywhere("cmd-2", Command::SwitchMode(Mode::Perform)),
    outside_text("enter", Command::EditSet(SetEdit::Add)),
    outside_text("cmd-enter", Command::EditSet(SetEdit::InsertAfterSelection)),
    outside_text("alt-up", Command::EditSet(SetEdit::Move(Direction::Up))),
    outside_text("alt-down", Command::EditSet(SetEdit::Move(Direction::Down))),
    outside_text("backspace", Command::EditSet(SetEdit::Remove)),
    outside_text("f", Command::EditSet(SetEdit::TogglePlanned)),
    outside_text("y", Command::EditSet(SetEdit::AcceptProposedCues)),
    outside_text("b", Command::EditSet(SetEdit::ToggleBackup)),
    outside_text(
        "g",
        Command::EditSet(SetEdit::Grid(GridEdit::ToggleConfirmed)),
    ),
    outside_text("i", Command::EditSet(SetEdit::MixInAtPlayhead)),
    outside_text("o", Command::EditSet(SetEdit::MixOutAtPlayhead)),
    outside_text(
        "[",
        Command::EditSet(SetEdit::Grid(GridEdit::Nudge(-GRID_NUDGE_MS))),
    ),
    outside_text(
        "]",
        Command::EditSet(SetEdit::Grid(GridEdit::Nudge(GRID_NUDGE_MS))),
    ),
    outside_text(
        ",",
        Command::EditSet(SetEdit::Grid(GridEdit::ShiftDownbeat(-1))),
    ),
    outside_text(
        ".",
        Command::EditSet(SetEdit::Grid(GridEdit::ShiftDownbeat(1))),
    ),
    outside_text("cmd-z", Command::EditSet(SetEdit::Undo)),
    Shortcut {
        keystroke: "ctrl-tab",
        command: Command::SwitchPrepareFocus(Direction::Down),
        scope: Scope::OutsideModal,
    },
    Shortcut {
        keystroke: "ctrl-shift-tab",
        command: Command::SwitchPrepareFocus(Direction::Up),
        scope: Scope::OutsideModal,
    },
    suggestion("alt-1", 0),
    suggestion("alt-2", 1),
    suggestion("alt-3", 2),
    suggestion("alt-4", 3),
    suggestion("alt-5", 4),
    anywhere("cmd-l", Command::LoadSet),
    anywhere("cmd-e", Command::ExportSet),
    anywhere("cmd-shift-i", Command::ImportFromUsb),
    anywhere("cmd-shift-e", Command::ExportSetToUsb),
    outside_text("s", Command::AuditionSeam),
    anywhere("cmd-shift-r", Command::ToggleRecording),
    outside_text("t", Command::TapTempo(DeckId::A)),
    outside_text("shift-t", Command::TapTempo(DeckId::B)),
    outside_text("e", Command::ToggleGridBar(DeckId::A)),
    outside_text("shift-e", Command::ToggleGridBar(DeckId::B)),
    outside_text("k", Command::ToggleKeyLock(DeckId::A)),
    outside_text("shift-k", Command::ToggleKeyLock(DeckId::B)),
    outside_text("alt-k", Command::KeySync(DeckId::A)),
    outside_text("shift-alt-k", Command::KeySync(DeckId::B)),
    outside_text(
        "q",
        Command::Stems(DeckId::A, StemAction::Toggle(Stem::Drums)),
    ),
    outside_text(
        "shift-q",
        Command::Stems(DeckId::B, StemAction::Toggle(Stem::Drums)),
    ),
    outside_text(
        "w",
        Command::Stems(DeckId::A, StemAction::Toggle(Stem::Bass)),
    ),
    outside_text(
        "shift-w",
        Command::Stems(DeckId::B, StemAction::Toggle(Stem::Bass)),
    ),
    outside_text(
        "r",
        Command::Stems(DeckId::A, StemAction::Toggle(Stem::Melody)),
    ),
    outside_text(
        "shift-r",
        Command::Stems(DeckId::B, StemAction::Toggle(Stem::Melody)),
    ),
    outside_text(
        "v",
        Command::Stems(DeckId::A, StemAction::Toggle(Stem::Vocals)),
    ),
    outside_text(
        "shift-v",
        Command::Stems(DeckId::B, StemAction::Toggle(Stem::Vocals)),
    ),
    outside_text("x", Command::Stems(DeckId::A, StemAction::Instrumental)),
    outside_text(
        "shift-x",
        Command::Stems(DeckId::B, StemAction::Instrumental),
    ),
    outside_text("a", Command::Stems(DeckId::A, StemAction::Acapella)),
    outside_text("shift-a", Command::Stems(DeckId::B, StemAction::Acapella)),
    outside_text("alt-q", Command::ToggleQuantize),
    outside_text("alt-v", Command::ToggleVinylMode(DeckId::A)),
    outside_text("shift-alt-v", Command::ToggleVinylMode(DeckId::B)),
    outside_text("u", Command::ToggleSlip(DeckId::A)),
    outside_text("shift-u", Command::ToggleSlip(DeckId::B)),
    outside_text("d", Command::CycleSlicer(DeckId::A)),
    outside_text("shift-d", Command::CycleSlicer(DeckId::B)),
    outside_text("m", Command::CyclePadMode(DeckId::A)),
    outside_text("shift-m", Command::CyclePadMode(DeckId::B)),
    outside_text("l", Command::Loop(DeckId::A, LoopAction::FourBeatOrExit)),
    outside_text(
        "shift-l",
        Command::Loop(DeckId::B, LoopAction::FourBeatOrExit),
    ),
    outside_text("h", Command::CueLoopCall(DeckId::A, Direction::Down)),
    outside_text("shift-h", Command::CueLoopCall(DeckId::B, Direction::Down)),
    outside_text("j", Command::CueLoopCall(DeckId::A, Direction::Up)),
    outside_text("shift-j", Command::CueLoopCall(DeckId::B, Direction::Up)),
    outside_text("n", Command::StoreMemoryCue(DeckId::A)),
    outside_text("shift-n", Command::StoreMemoryCue(DeckId::B)),
    outside_text("alt-n", Command::DeleteMemoryCue(DeckId::A)),
    outside_text("shift-alt-n", Command::DeleteMemoryCue(DeckId::B)),
    outside_text("?", Command::ToggleHelp),
    anywhere("cmd-,", Command::ToggleLibrary),
    anywhere("cmd-o", Command::AddMusicFolders),
    anywhere("escape", Command::Dismiss),
];

/// The key bindings of [`SHORTCUTS`], for `App::bind_keys`. A shifted digit or symbol also
/// gets the character macOS reports for it, so ⇧3 works as `#` does.
pub fn key_bindings() -> Vec<KeyBinding> {
    SHORTCUTS
        .iter()
        .flat_map(|shortcut| {
            let context = binding_context(shortcut);
            [
                Some(shortcut.keystroke),
                shifted_character(shortcut.keystroke),
            ]
            .into_iter()
            .flatten()
            .map(move |keystroke| {
                KeyBinding::new(keystroke, Perform(shortcut.command), Some(context))
            })
        })
        .collect()
}

/// Focused controls keep their activation and editing keys, while non-conflicting DJ
/// shortcuts (including pad digits and Option-digit suggestions) remain available. A control
/// only reads those keys bare or with Shift, so Command, Control and Option chords such as
/// Option-Up or Command-Enter always reach the window.
fn binding_context(shortcut: &Shortcut) -> &'static str {
    match shortcut.scope {
        Scope::Window => WINDOW_CONTEXT,
        Scope::OutsideModal => "Dubplate && !Modal",
        Scope::OutsideText => {
            let key = shortcut
                .keystroke
                .rsplit('-')
                .next()
                .unwrap_or(shortcut.keystroke);
            let chorded = ["cmd-", "ctrl-", "alt-"]
                .iter()
                .any(|modifier| shortcut.keystroke.contains(modifier));
            if !chorded
                && matches!(
                    key,
                    "space"
                        | "enter"
                        | "left"
                        | "right"
                        | "up"
                        | "down"
                        | "home"
                        | "end"
                        | "backspace"
                        | "tab"
                )
            {
                OUTSIDE_CONTROL_CONTEXT
            } else {
                OUTSIDE_TEXT_CONTEXT
            }
        }
    }
}

/// The character a shifted digit or symbol types on a US keyboard, which is the keystroke macOS
/// reports for it: `#` for `shift-3`.
fn shifted_character(keystroke: &str) -> Option<&'static str> {
    Some(match keystroke {
        "shift-1" => "!",
        "shift-2" => "@",
        "shift-3" => "#",
        "shift-4" => "$",
        "shift-5" => "%",
        "shift-6" => "^",
        "shift-7" => "&",
        "shift-8" => "*",
        "shift--" => "_",
        "shift-=" => "+",
        _ => return None,
    })
}

/// A line of the help overlay: the keys, as printed on a keyboard, and what they do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelpLine {
    /// Each keystroke, such as `⇧ Space`, in the order the description names them.
    pub keys: Vec<String>,
    /// What they do.
    pub description: &'static str,
}

/// A group of help lines about one part of Dubplate, such as the decks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelpSection {
    /// The part, such as `Decks`.
    pub title: &'static str,
    /// Its lines.
    pub lines: Vec<HelpLine>,
}

/// The help overlay, one line per pair of deck A and deck B keys, grouped by the part of
/// Dubplate they work on. The keys are written from [`SHORTCUTS`], so the help always shows the
/// keys that work.
pub fn help_sections() -> Vec<HelpSection> {
    let line = |keystrokes: &[&str], description| HelpLine {
        keys: keystrokes
            .iter()
            .map(|keystroke| printed_keys(keystroke))
            .collect(),
        description,
    };
    vec![
        HelpSection {
            title: "Decks",
            lines: vec![
                line(
                    &["space", "shift-space"],
                    "Play or pause the deck you can hear (A when neither) / deck B",
                ),
                line(&["c", "shift-c"], "Cue deck A / deck B"),
                line(
                    &["1", "shift-1"],
                    "Pads 1 to 8 in the deck's pad mode (hot cues set an empty one), deck A / deck B",
                ),
                line(&["d", "shift-d"], "Cycle slicer mode, deck A / deck B"),
                line(&["m", "shift-m"], "Next pad mode, deck A / deck B"),
                line(&["-", "="], "Tempo down / up, deck A"),
                line(&["shift--", "shift-="], "Tempo down / up, deck B"),
                line(
                    &["k", "shift-k"],
                    "Key lock for the track on deck A / deck B (new tracks: Settings)",
                ),
                line(
                    &["alt-k", "shift-alt-k"],
                    "Key sync to the other deck, or back to the track's key, deck A / deck B",
                ),
                line(&["q", "shift-q"], "Drums stem out or back, deck A / deck B"),
                line(&["w", "shift-w"], "Bass stem out or back, deck A / deck B"),
                line(
                    &["r", "shift-r"],
                    "Melody stem out or back, deck A / deck B",
                ),
                line(
                    &["v", "shift-v"],
                    "Vocals stem out or back, deck A / deck B",
                ),
                line(
                    &["x", "shift-x"],
                    "Instrumental, the vocals out, deck A / deck B",
                ),
                line(
                    &["a", "shift-a"],
                    "Acapella, only the vocals, deck A / deck B",
                ),
                line(
                    &["e", "shift-e"],
                    "Fix the beat grid with the grid keys, deck A / deck B",
                ),
                line(&["u", "shift-u"], "Slip, deck A / deck B"),
                line(
                    &["alt-v", "shift-alt-v"],
                    "Vinyl mode (jog top scratches) or CDJ mode, deck A / deck B",
                ),
                line(&["alt-q"], "Quantize on or off, both decks"),
                line(&["l", "shift-l"], "Four-beat loop or exit, deck A / deck B"),
                line(
                    &["h", "j"],
                    "Call the memory cue before / after the playhead, or halve / double the loop that is on, deck A",
                ),
                line(
                    &["shift-h", "shift-j"],
                    "Call the memory cue before / after the playhead, or halve / double the loop that is on, deck B",
                ),
                line(
                    &["n", "shift-n"],
                    "Keep the cue point or the loop that is on as a memory cue, deck A / deck B",
                ),
                line(
                    &["alt-n", "shift-alt-n"],
                    "Delete the memory cue at the cue point, deck A / deck B",
                ),
                line(
                    &["t", "shift-t"],
                    "Tap the tempo of a track without a grid, deck A / deck B",
                ),
                line(&["left", "right"], "Crossfader towards deck A / deck B"),
                line(
                    &["cmd-=", "cmd--", "cmd-0"],
                    "Zoom the waveforms in / out / back (Shift + BROWSE on the FLX4)",
                ),
                line(
                    &["z", "shift-z"],
                    "Crossfader assign A, THRU or B, deck A / deck B",
                ),
                line(&["alt-x"], "Crossfader on or off"),
            ],
        },
        HelpSection {
            title: "Library",
            lines: vec![
                line(&["up", "down"], "Select the previous / next track"),
                line(
                    &["shift-up", "shift-down"],
                    "Add the previous / next track to the selection",
                ),
                line(&["cmd-a"], "Select every track shown"),
                line(
                    &["/", "cmd-f"],
                    "Search titles, artists, genres, folders and comments",
                ),
                line(
                    &["shift-left", "shift-right"],
                    "Load the selected track on deck A / deck B",
                ),
                line(
                    &["p"],
                    "Preview the selected track in the headphones, or stop",
                ),
                line(
                    &["alt-cmd-r", "alt-cmd-c"],
                    "Show the selected track in Finder / copy its file path",
                ),
                line(
                    &["ctrl-0", "ctrl-1"],
                    "Rate the selected track: no stars / one star (⌃ 2 to ⌃ 5 for two to five stars)",
                ),
            ],
        },
        HelpSection {
            title: "Set",
            lines: vec![
                line(
                    &["enter", "cmd-enter"],
                    "Add to the set / insert after the selected set track",
                ),
                line(
                    &["alt-up", "alt-down"],
                    "Move the selected set track up / down",
                ),
                line(&["backspace", "cmd-z"], "Take out of the set / undo"),
                line(&["b"], "Keep as a backup"),
                line(
                    &["ctrl-tab", "ctrl-shift-tab"],
                    "Move to the library, the set and Next in turn / back",
                ),
                line(
                    &["alt-1"],
                    "Insert Next suggestion 1 to 5 after the selected set track",
                ),
                line(&["f"], "Seam after the selected set track planned or free"),
                line(
                    &["s"],
                    "Hear the seam after the selected set track in the headphones",
                ),
                line(&["cmd-l"], "Load the set's first two tracks on the decks"),
                line(&["cmd-e"], "Export the set to rekordbox"),
                line(&["cmd-shift-i"], "Import CDJ histories and cues from USB"),
                line(&["cmd-shift-e"], "Export the set to a USB stick for CDJs"),
            ],
        },
        HelpSection {
            title: "Grid and track check",
            lines: vec![
                line(
                    &["g", "y"],
                    "Confirm the grid (the open deck grid in Perform) / accept the proposed cues",
                ),
                line(&["[", "]"], "Grid 5 ms earlier / later"),
                line(&[",", "."], "Downbeat one beat earlier / later"),
                line(&["i", "o"], "Mix-in / mix-out on the bar at the playhead"),
            ],
        },
        HelpSection {
            title: "Dubplate",
            lines: vec![
                line(&["cmd-1", "cmd-2"], "Prepare / Perform"),
                line(&["cmd-shift-r"], "Record the mix, or stop recording"),
                line(&["cmd-o"], "Add music folders to the library"),
                line(
                    &["cmd-,"],
                    "Settings: music folders, rekordbox, sound card, theme",
                ),
                line(&["?", "escape"], "Show / close this help"),
            ],
        },
    ]
}

/// A keystroke as printed on the keys, such as `⇧ ←` for `shift-left`.
pub fn printed_keys(keystroke: &str) -> String {
    let (modifiers, key) = match keystroke.strip_suffix('-') {
        Some(modifiers) => (modifiers.strip_suffix('-').unwrap_or(modifiers), "-"),
        None => keystroke.rsplit_once('-').unwrap_or(("", keystroke)),
    };
    modifiers
        .split('-')
        .chain([key])
        .filter(|part| !part.is_empty())
        .map(|part| match part {
            "shift" => "⇧".to_owned(),
            "cmd" => "⌘".to_owned(),
            "ctrl" => "⌃".to_owned(),
            "alt" => "⌥".to_owned(),
            "space" => "Space".to_owned(),
            "left" => "←".to_owned(),
            "right" => "→".to_owned(),
            "up" => "↑".to_owned(),
            "down" => "↓".to_owned(),
            "escape" => "Esc".to_owned(),
            "enter" => "↩".to_owned(),
            "backspace" => "⌫".to_owned(),
            "tab" => "⇥".to_owned(),
            other => other.to_uppercase(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn no_key_does_two_things() {
        let mut seen = HashSet::new();
        for shortcut in SHORTCUTS {
            assert!(seen.insert(shortcut.keystroke), "{}", shortcut.keystroke);
        }
    }

    #[test]
    fn every_deck_a_command_has_a_deck_b_twin_on_shift() {
        for shortcut in SHORTCUTS {
            let twin_command = match shortcut.command {
                Command::TogglePlay(DeckId::A) => Command::TogglePlay(DeckId::B),
                Command::Cue(DeckId::A, press) => Command::Cue(DeckId::B, press),
                Command::Pad(
                    press @ PadPress {
                        deck: DeckId::A, ..
                    },
                ) => Command::Pad(PadPress {
                    deck: DeckId::B,
                    ..press
                }),
                Command::CycleSlicer(DeckId::A) => Command::CycleSlicer(DeckId::B),
                Command::CyclePadMode(DeckId::A) => Command::CyclePadMode(DeckId::B),
                Command::ToggleKeyLock(DeckId::A) => Command::ToggleKeyLock(DeckId::B),
                Command::KeySync(DeckId::A) => Command::KeySync(DeckId::B),
                Command::CycleCrossfaderAssign(DeckId::A) => {
                    Command::CycleCrossfaderAssign(DeckId::B)
                }
                Command::Stems(DeckId::A, action) => Command::Stems(DeckId::B, action),
                Command::ToggleGridBar(DeckId::A) => Command::ToggleGridBar(DeckId::B),
                Command::TapTempo(DeckId::A) => Command::TapTempo(DeckId::B),
                Command::ToggleSlip(DeckId::A) => Command::ToggleSlip(DeckId::B),
                Command::ToggleVinylMode(DeckId::A) => Command::ToggleVinylMode(DeckId::B),

                Command::Loop(DeckId::A, action) => Command::Loop(DeckId::B, action),
                Command::CueLoopCall(DeckId::A, direction) => {
                    Command::CueLoopCall(DeckId::B, direction)
                }
                Command::StoreMemoryCue(DeckId::A) => Command::StoreMemoryCue(DeckId::B),
                Command::DeleteMemoryCue(DeckId::A) => Command::DeleteMemoryCue(DeckId::B),
                Command::NudgeTempo(DeckId::A, direction) => {
                    Command::NudgeTempo(DeckId::B, direction)
                }
                _ => continue,
            };
            let twin_keystroke = format!("shift-{}", shortcut.keystroke);
            assert!(
                SHORTCUTS
                    .iter()
                    .any(|twin| twin.keystroke == twin_keystroke && twin.command == twin_command),
                "{twin_keystroke}"
            );
        }
    }

    #[test]
    fn the_pad_keys_follow_the_pads() {
        let deck_a_pads: Vec<(&str, usize)> = SHORTCUTS
            .iter()
            .filter_map(|shortcut| match shortcut.command {
                Command::Pad(PadPress {
                    deck: DeckId::A,
                    pad,
                    ..
                }) => Some((shortcut.keystroke, pad)),
                _ => None,
            })
            .collect();
        assert_eq!(
            deck_a_pads,
            [
                ("1", 0),
                ("2", 1),
                ("3", 2),
                ("4", 3),
                ("5", 4),
                ("6", 5),
                ("7", 6),
                ("8", 7)
            ]
        );
    }

    #[test]
    fn keys_that_type_text_leave_the_search_field_alone() {
        for shortcut in SHORTCUTS {
            let types_text = !shortcut.keystroke.contains("cmd-")
                && !shortcut.keystroke.contains("ctrl-")
                && shortcut.keystroke != "escape";
            if types_text {
                assert_eq!(shortcut.scope, Scope::OutsideText, "{}", shortcut.keystroke);
            }
        }
    }

    #[test]
    fn focused_controls_keep_editing_keys_and_leave_dj_shortcuts_available() {
        for key in [
            "m",
            "c",
            "1",
            "shift-1",
            "alt-1",
            "alt-up",
            "cmd-enter",
            "cmd-z",
        ] {
            let shortcut = SHORTCUTS
                .iter()
                .find(|shortcut| shortcut.keystroke == key)
                .expect("shortcut exists");
            assert_eq!(binding_context(shortcut), OUTSIDE_TEXT_CONTEXT, "{key}");
        }
        for key in ["space", "enter", "left", "shift-right", "up", "backspace"] {
            let shortcut = SHORTCUTS
                .iter()
                .find(|shortcut| shortcut.keystroke == key)
                .expect("shortcut exists");
            assert_eq!(binding_context(shortcut), OUTSIDE_CONTROL_CONTEXT, "{key}");
        }
        assert!(
            !SHORTCUTS
                .iter()
                .any(|shortcut| matches!(shortcut.keystroke, "tab" | "shift-tab"))
        );
    }

    #[test]
    fn the_help_shows_every_key_but_the_summarised_ones() {
        let summarised = |keystroke: &str| {
            let key = keystroke
                .strip_prefix("shift-")
                .or_else(|| keystroke.strip_prefix("alt-"))
                .unwrap_or(keystroke);
            matches!(key, "2" | "3" | "4" | "5" | "6" | "7" | "8")
                || matches!(
                    keystroke,
                    "ctrl-2" | "ctrl-3" | "ctrl-4" | "ctrl-5" | "ctrl-f"
                )
        };
        let shown: HashSet<String> = help_sections()
            .into_iter()
            .flat_map(|section| section.lines)
            .flat_map(|line| line.keys)
            .collect();
        for shortcut in SHORTCUTS {
            if !summarised(shortcut.keystroke) {
                let printed = printed_keys(shortcut.keystroke);
                assert!(shown.contains(&printed), "{}", shortcut.keystroke);
            }
        }
    }

    #[test]
    fn control_and_a_digit_rate_the_selected_track() {
        for stars in 0..=5u8 {
            let keystroke = format!("ctrl-{stars}");
            let shortcut = SHORTCUTS
                .iter()
                .find(|shortcut| shortcut.keystroke == keystroke)
                .map(|shortcut| shortcut.command);
            assert_eq!(shortcut, Some(Command::RateSelected(stars)), "{keystroke}");
        }
        assert!(help_sections().into_iter().any(|section| {
            section
                .lines
                .iter()
                .any(|line| line.description.contains("Rate the selected track"))
        }));
    }

    #[test]
    fn option_and_a_digit_insert_that_next_suggestion() {
        let ranks: Vec<(&str, usize)> = SHORTCUTS
            .iter()
            .filter_map(|shortcut| match shortcut.command {
                Command::EditSet(SetEdit::InsertSuggestion(rank)) => {
                    Some((shortcut.keystroke, rank))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            ranks,
            [
                ("alt-1", 0),
                ("alt-2", 1),
                ("alt-3", 2),
                ("alt-4", 3),
                ("alt-5", 4)
            ]
        );
    }

    #[test]
    fn keys_print_as_on_the_keyboard() {
        assert_eq!(printed_keys("shift-space"), "⇧ Space");
        assert_eq!(printed_keys("shift-left"), "⇧ ←");
        assert_eq!(printed_keys("cmd-f"), "⌘ F");
        assert_eq!(printed_keys("-"), "-");
        assert_eq!(printed_keys("shift--"), "⇧ -");
        assert_eq!(printed_keys("shift-="), "⇧ =");
        assert_eq!(printed_keys("c"), "C");
        assert_eq!(printed_keys("cmd-enter"), "⌘ ↩");
        assert_eq!(printed_keys("backspace"), "⌫");
    }

    #[test]
    fn shifted_digits_and_symbols_also_bind_the_character_they_type() {
        let shifted = SHORTCUTS
            .iter()
            .filter(|shortcut| shifted_character(shortcut.keystroke).is_some())
            .count();
        assert_eq!(shifted, 10);
        assert_eq!(shifted_character("shift-3"), Some("#"));
        assert_eq!(shifted_character("shift-="), Some("+"));
        assert_eq!(shifted_character("shift-c"), None);
        assert_eq!(key_bindings().len(), SHORTCUTS.len() + shifted);
    }
}
