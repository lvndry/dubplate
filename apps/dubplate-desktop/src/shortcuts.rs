//! What the keyboard does, so the decks can be played without a controller. The key bindings
//! and the help overlay both come from [`SHORTCUTS`].

use dubplate_engine::DeckId;
use dubplate_rekordbox::HotCuePad;
use gpui::{Action, KeyBinding};

use crate::commands::{Command, Direction, Mode, Press, SetEdit};

/// The key context of the window. Keys that type text only count outside the search field.
pub const WINDOW_CONTEXT: &str = "Dubplate";
const OUTSIDE_TEXT_CONTEXT: &str = "Dubplate && !Input";

/// The action every shortcut sends: one [`Command`].
#[derive(Debug, Clone, PartialEq, Action)]
#[action(namespace = dubplate, no_json)]
pub struct Perform(pub Command);

/// Where a shortcut works.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Anywhere in the window.
    Window,
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
        scope: Scope::Window,
    }
}

const fn pad(keystroke: &'static str, deck: DeckId, pad: HotCuePad) -> Shortcut {
    outside_text(keystroke, Command::HotCue(deck, pad, Press::Tap))
}

/// Every keyboard shortcut.
pub const SHORTCUTS: [Shortcut; 50] = [
    outside_text("space", Command::TogglePlay(DeckId::A)),
    outside_text("shift-space", Command::TogglePlay(DeckId::B)),
    outside_text("c", Command::Cue(DeckId::A, Press::Tap)),
    outside_text("shift-c", Command::Cue(DeckId::B, Press::Tap)),
    pad("1", DeckId::A, HotCuePad::A),
    pad("2", DeckId::A, HotCuePad::B),
    pad("3", DeckId::A, HotCuePad::C),
    pad("4", DeckId::A, HotCuePad::D),
    pad("5", DeckId::A, HotCuePad::E),
    pad("6", DeckId::A, HotCuePad::F),
    pad("7", DeckId::A, HotCuePad::G),
    pad("8", DeckId::A, HotCuePad::H),
    pad("shift-1", DeckId::B, HotCuePad::A),
    pad("shift-2", DeckId::B, HotCuePad::B),
    pad("shift-3", DeckId::B, HotCuePad::C),
    pad("shift-4", DeckId::B, HotCuePad::D),
    pad("shift-5", DeckId::B, HotCuePad::E),
    pad("shift-6", DeckId::B, HotCuePad::F),
    pad("shift-7", DeckId::B, HotCuePad::G),
    pad("shift-8", DeckId::B, HotCuePad::H),
    outside_text("-", Command::NudgeTempo(DeckId::A, Direction::Down)),
    outside_text("=", Command::NudgeTempo(DeckId::A, Direction::Up)),
    outside_text("shift--", Command::NudgeTempo(DeckId::B, Direction::Down)),
    outside_text("shift-=", Command::NudgeTempo(DeckId::B, Direction::Up)),
    outside_text("left", Command::MoveCrossfader(Direction::Down)),
    outside_text("right", Command::MoveCrossfader(Direction::Up)),
    outside_text("shift-left", Command::LoadSelected(DeckId::A)),
    outside_text("shift-right", Command::LoadSelected(DeckId::B)),
    outside_text("up", Command::MoveSelection(Direction::Up)),
    outside_text("down", Command::MoveSelection(Direction::Down)),
    outside_text("/", Command::FocusSearch),
    anywhere("cmd-f", Command::FocusSearch),
    anywhere("ctrl-f", Command::FocusSearch),
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
    outside_text("cmd-z", Command::EditSet(SetEdit::Undo)),
    outside_text("tab", Command::SwitchPrepareFocus),
    anywhere("cmd-l", Command::LoadSet),
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
            let context = match shortcut.scope {
                Scope::Window => WINDOW_CONTEXT,
                Scope::OutsideText => OUTSIDE_TEXT_CONTEXT,
            };
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
    /// The keys, such as `⇧ Space`.
    pub keys: String,
    /// What they do.
    pub description: &'static str,
}

/// The help overlay, one line per pair of deck A and deck B keys. The keys are written from
/// [`SHORTCUTS`], so the help always shows the keys that work.
pub fn help_lines() -> Vec<HelpLine> {
    let line = |keystrokes: &[&str], description| HelpLine {
        keys: keystrokes
            .iter()
            .map(|keystroke| printed_keys(keystroke))
            .collect::<Vec<_>>()
            .join("  /  "),
        description,
    };
    vec![
        line(&["space", "shift-space"], "Play or pause deck A / deck B"),
        line(&["c", "shift-c"], "Cue deck A / deck B"),
        line(
            &["1", "shift-1"],
            "Hot cues A to H with 1 to 8 (sets an empty one), deck A / deck B",
        ),
        line(&["-", "="], "Tempo down / up, deck A"),
        line(&["shift--", "shift-="], "Tempo down / up, deck B"),
        line(&["left", "right"], "Crossfader towards deck A / deck B"),
        line(
            &["shift-left", "shift-right"],
            "Load the selected track on deck A / deck B",
        ),
        line(&["up", "down"], "Select the previous / next track"),
        line(&["/", "cmd-f"], "Search titles and artists"),
        line(&["cmd-1", "cmd-2"], "Prepare / Perform"),
        line(&["enter", "cmd-enter"], "Add to the set / insert after the selected set track"),
        line(&["alt-up", "alt-down"], "Move the selected set track up / down"),
        line(&["backspace", "cmd-z"], "Take out of the set / undo"),
        line(&["f", "y"], "Seam planned or free / accept the proposed cues"),
        line(&["b", "tab"], "Keep as a backup / move between library and set"),
        line(&["cmd-l"], "Load the set's first two tracks on the decks"),
        line(&["cmd-o"], "Add music folders to the library"),
        line(&["cmd-,"], "Library: music folders and rekordbox"),
        line(&["?", "escape"], "Show / close this help"),
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
                Command::HotCue(DeckId::A, pad, press) => Command::HotCue(DeckId::B, pad, press),
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
    fn the_hot_cue_keys_follow_the_pads() {
        let deck_a_pads: Vec<(&str, char)> = SHORTCUTS
            .iter()
            .filter_map(|shortcut| match shortcut.command {
                Command::HotCue(DeckId::A, pad, _) => Some((shortcut.keystroke, pad.letter())),
                _ => None,
            })
            .collect();
        assert_eq!(
            deck_a_pads,
            [
                ("1", 'A'),
                ("2", 'B'),
                ("3", 'C'),
                ("4", 'D'),
                ("5", 'E'),
                ("6", 'F'),
                ("7", 'G'),
                ("8", 'H')
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
    fn the_help_shows_every_key_but_the_summarised_ones() {
        let summarised = |keystroke: &str| {
            let key = keystroke.strip_prefix("shift-").unwrap_or(keystroke);
            matches!(key, "2" | "3" | "4" | "5" | "6" | "7" | "8") || keystroke == "ctrl-f"
        };
        let shown: HashSet<String> = help_lines()
            .iter()
            .flat_map(|line| {
                line.keys
                    .split("  /  ")
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .collect();
        for shortcut in SHORTCUTS {
            if !summarised(shortcut.keystroke) {
                let printed = printed_keys(shortcut.keystroke);
                assert!(shown.contains(&printed), "{}", shortcut.keystroke);
            }
        }
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
