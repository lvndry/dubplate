//! Turning what the DJ does on a controller into [`Command`]s, the same commands the keyboard
//! and the mouse send, following how rekordbox drives a Pioneer controller.

use std::time::{Duration, Instant};

use dubplate_engine::{DECK_COUNT, DeckId};
use dubplate_midi::{
    Action, Button, ButtonState, Continuous, ControlEvent, Deck, Encoder, Layer, PadIndex, PadMode,
    Position,
};
use dubplate_rekordbox::HotCuePad;

use crate::commands::{Command, Press};
use crate::controls::{Control, MixerPositions};

/// Two presses of LOAD this close together load the other deck's track, as rekordbox does.
const DOUBLE_PRESS: Duration = Duration::from_millis(400);
/// A tempo slider this close to the tempo on screen picks it up.
const PICKUP_DISTANCE: f32 = 0.01;

/// What the dispatcher reads of the app to decide what a control does.
#[derive(Debug, Clone, Copy, Default)]
pub struct Surface {
    /// Where the faders and knobs are on screen.
    pub positions: MixerPositions,
    /// Whether each deck is playing.
    pub playing: [bool; DECK_COUNT],
}

/// The engine's deck for a controller deck.
pub fn deck_id(deck: Deck) -> DeckId {
    match deck {
        Deck::A => DeckId::A,
        Deck::B => DeckId::B,
    }
}

/// The controller deck for an engine deck.
pub fn controller_deck(deck: DeckId) -> Deck {
    match deck {
        DeckId::A => Deck::A,
        DeckId::B => Deck::B,
    }
}

/// The hot cue a pad sets and calls, pad 1 being A.
pub fn hot_cue_pad(pad: PadIndex) -> Option<HotCuePad> {
    HotCuePad::ALL
        .get(usize::from(pad.number()).checked_sub(1)?)
        .copied()
}

/// A hardware control that only takes over the value on screen once it reaches it, so a slider
/// left elsewhere does not make the tempo jump. This is the pick-up of rekordbox's tempo slider.
#[derive(Debug, Clone, Copy, Default)]
struct Pickup {
    last_hardware: Option<f32>,
}

impl Pickup {
    /// Whether the control at `hardware` should set a value now on screen at `shown`. A control
    /// that is `free` to jump, such as the tempo of a stopped deck, always takes over.
    fn takes_over(&mut self, hardware: f32, shown: f32, free: bool) -> bool {
        let previous = self.last_hardware.replace(hardware);
        let close = (hardware - shown).abs() <= PICKUP_DISTANCE;
        let was_holding =
            previous.is_some_and(|previous| (previous - shown).abs() <= PICKUP_DISTANCE);
        let crossed = previous.is_some_and(|previous| (previous < shown) != (hardware < shown));
        free || close || was_holding || crossed
    }
}

/// The controller's side of the conversation: which pad mode each deck is in, LOAD presses for
/// instant doubles, and the tempo sliders' pick-up.
#[derive(Debug)]
pub struct Dispatcher {
    pad_modes: [PadMode; DECK_COUNT],
    last_load: [Option<Instant>; DECK_COUNT],
    tempo_pickups: [Pickup; DECK_COUNT],
}

impl Default for Dispatcher {
    fn default() -> Self {
        Self {
            pad_modes: [PadMode::HotCue; DECK_COUNT],
            last_load: [None; DECK_COUNT],
            tempo_pickups: [Pickup::default(); DECK_COUNT],
        }
    }
}

impl Dispatcher {
    /// The pad mode a deck's pads are in, as the controller last said.
    pub fn pad_mode(&self, deck: DeckId) -> PadMode {
        self.pad_modes[deck.index()]
    }

    /// What `event`, which arrived at `now`, asks of the app, if anything.
    pub fn dispatch(
        &mut self,
        event: ControlEvent,
        now: Instant,
        surface: &Surface,
    ) -> Option<Command> {
        match event.action {
            Action::Button {
                button,
                state: edge,
            } => self.button(button, edge, event.layer, now),
            Action::Continuous { control, position } => self.continuous(control, position, surface),
            Action::Encoder {
                encoder: Encoder::Browse,
                ticks,
            } => (event.layer == Layer::Normal).then(|| Command::Browse(i32::from(ticks.get()))),
            Action::Encoder {
                encoder: Encoder::Jog { .. },
                ..
            }
            | Action::Switch { .. } => None,
        }
    }

    fn button(
        &mut self,
        button: Button,
        edge: ButtonState,
        layer: Layer,
        now: Instant,
    ) -> Option<Command> {
        let pressed = edge == ButtonState::Pressed;
        let press = if pressed { Press::Down } else { Press::Up };
        match (button, layer) {
            (Button::Play(deck), Layer::Normal) if pressed => {
                Some(Command::TogglePlay(deck_id(deck)))
            }
            (Button::Cue(deck), Layer::Normal) => Some(Command::Cue(deck_id(deck), press)),
            (Button::Cue(deck), Layer::Shift) if pressed => {
                Some(Command::JumpToStart(deck_id(deck)))
            }
            (Button::Load(deck), Layer::Normal) if pressed => Some(self.load(deck_id(deck), now)),
            (Button::BrowsePush, _) if pressed => Some(Command::SwitchBrowserPane),
            (Button::PadMode { deck, mode }, _) if pressed => {
                self.pad_modes[deck_id(deck).index()] = mode;
                None
            }
            (Button::Pad { deck, mode, pad }, layer) => {
                let deck = deck_id(deck);
                self.pad_modes[deck.index()] = mode;
                match (mode, layer) {
                    (PadMode::HotCue, Layer::Normal) => {
                        Some(Command::HotCue(deck, hot_cue_pad(pad)?, press))
                    }
                    (PadMode::HotCue, Layer::Shift) if pressed => {
                        Some(Command::DeleteHotCue(deck, hot_cue_pad(pad)?))
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// LOAD loads the selected track; a second press soon after copies the other deck instead.
    fn load(&mut self, deck: DeckId, now: Instant) -> Command {
        let previous = self.last_load[deck.index()].replace(now);
        let double = previous
            .is_some_and(|previous| now.saturating_duration_since(previous) <= DOUBLE_PRESS);
        if double {
            self.last_load[deck.index()] = None;
            Command::InstantDouble(deck)
        } else {
            Command::LoadSelected(deck)
        }
    }

    fn continuous(
        &mut self,
        control: Continuous,
        position: Position,
        surface: &Surface,
    ) -> Option<Command> {
        let fraction = position.fraction();
        let control = match control {
            Continuous::ChannelFader(deck) => Control::ChannelFader(deck_id(deck)),
            Continuous::Crossfader => Control::Crossfader,
            Continuous::MasterLevel => Control::MasterLevel,
            Continuous::Trim(deck) => Control::Trim(deck_id(deck)),
            Continuous::Tempo(deck) => {
                let deck = deck_id(deck);
                let control = Control::Tempo(deck);
                let takes_over = self.tempo_pickups[deck.index()].takes_over(
                    fraction,
                    surface.positions.fraction(control),
                    !surface.playing[deck.index()],
                );
                if !takes_over {
                    return None;
                }
                control
            }
            Continuous::Eq { .. }
            | Continuous::Filter(_)
            | Continuous::BeatFxLevelDepth
            | Continuous::MicLevel
            | Continuous::HeadphoneMix
            | Continuous::HeadphoneLevel => return None,
        };
        Some(Command::SetControl(control, fraction))
    }
}

#[cfg(test)]
mod tests {
    use dubplate_midi::{Ticks, U14};

    use super::*;

    fn button(button: Button, state: ButtonState, layer: Layer) -> ControlEvent {
        ControlEvent {
            action: Action::Button { button, state },
            layer,
        }
    }

    fn press(button: Button) -> ControlEvent {
        self::button(button, ButtonState::Pressed, Layer::Normal)
    }

    fn release(button: Button) -> ControlEvent {
        self::button(button, ButtonState::Released, Layer::Normal)
    }

    fn shift_press(button: Button) -> ControlEvent {
        self::button(button, ButtonState::Pressed, Layer::Shift)
    }

    fn moved(control: Continuous, fraction: f32) -> ControlEvent {
        let raw = (fraction * f32::from(U14::MAX.get())).round() as u16;
        ControlEvent {
            action: Action::Continuous {
                control,
                position: Position::new(U14::new(raw).unwrap()),
            },
            layer: Layer::Normal,
        }
    }

    fn pad(deck: Deck, mode: PadMode, number: u8) -> Button {
        Button::Pad {
            deck,
            mode,
            pad: PadIndex::new(number).unwrap(),
        }
    }

    fn run(
        dispatcher: &mut Dispatcher,
        surface: &Surface,
        events: &[ControlEvent],
    ) -> Vec<Command> {
        let now = Instant::now();
        events
            .iter()
            .filter_map(|event| dispatcher.dispatch(*event, now, surface))
            .collect()
    }

    #[test]
    fn transport_buttons_follow_rekordbox() {
        let mut dispatcher = Dispatcher::default();
        let surface = Surface::default();
        let commands = run(
            &mut dispatcher,
            &surface,
            &[
                press(Button::Play(Deck::A)),
                release(Button::Play(Deck::A)),
                press(Button::Cue(Deck::B)),
                release(Button::Cue(Deck::B)),
                shift_press(Button::Cue(Deck::A)),
                shift_press(Button::Play(Deck::A)),
            ],
        );
        assert_eq!(
            commands,
            [
                Command::TogglePlay(DeckId::A),
                Command::Cue(DeckId::B, Press::Down),
                Command::Cue(DeckId::B, Press::Up),
                Command::JumpToStart(DeckId::A),
            ]
        );
    }

    #[test]
    fn hot_cue_pads_call_and_set_and_shift_deletes() {
        let mut dispatcher = Dispatcher::default();
        let surface = Surface::default();
        let commands = run(
            &mut dispatcher,
            &surface,
            &[
                press(pad(Deck::A, PadMode::HotCue, 1)),
                release(pad(Deck::A, PadMode::HotCue, 1)),
                shift_press(pad(Deck::B, PadMode::HotCue, 8)),
                button(
                    pad(Deck::B, PadMode::HotCue, 8),
                    ButtonState::Released,
                    Layer::Shift,
                ),
            ],
        );
        assert_eq!(
            commands,
            [
                Command::HotCue(DeckId::A, HotCuePad::A, Press::Down),
                Command::HotCue(DeckId::A, HotCuePad::A, Press::Up),
                Command::DeleteHotCue(DeckId::B, HotCuePad::H),
            ]
        );
    }

    #[test]
    fn pad_mode_buttons_and_pads_tell_which_mode_the_pads_are_in() {
        let mut dispatcher = Dispatcher::default();
        let surface = Surface::default();
        let commands = run(
            &mut dispatcher,
            &surface,
            &[shift_press(Button::PadMode {
                deck: Deck::B,
                mode: PadMode::BeatLoop,
            })],
        );
        assert!(commands.is_empty());
        assert_eq!(dispatcher.pad_mode(DeckId::B), PadMode::BeatLoop);
        assert_eq!(dispatcher.pad_mode(DeckId::A), PadMode::HotCue);
        run(
            &mut dispatcher,
            &surface,
            &[press(pad(Deck::A, PadMode::Sampler, 2))],
        );
        assert_eq!(dispatcher.pad_mode(DeckId::A), PadMode::Sampler);
    }

    #[test]
    fn load_loads_the_selection_and_a_quick_second_press_doubles_the_other_deck() {
        let mut dispatcher = Dispatcher::default();
        let surface = Surface::default();
        let start = Instant::now();
        let load = press(Button::Load(Deck::B));
        assert_eq!(
            dispatcher.dispatch(load, start, &surface),
            Some(Command::LoadSelected(DeckId::B))
        );
        assert_eq!(
            dispatcher.dispatch(load, start + Duration::from_millis(250), &surface),
            Some(Command::InstantDouble(DeckId::B))
        );
        assert_eq!(
            dispatcher.dispatch(load, start + Duration::from_millis(400), &surface),
            Some(Command::LoadSelected(DeckId::B))
        );
        assert_eq!(
            dispatcher.dispatch(load, start + Duration::from_secs(2), &surface),
            Some(Command::LoadSelected(DeckId::B))
        );
    }

    #[test]
    fn the_browse_knob_moves_the_cursor_and_its_push_switches_pane() {
        let mut dispatcher = Dispatcher::default();
        let surface = Surface::default();
        let turn = |count, layer| ControlEvent {
            action: Action::Encoder {
                encoder: Encoder::Browse,
                ticks: Ticks::new(count),
            },
            layer,
        };
        let commands = run(
            &mut dispatcher,
            &surface,
            &[
                turn(3, Layer::Normal),
                turn(-1, Layer::Normal),
                turn(2, Layer::Shift),
                press(Button::BrowsePush),
                release(Button::BrowsePush),
            ],
        );
        assert_eq!(
            commands,
            [
                Command::Browse(3),
                Command::Browse(-1),
                Command::SwitchBrowserPane
            ]
        );
    }

    #[test]
    fn mixer_controls_set_their_position_at_fourteen_bits() {
        let mut dispatcher = Dispatcher::default();
        let surface = Surface::default();
        let commands = run(
            &mut dispatcher,
            &surface,
            &[
                moved(Continuous::ChannelFader(Deck::A), 0.25),
                moved(Continuous::Crossfader, 0.0),
                moved(Continuous::MasterLevel, 1.0),
                moved(Continuous::Trim(Deck::B), 0.5),
            ],
        );
        let expected = [
            (Control::ChannelFader(DeckId::A), 0.25),
            (Control::Crossfader, 0.0),
            (Control::MasterLevel, 1.0),
            (Control::Trim(DeckId::B), 0.5),
        ];
        assert_eq!(commands.len(), expected.len());
        for (command, (control, fraction)) in commands.iter().zip(expected) {
            let Command::SetControl(sent_control, sent_fraction) = *command else {
                panic!("{command:?} is not a control");
            };
            assert_eq!(sent_control, control);
            assert!(
                (sent_fraction - fraction).abs() < 1.0 / 16383.0,
                "{command:?}"
            );
        }
    }

    #[test]
    fn a_playing_decks_tempo_slider_picks_up_the_tempo_on_screen() {
        let mut dispatcher = Dispatcher::default();
        let mut surface = Surface::default();
        surface.playing[0] = true;
        surface.positions.tempos[0] = 1.04;
        let tempo = Continuous::Tempo(Deck::A);
        let commands = run(
            &mut dispatcher,
            &surface,
            &[moved(tempo, 0.5), moved(tempo, 0.6), moved(tempo, 0.7)],
        );
        assert!(commands.is_empty(), "{commands:?}");
        let commands = run(&mut dispatcher, &surface, &[moved(tempo, 0.8)]);
        assert!(
            matches!(
                commands[..],
                [Command::SetControl(Control::Tempo(DeckId::A), _)]
            ),
            "crossing the tempo on screen picks it up: {commands:?}"
        );
    }

    #[test]
    fn a_stopped_decks_tempo_follows_the_slider_at_once() {
        let mut dispatcher = Dispatcher::default();
        let mut surface = Surface::default();
        surface.positions.tempos[1] = 1.04;
        let commands = run(
            &mut dispatcher,
            &surface,
            &[moved(Continuous::Tempo(Deck::B), 0.1)],
        );
        assert_eq!(commands.len(), 1);
    }

    #[test]
    fn controls_without_a_feature_yet_do_nothing() {
        let mut dispatcher = Dispatcher::default();
        let surface = Surface::default();
        let commands = run(
            &mut dispatcher,
            &surface,
            &[
                moved(Continuous::MicLevel, 0.5),
                press(Button::Sync(Deck::A)),
                press(pad(Deck::A, PadMode::Sampler, 1)),
                shift_press(Button::Load(Deck::A)),
            ],
        );
        assert!(commands.is_empty(), "{commands:?}");
    }
}
