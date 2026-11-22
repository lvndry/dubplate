//! A script that drives a Dubplate running with offline devices, one step per line. Controller
//! messages are decoded with the DDJ-FLX4 mapping and go through the dispatcher as the DJ's
//! would; keys and clicks go through the window as if typed and clicked, so they reach this
//! window alone, whichever application is in front.
//!
//! ```text
//! # a comment
//! midi 96 46 7F            a controller message in hex
//! key space                a key pressed and released, written as in the key bindings
//! press shift              a key pressed and held, until
//! release shift            it is released
//! click 400 300            a left click, in points from the window's top left
//! drag 400 300 520 300     a press, a move and a release
//! ```
//!
//! A named pipe is opened again each time its writer closes, so a test can send steps one at a
//! time and look at the window in between.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use dubplate_midi::{Decoder, Mapping};
use gpui::{
    Context, KeyDownEvent, KeyUpEvent, Keystroke, Modifiers, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, PlatformInput, Point, Window, point, px,
};

use super::DubplateApp;
use crate::hardware::LinkEvent;

/// The controller a script plays.
const SCRIPT_CONTROLLER: &str = "DDJ-FLX4";
/// The port a script's controller shows on.
const SCRIPT_PORT: &str = "script";
/// How many moves a drag takes between its press and its release.
const DRAG_MOVES: u16 = 8;

/// One line of a script.
#[derive(Debug, Clone, PartialEq)]
enum Step {
    Midi(Vec<u8>),
    Key(String),
    Press(String),
    Release(String),
    Click(Point<Pixels>),
    /// A right-click, which opens a context menu.
    RightClick(Point<Pixels>),
    Drag {
        from: Point<Pixels>,
        to: Point<Pixels>,
    },
}

/// What a script line got wrong.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
enum StepError {
    #[error("unknown step {0:?}")]
    UnknownStep(String),
    #[error("{0:?} is not a byte in hex")]
    NotAByte(String),
    #[error("{0:?} is not a number of points")]
    NotAPoint(String),
    #[error("{step} takes {expected}")]
    Arguments {
        step: &'static str,
        expected: &'static str,
    },
}

/// What a script sends the window.
enum ScriptEvent {
    Link(LinkEvent),
    Window(Step),
    Problem(String),
}

fn parse_step(line: &str) -> Result<Option<Step>, StepError> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return Ok(None);
    }
    let (verb, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
    let arguments: Vec<&str> = rest.split_whitespace().collect();
    let step = match verb {
        "midi" => Step::Midi(
            arguments
                .iter()
                .map(|byte| {
                    u8::from_str_radix(byte, 16).map_err(|_| StepError::NotAByte((*byte).into()))
                })
                .collect::<Result<_, _>>()?,
        ),
        "key" | "press" | "release" => {
            let [key] = arguments.as_slice() else {
                return Err(StepError::Arguments {
                    step: "a key step",
                    expected: "one key, such as shift-a",
                });
            };
            let key = (*key).to_owned();
            match verb {
                "key" => Step::Key(key),
                "press" => Step::Press(key),
                _ => Step::Release(key),
            }
        }
        "click" | "rightclick" => {
            let [x, y] = arguments.as_slice() else {
                return Err(StepError::Arguments {
                    step: "a click step",
                    expected: "x and y",
                });
            };
            let at = parse_point(x, y)?;
            if verb == "click" {
                Step::Click(at)
            } else {
                Step::RightClick(at)
            }
        }
        "drag" => {
            let [from_x, from_y, to_x, to_y] = arguments.as_slice() else {
                return Err(StepError::Arguments {
                    step: "drag",
                    expected: "x and y to start from and x and y to end at",
                });
            };
            Step::Drag {
                from: parse_point(from_x, from_y)?,
                to: parse_point(to_x, to_y)?,
            }
        }
        other => return Err(StepError::UnknownStep(other.to_owned())),
    };
    Ok(Some(step))
}

fn parse_point(x: &str, y: &str) -> Result<Point<Pixels>, StepError> {
    let coordinate = |text: &str| {
        text.parse::<f32>()
            .map_err(|_| StepError::NotAPoint(text.to_owned()))
    };
    Ok(point(px(coordinate(x)?), px(coordinate(y)?)))
}

/// Reads the script at `path` on its own thread and plays it into this window.
pub(super) fn listen(path: PathBuf, window: &mut Window, cx: &mut Context<'_, DubplateApp>) {
    let (sender, receiver) = async_channel::unbounded();
    let spawned = std::thread::Builder::new()
        .name("dubplate-script".to_owned())
        .spawn(move || read_script(&path, &sender));
    if let Err(error) = spawned {
        eprintln!("error: the script cannot be read: {error}");
        return;
    }
    cx.spawn_in(window, async move |this, cx| {
        while let Ok(event) = receiver.recv().await {
            // Keys and clicks go to the window outside any update of the app, since the
            // window hands them to the app's own listeners.
            let played = match event {
                ScriptEvent::Link(event) => this.update_in(cx, |this, window, cx| {
                    this.handle_link_events(vec![event], window, cx);
                }),
                ScriptEvent::Window(step) => {
                    cx.update(|window, cx| play_in_window(&step, window, cx))
                }
                ScriptEvent::Problem(problem) => {
                    eprintln!("script: {problem}");
                    Ok(())
                }
            };
            if played.is_err() {
                break;
            }
        }
    })
    .detach();
}

fn read_script(path: &Path, events: &async_channel::Sender<ScriptEvent>) {
    let send = |event| events.send_blocking(event).is_ok();
    let mapping = match Mapping::built_in_for_port(SCRIPT_CONTROLLER) {
        Ok(Some(mapping)) => Arc::new(mapping),
        Ok(None) => {
            send(ScriptEvent::Problem(format!(
                "there is no built-in mapping for {SCRIPT_CONTROLLER}"
            )));
            return;
        }
        Err(error) => {
            send(ScriptEvent::Problem(format!(
                "the {SCRIPT_CONTROLLER} mapping is broken: {error}"
            )));
            return;
        }
    };
    let connected = ScriptEvent::Link(LinkEvent::Connected {
        port: SCRIPT_PORT.to_owned(),
        name: mapping.name().to_owned(),
        jog_ticks_per_revolution: mapping.jog_ticks_per_revolution(),
    });
    if !send(connected) {
        return;
    }
    let mut decoder = Decoder::new(mapping);
    let mut line_number = 0_usize;
    loop {
        let file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(error) => {
                send(ScriptEvent::Problem(format!("{}: {error}", path.display())));
                return;
            }
        };
        let is_pipe = is_named_pipe(&file);
        for line in BufReader::new(file).lines() {
            let line = match line {
                Ok(line) => line,
                Err(error) => {
                    send(ScriptEvent::Problem(format!("{}: {error}", path.display())));
                    return;
                }
            };
            line_number += 1;
            let event = match parse_step(&line) {
                Ok(None) => continue,
                Ok(Some(Step::Midi(bytes))) => match decoder.decode(&bytes) {
                    Some(control) => ScriptEvent::Link(LinkEvent::Control {
                        control,
                        received: Instant::now(),
                    }),
                    None => continue,
                },
                Ok(Some(step)) => ScriptEvent::Window(step),
                Err(error) => ScriptEvent::Problem(format!("line {line_number}: {error}")),
            };
            if !send(event) {
                return;
            }
        }
        if !is_pipe {
            return;
        }
    }
}

#[cfg(unix)]
fn is_named_pipe(file: &std::fs::File) -> bool {
    use std::os::unix::fs::FileTypeExt;
    file.metadata()
        .is_ok_and(|metadata| metadata.file_type().is_fifo())
}

#[cfg(not(unix))]
fn is_named_pipe(_file: &std::fs::File) -> bool {
    false
}

fn play_in_window(step: &Step, window: &mut Window, cx: &mut gpui::App) {
    let os = cx.platform_info().os;
    let keystroke = |key: &str| match Keystroke::parse_with_os(key, os) {
        Ok(keystroke) => Some(keystroke),
        Err(error) => {
            eprintln!("script: {key:?} is not a key: {error}");
            None
        }
    };
    match step {
        Step::Midi(_) => {}
        Step::Key(key) => {
            if let Some(keystroke) = keystroke(key) {
                window.dispatch_keystroke(keystroke.clone(), cx);
                window.dispatch_event(PlatformInput::KeyUp(KeyUpEvent { keystroke }), cx);
            }
        }
        Step::Press(key) => {
            if let Some(keystroke) = keystroke(key) {
                window.dispatch_event(
                    PlatformInput::KeyDown(KeyDownEvent {
                        keystroke,
                        is_held: false,
                        prefer_character_input: false,
                    }),
                    cx,
                );
            }
        }
        Step::Release(key) => {
            if let Some(keystroke) = keystroke(key) {
                window.dispatch_event(PlatformInput::KeyUp(KeyUpEvent { keystroke }), cx);
            }
        }
        Step::Click(position) => {
            move_mouse(*position, None, window, cx);
            press_mouse(*position, MouseButton::Left, window, cx);
            release_mouse(*position, MouseButton::Left, window, cx);
        }
        Step::RightClick(position) => {
            move_mouse(*position, None, window, cx);
            press_mouse(*position, MouseButton::Right, window, cx);
            release_mouse(*position, MouseButton::Right, window, cx);
        }
        Step::Drag { from, to } => {
            move_mouse(*from, None, window, cx);
            press_mouse(*from, MouseButton::Left, window, cx);
            for index in 1..=DRAG_MOVES {
                let progress = f32::from(index) / f32::from(DRAG_MOVES);
                let position = point(
                    from.x + (to.x - from.x) * progress,
                    from.y + (to.y - from.y) * progress,
                );
                move_mouse(position, Some(MouseButton::Left), window, cx);
            }
            release_mouse(*to, MouseButton::Left, window, cx);
        }
    }
}

fn move_mouse(
    position: Point<Pixels>,
    pressed_button: Option<MouseButton>,
    window: &mut Window,
    cx: &mut gpui::App,
) {
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position,
            pressed_button,
            modifiers: Modifiers::default(),
        }),
        cx,
    );
}

fn press_mouse(
    position: Point<Pixels>,
    button: MouseButton,
    window: &mut Window,
    cx: &mut gpui::App,
) {
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            button,
            position,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
}

fn release_mouse(
    position: Point<Pixels>,
    button: MouseButton,
    window: &mut Window,
    cx: &mut gpui::App,
) {
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            button,
            position,
            modifiers: Modifiers::default(),
            click_count: 1,
        }),
        cx,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_lines_become_steps() {
        assert_eq!(parse_step("  # load deck A").unwrap(), None);
        assert_eq!(parse_step("").unwrap(), None);
        assert_eq!(
            parse_step("midi 96 46 7F").unwrap(),
            Some(Step::Midi(vec![0x96, 0x46, 0x7F]))
        );
        assert_eq!(
            parse_step("key shift-a").unwrap(),
            Some(Step::Key("shift-a".to_owned()))
        );
        assert_eq!(
            parse_step("press shift").unwrap(),
            Some(Step::Press("shift".to_owned()))
        );
        assert_eq!(
            parse_step("release shift").unwrap(),
            Some(Step::Release("shift".to_owned()))
        );
        assert_eq!(
            parse_step("click 400 300.5").unwrap(),
            Some(Step::Click(point(px(400.0), px(300.5))))
        );
        assert_eq!(
            parse_step("rightclick 400 300").unwrap(),
            Some(Step::RightClick(point(px(400.0), px(300.0))))
        );
        assert_eq!(
            parse_step("drag 1 2 3 4").unwrap(),
            Some(Step::Drag {
                from: point(px(1.0), px(2.0)),
                to: point(px(3.0), px(4.0)),
            })
        );
    }

    #[test]
    fn a_wrong_line_says_what_is_wrong() {
        assert_eq!(
            parse_step("midi 96 GG").unwrap_err(),
            StepError::NotAByte("GG".to_owned())
        );
        assert_eq!(
            parse_step("tap 1").unwrap_err(),
            StepError::UnknownStep("tap".to_owned())
        );
        assert!(matches!(
            parse_step("click 1").unwrap_err(),
            StepError::Arguments {
                step: "a click step",
                ..
            }
        ));
        assert_eq!(
            parse_step("click left 1").unwrap_err(),
            StepError::NotAPoint("left".to_owned())
        );
        assert!(matches!(
            parse_step("key a b").unwrap_err(),
            StepError::Arguments { .. }
        ));
    }

    #[test]
    fn a_script_file_plays_its_controller_steps_through_the_mapping() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("script");
        std::fs::write(&path, "midi 96 46 7F\nkey space\nfly away\n").unwrap();
        let (sender, receiver) = async_channel::unbounded();
        read_script(&path, &sender);
        drop(sender);
        let events: Vec<ScriptEvent> = std::iter::from_fn(|| receiver.try_recv().ok()).collect();
        assert!(matches!(
            &events[0],
            ScriptEvent::Link(LinkEvent::Connected { name, .. }) if name.ends_with(SCRIPT_CONTROLLER)
        ));
        assert!(matches!(
            &events[1],
            ScriptEvent::Link(LinkEvent::Control { .. })
        ));
        assert!(matches!(&events[2], ScriptEvent::Window(Step::Key(key)) if key == "space"));
        assert!(
            matches!(&events[3], ScriptEvent::Problem(problem) if problem.starts_with("line 3:"))
        );
        assert_eq!(events.len(), 4);
    }
}
