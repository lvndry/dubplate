//! Turning what the DJ does on a controller into [`Command`]s, the same commands the keyboard
//! and the mouse send, following how rekordbox drives a Pioneer controller.

use std::time::{Duration, Instant};

use dubplate_engine::{DECK_COUNT, DeckId, EffectTarget, EffectUnitId};
use dubplate_midi::{
    Action, Button, ButtonState, Continuous, ControlEvent, Deck, Direction as Arrow, Encoder,
    JogMode, Layer, PadIndex, Position, Switch,
};

use crate::commands::{Command, Direction, EffectEdit, JogMotion, LoopAction, PadPress, Press};
use crate::controls::{Control, MixerPositions};

/// Two presses of LOAD this close together load the other deck's track, as rekordbox does.
const DOUBLE_PRESS: Duration = Duration::from_millis(400);
/// A slider or knob this close to the value on screen picks it up.
const PICKUP_DISTANCE: f32 = 0.01;

/// What the dispatcher reads of the app to decide what a control does.
#[derive(Debug, Clone, Copy, Default)]
pub struct Surface {
    /// Where the faders and knobs are on screen.
    pub positions: MixerPositions,
    /// Whether each deck is playing.
    pub playing: [bool; DECK_COUNT],
    /// The effect unit the BEAT FX section drives.
    pub focused_effect: Option<EffectUnitId>,
}

/// Where the controls of the controller's BEAT FX section physically sit, as far as the
/// controller has reported them. Both are absolute: the unit the section drives always takes
/// the lever's target and the knob's level, as rekordbox does with its single Beat FX unit.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BeatFxSection {
    /// The target the CH SELECT lever points at, once the lever has reported.
    pub lever: Option<EffectTarget>,
    /// The LEVEL/DEPTH knob, from 0.0 to 1.0, once the knob has reported.
    pub level: Option<f32>,
}

impl BeatFxSection {
    /// The commands that give `unit` the lever's target and the knob's level.
    pub fn commands_for(self, unit: EffectUnitId) -> impl Iterator<Item = Command> {
        let target = self
            .lever
            .map(|target| Command::Effect(unit, EffectEdit::Target(target)));
        let level = self
            .level
            .map(|level| Command::Effect(unit, EffectEdit::Depth(level)));
        target.into_iter().chain(level)
    }
}

/// The target of the CH SELECT lever from its two contacts: 1 alone, 2 alone, or 1&2, which
/// the controller reports as both or neither.
fn lever_target(contacts: [bool; DECK_COUNT]) -> EffectTarget {
    match contacts {
        [true, false] => EffectTarget::Deck(DeckId::A),
        [false, true] => EffectTarget::Deck(DeckId::B),
        [false, false] | [true, true] => EffectTarget::BothDecks,
    }
}

/// The engine's deck for a controller deck.
pub fn deck_id(deck: Deck) -> DeckId {
    match deck {
        Deck::A => DeckId::A,
        Deck::B => DeckId::B,
    }
}

/// Where a pad sits among a deck's pads, 0 for pad 1.
fn pad_position(pad: PadIndex) -> usize {
    usize::from(pad.number()).saturating_sub(1)
}

/// The engine's EQ band for a controller's EQ knob.
fn eq_band(band: dubplate_midi::EqBand) -> dubplate_engine::EqBand {
    match band {
        dubplate_midi::EqBand::High => dubplate_engine::EqBand::High,
        dubplate_midi::EqBand::Mid => dubplate_engine::EqBand::Mid,
        dubplate_midi::EqBand::Low => dubplate_engine::EqBand::Low,
    }
}

/// The controller deck for an engine deck.
pub fn controller_deck(deck: DeckId) -> Deck {
    match deck {
        DeckId::A => Deck::A,
        DeckId::B => Deck::B,
    }
}

/// A hardware control that only takes over the value on screen once it reaches it, so a slider
/// left elsewhere does not make the tempo jump. This is the pick-up of rekordbox's tempo slider.
/// The BEAT FX LEVEL/DEPTH knob has none: like rekordbox's, it sets the level the moment it
/// reports.
#[derive(Debug, Clone, Copy, Default)]
struct Pickup {
    last_hardware: Option<f32>,
    holding: bool,
}

impl Pickup {
    /// Whether the control at `hardware` should set a value now on screen at `shown`. A control
    /// that is `free` to jump, such as the tempo of a stopped deck, always takes over.
    fn takes_over(&mut self, hardware: f32, shown: f32, free: bool) -> bool {
        // A knob that has not reported yet, or whose value was changed elsewhere, holds back
        // until it reaches the value on screen.
        let previous = self.last_hardware.replace(hardware);
        let close = (hardware - shown).abs() <= PICKUP_DISTANCE;
        let was_holding =
            previous.is_some_and(|previous| (previous - shown).abs() <= PICKUP_DISTANCE);
        let crossed = previous.is_some_and(|previous| (previous < shown) != (hardware < shown));
        let takes_over = free || close || was_holding || crossed;
        self.holding = !takes_over;
        takes_over
    }

    /// Where the control sits while it holds back from the value on screen at `shown`, so the
    /// screen can show both, as rekordbox does until the DJ moves it through.
    fn waiting_at(&self, shown: f32) -> Option<f32> {
        self.last_hardware
            .filter(|hardware| self.holding && (hardware - shown).abs() > PICKUP_DISTANCE)
    }
}

/// The controller's side of the conversation: LOAD presses for instant doubles, the pick-up of
/// the tempo sliders, and where the BEAT FX section's lever and knob sit.
#[derive(Debug)]
pub struct Dispatcher {
    last_load: [Option<Instant>; DECK_COUNT],
    tempo_pickups: [Pickup; DECK_COUNT],
    lever_contacts: [bool; DECK_COUNT],
    beat_fx: BeatFxSection,
    master_pickup: Pickup,
}

impl Default for Dispatcher {
    fn default() -> Self {
        Self {
            last_load: [None; DECK_COUNT],
            tempo_pickups: [Pickup::default(); DECK_COUNT],
            lever_contacts: [false; DECK_COUNT],
            beat_fx: BeatFxSection::default(),
            master_pickup: Pickup::default(),
        }
    }
}

impl Dispatcher {
    /// Where the BEAT FX section's lever and knob sit, as far as the controller has said.
    pub fn beat_fx_section(&self) -> BeatFxSection {
        self.beat_fx
    }

    /// Forgets where the controller's controls sit, when it is unplugged or plugged in again.
    pub fn forget_positions(&mut self) {
        *self = Self::default();
    }

    /// Where a deck's TEMPO slider sits while it waits to reach the tempo on screen at
    /// `shown`.
    pub fn tempo_waiting(&self, deck: DeckId, shown: f32) -> Option<f32> {
        self.tempo_pickups[deck.index()].waiting_at(shown)
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
            } => self.button(button, edge, event.layer, now, surface),
            Action::Continuous { control, position } => self.continuous(control, position, surface),
            Action::Encoder {
                encoder: Encoder::Browse,
                ticks,
            } => (event.layer == Layer::Normal).then(|| Command::Browse(i32::from(ticks.get()))),
            Action::Encoder {
                encoder: Encoder::Jog { deck, mode },
                ticks,
            } => {
                let motion = match mode {
                    JogMode::Scratch => JogMotion::Scratch,
                    JogMode::Nudge => JogMotion::Nudge,
                    JogMode::Search => JogMotion::Search,
                };
                Some(Command::Jog(deck_id(deck), motion, i32::from(ticks.get())))
            }
            Action::Switch {
                switch: Switch::BeatFxChannel(deck),
                on,
            } => {
                self.lever_contacts[deck_id(deck).index()] = on;
                let target = lever_target(self.lever_contacts);
                self.beat_fx.lever = Some(target);
                Some(Command::Effect(
                    surface.focused_effect?,
                    EffectEdit::Target(target),
                ))
            }
            Action::Switch {
                switch: Switch::MonoOutput,
                ..
            } => None,
        }
    }

    fn button(
        &mut self,
        button: Button,
        edge: ButtonState,
        layer: Layer,
        now: Instant,
        surface: &Surface,
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
            (Button::Sync(deck), Layer::Normal) if pressed => {
                Some(Command::ToggleSync(deck_id(deck)))
            }
            (Button::Sync(deck), Layer::Shift) if pressed => {
                Some(Command::CycleTempoRange(deck_id(deck)))
            }
            (Button::SyncHold(deck), _) if pressed => Some(Command::MakeTempoMaster(deck_id(deck))),
            (Button::Load(deck), Layer::Normal) if pressed => Some(self.load(deck_id(deck), now)),
            (Button::BrowsePush, _) if pressed => Some(Command::SwitchBrowserPane),
            (Button::JogTouch(deck), _) => Some(Command::JogTouch(deck_id(deck), pressed)),
            (Button::HeadphoneCue(deck), Layer::Normal) if pressed => {
                Some(Command::ToggleHeadphoneCue(deck_id(deck)))
            }
            (Button::HeadphoneCue(deck), Layer::Shift) if pressed => {
                Some(Command::TapTempo(deck_id(deck)))
            }
            (Button::MasterCue, Layer::Normal) if pressed => Some(Command::ToggleMasterCue),
            (Button::BeatFxSelect, layer) if pressed => {
                let steps = if layer == Layer::Shift { -1 } else { 1 };
                Some(Command::Effect(
                    surface.focused_effect?,
                    EffectEdit::StepKind(steps),
                ))
            }
            (Button::BeatFxBeat(arrow), Layer::Normal) if pressed => {
                let direction = match arrow {
                    Arrow::Left => Direction::Down,
                    Arrow::Right => Direction::Up,
                };
                Some(Command::Effect(
                    surface.focused_effect?,
                    EffectEdit::Beat(direction),
                ))
            }
            (Button::BeatFxBeat(arrow), Layer::Shift) if pressed => {
                Some(Command::FocusEffectUnit(match arrow {
                    Arrow::Left => -1,
                    Arrow::Right => 1,
                }))
            }
            (Button::BeatFxOn, Layer::Normal) if pressed => {
                Some(Command::Effect(surface.focused_effect?, EffectEdit::Toggle))
            }
            (Button::SmartCfx, Layer::Normal) if pressed => Some(Command::ToggleSmartCfx),
            (Button::SmartCfx, Layer::Shift) if pressed => Some(Command::NextSmartCfx),
            (Button::SmartFader, Layer::Normal) if pressed => Some(Command::ToggleSmartFader),
            (Button::PadMode { deck, mode }, _) if pressed => {
                Some(Command::SetPadMode(deck_id(deck), mode))
            }
            (Button::Pad { deck, mode, pad }, layer) => Some(Command::Pad(PadPress {
                deck: deck_id(deck),
                pad: pad_position(pad),
                mode: Some(mode),
                press,
                shift: layer == Layer::Shift,
            })),
            (Button::LoopIn(deck), Layer::Normal) if pressed => {
                Some(Command::Loop(deck_id(deck), LoopAction::In))
            }
            (Button::LoopOut(deck), Layer::Normal) if pressed => {
                Some(Command::Loop(deck_id(deck), LoopAction::Out))
            }
            (Button::FourBeatExit(deck), Layer::Normal) if pressed => {
                Some(Command::Loop(deck_id(deck), LoopAction::FourBeatOrExit))
            }
            (Button::CueLoopCall { deck, direction }, Layer::Normal) if pressed => {
                let action = match direction {
                    Arrow::Left => LoopAction::Halve,
                    Arrow::Right => LoopAction::Double,
                };
                Some(Command::Loop(deck_id(deck), action))
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
            Continuous::MasterLevel => {
                // Turning the master down always works; a knob that reports a louder level
                // than the screen's, as when the controller connects, waits to reach it.
                let shown = surface.positions.master_level;
                if !self
                    .master_pickup
                    .takes_over(fraction, shown, fraction <= shown)
                {
                    return None;
                }
                Control::MasterLevel
            }
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
            Continuous::Eq { deck, band } => Control::Eq(deck_id(deck), eq_band(band)),
            Continuous::Filter(deck) => Control::Filter(deck_id(deck)),
            Continuous::BeatFxLevelDepth => {
                self.beat_fx.level = Some(fraction);
                return Some(Command::Effect(
                    surface.focused_effect?,
                    EffectEdit::Depth(fraction),
                ));
            }
            Continuous::HeadphoneMix => Control::HeadphoneMix,
            Continuous::HeadphoneLevel => Control::HeadphoneLevel,
            Continuous::MicLevel => return None,
        };
        Some(Command::SetControl(control, fraction))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use dubplate_midi::{Decoder, Mapping, PadMode, Ticks, U14};

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
    fn pads_carry_their_mode_and_shift_and_mode_buttons_choose_the_mode() {
        let mut dispatcher = Dispatcher::default();
        let surface = Surface::default();
        let commands = run(
            &mut dispatcher,
            &surface,
            &[
                press(pad(Deck::A, PadMode::HotCue, 1)),
                release(pad(Deck::A, PadMode::HotCue, 1)),
                shift_press(pad(Deck::B, PadMode::BeatJump, 8)),
                shift_press(Button::PadMode {
                    deck: Deck::B,
                    mode: PadMode::BeatLoop,
                }),
            ],
        );
        let pad_press = |deck, pad, mode, press, shift| {
            Command::Pad(PadPress {
                deck,
                pad,
                mode: Some(mode),
                press,
                shift,
            })
        };
        assert_eq!(
            commands,
            [
                pad_press(DeckId::A, 0, PadMode::HotCue, Press::Down, false),
                pad_press(DeckId::A, 0, PadMode::HotCue, Press::Up, false),
                pad_press(DeckId::B, 7, PadMode::BeatJump, Press::Down, true),
                Command::SetPadMode(DeckId::B, PadMode::BeatLoop),
            ]
        );
    }

    #[test]
    fn the_loop_section_follows_rekordbox() {
        let mut dispatcher = Dispatcher::default();
        let surface = Surface::default();
        let commands = run(
            &mut dispatcher,
            &surface,
            &[
                press(Button::LoopIn(Deck::A)),
                press(Button::LoopOut(Deck::A)),
                press(Button::FourBeatExit(Deck::B)),
                press(Button::CueLoopCall {
                    deck: Deck::B,
                    direction: Arrow::Left,
                }),
                press(Button::CueLoopCall {
                    deck: Deck::B,
                    direction: Arrow::Right,
                }),
                shift_press(Button::LoopIn(Deck::A)),
            ],
        );
        assert_eq!(
            commands,
            [
                Command::Loop(DeckId::A, LoopAction::In),
                Command::Loop(DeckId::A, LoopAction::Out),
                Command::Loop(DeckId::B, LoopAction::FourBeatOrExit),
                Command::Loop(DeckId::B, LoopAction::Halve),
                Command::Loop(DeckId::B, LoopAction::Double),
            ]
        );
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
    fn a_master_knob_louder_than_the_screen_waits_but_turning_down_always_works() {
        let mut dispatcher = Dispatcher::default();
        let mut surface = Surface::default();
        surface.positions.master_level = 0.4;
        let master = |fraction| moved(Continuous::MasterLevel, fraction);
        assert!(
            run(&mut dispatcher, &surface, &[master(1.0), master(0.9)]).is_empty(),
            "a knob left at full on connect does not blast the room"
        );
        assert_eq!(
            run(&mut dispatcher, &surface, &[master(0.35)]).len(),
            1,
            "it takes over once it comes down through the level"
        );
        let mut quiet = Dispatcher::default();
        assert_eq!(run(&mut quiet, &surface, &[master(0.1)]).len(), 1);
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
        surface.positions.tempo_sliders[0] = 0.75;
        let tempo = Continuous::Tempo(Deck::A);
        let commands = run(
            &mut dispatcher,
            &surface,
            &[moved(tempo, 0.5), moved(tempo, 0.6), moved(tempo, 0.7)],
        );
        assert!(commands.is_empty(), "{commands:?}");
        let waiting = dispatcher.tempo_waiting(DeckId::A, 0.75);
        assert!(
            waiting.is_some_and(|position| (position - 0.7).abs() < 1e-2),
            "the screen can show where the slider waits: {waiting:?}"
        );
        assert_eq!(dispatcher.tempo_waiting(DeckId::B, 0.75), None);
        let commands = run(&mut dispatcher, &surface, &[moved(tempo, 0.8)]);
        assert!(
            matches!(
                commands[..],
                [Command::SetControl(Control::Tempo(DeckId::A), _)]
            ),
            "crossing the tempo on screen picks it up: {commands:?}"
        );
        assert_eq!(dispatcher.tempo_waiting(DeckId::A, 0.8), None);
    }

    #[test]
    fn a_stopped_decks_tempo_follows_the_slider_at_once() {
        let mut dispatcher = Dispatcher::default();
        let mut surface = Surface::default();
        surface.positions.tempo_sliders[1] = 0.75;
        let commands = run(
            &mut dispatcher,
            &surface,
            &[moved(Continuous::Tempo(Deck::B), 0.1)],
        );
        assert_eq!(commands.len(), 1);
    }

    fn fx_surface() -> Surface {
        Surface {
            focused_effect: Some(EffectUnitId::ALL[1]),
            ..Surface::default()
        }
    }

    #[test]
    fn eq_and_filter_knobs_turn_the_channel() {
        let mut dispatcher = Dispatcher::default();
        let commands = run(
            &mut dispatcher,
            &Surface::default(),
            &[
                moved(
                    Continuous::Eq {
                        deck: Deck::A,
                        band: dubplate_midi::EqBand::Low,
                    },
                    0.0,
                ),
                moved(Continuous::Filter(Deck::B), 1.0),
            ],
        );
        assert_eq!(
            commands,
            [
                Command::SetControl(Control::Eq(DeckId::A, dubplate_engine::EqBand::Low), 0.0),
                Command::SetControl(Control::Filter(DeckId::B), 1.0),
            ]
        );
    }

    #[test]
    fn jogs_scratch_nudge_and_search_and_report_the_hand() {
        let mut dispatcher = Dispatcher::default();
        let turn = |mode, count, layer| ControlEvent {
            action: Action::Encoder {
                encoder: Encoder::Jog {
                    deck: Deck::B,
                    mode,
                },
                ticks: Ticks::new(count),
            },
            layer,
        };
        let commands = run(
            &mut dispatcher,
            &Surface::default(),
            &[
                press(Button::JogTouch(Deck::B)),
                turn(JogMode::Scratch, -3, Layer::Normal),
                release(Button::JogTouch(Deck::B)),
                turn(JogMode::Nudge, 2, Layer::Normal),
                turn(JogMode::Search, 5, Layer::Shift),
            ],
        );
        assert_eq!(
            commands,
            [
                Command::JogTouch(DeckId::B, true),
                Command::Jog(DeckId::B, JogMotion::Scratch, -3),
                Command::JogTouch(DeckId::B, false),
                Command::Jog(DeckId::B, JogMotion::Nudge, 2),
                Command::Jog(DeckId::B, JogMotion::Search, 5),
            ]
        );
    }

    #[test]
    fn the_beat_fx_section_drives_the_focused_unit() {
        let mut dispatcher = Dispatcher::default();
        let surface = fx_surface();
        let unit = EffectUnitId::ALL[1];
        let lever = |deck, on| ControlEvent {
            action: Action::Switch {
                switch: Switch::BeatFxChannel(deck),
                on,
            },
            layer: Layer::Normal,
        };
        let commands = run(
            &mut dispatcher,
            &surface,
            &[
                press(Button::BeatFxSelect),
                shift_press(Button::BeatFxSelect),
                press(Button::BeatFxBeat(Arrow::Right)),
                press(Button::BeatFxBeat(Arrow::Left)),
                shift_press(Button::BeatFxBeat(Arrow::Right)),
                lever(Deck::A, true),
                lever(Deck::B, true),
                lever(Deck::A, false),
                press(Button::BeatFxOn),
                release(Button::BeatFxOn),
                press(Button::SmartCfx),
                release(Button::SmartCfx),
                shift_press(Button::SmartCfx),
                press(Button::SmartFader),
                shift_press(Button::SmartFader),
            ],
        );
        assert_eq!(
            commands,
            [
                Command::Effect(unit, EffectEdit::StepKind(1)),
                Command::Effect(unit, EffectEdit::StepKind(-1)),
                Command::Effect(unit, EffectEdit::Beat(Direction::Up)),
                Command::Effect(unit, EffectEdit::Beat(Direction::Down)),
                Command::FocusEffectUnit(1),
                Command::Effect(unit, EffectEdit::Target(EffectTarget::Deck(DeckId::A))),
                Command::Effect(unit, EffectEdit::Target(EffectTarget::BothDecks)),
                Command::Effect(unit, EffectEdit::Target(EffectTarget::Deck(DeckId::B))),
                Command::Effect(unit, EffectEdit::Toggle),
                Command::ToggleSmartCfx,
                Command::NextSmartCfx,
                Command::ToggleSmartFader,
            ]
        );
    }

    #[test]
    fn the_level_knob_sets_the_focused_units_depth_from_its_first_report() {
        let mut dispatcher = Dispatcher::default();
        let surface = fx_surface();
        let unit = EffectUnitId::ALL[1];
        assert_eq!(dispatcher.beat_fx_section().level, None);
        let level = |fraction| moved(Continuous::BeatFxLevelDepth, fraction);
        let commands = run(&mut dispatcher, &surface, &[level(0.9), level(0.4)]);
        let depths: Vec<f32> = commands
            .iter()
            .map(|command| match command {
                Command::Effect(target_unit, EffectEdit::Depth(depth)) if *target_unit == unit => {
                    *depth
                }
                other => panic!("{other:?} is not the unit's depth"),
            })
            .collect();
        assert_eq!(depths.len(), 2);
        assert!((depths[0] - 0.9).abs() < 1e-3 && (depths[1] - 0.4).abs() < 1e-3);
        let section = dispatcher.beat_fx_section();
        assert!(
            section
                .level
                .is_some_and(|level| (level - 0.4).abs() < 1e-3)
        );
        let other_unit = EffectUnitId::ALL[2];
        assert!(
            section
                .commands_for(other_unit)
                .any(|command| matches!(command, Command::Effect(unit, EffectEdit::Depth(_)) if unit == other_unit)),
            "a newly focused unit takes the knob's level"
        );
    }

    #[test]
    fn beat_sync_toggles_holds_for_master_and_with_shift_steps_the_tempo_range() {
        let mut dispatcher = Dispatcher::default();
        let commands = run(
            &mut dispatcher,
            &Surface::default(),
            &[
                press(Button::Sync(Deck::A)),
                release(Button::Sync(Deck::A)),
                press(Button::SyncHold(Deck::B)),
                shift_press(Button::Sync(Deck::B)),
            ],
        );
        assert_eq!(
            commands,
            [
                Command::ToggleSync(DeckId::A),
                Command::MakeTempoMaster(DeckId::B),
                Command::CycleTempoRange(DeckId::B),
            ]
        );
    }

    #[test]
    fn the_headphone_section_cues_and_sets_mix_and_level_and_shift_cue_taps_the_tempo() {
        let mut dispatcher = Dispatcher::default();
        let commands = run(
            &mut dispatcher,
            &Surface::default(),
            &[
                press(Button::HeadphoneCue(Deck::B)),
                release(Button::HeadphoneCue(Deck::B)),
                press(Button::MasterCue),
                moved(Continuous::HeadphoneMix, 1.0),
                moved(Continuous::HeadphoneLevel, 0.0),
                shift_press(Button::HeadphoneCue(Deck::A)),
            ],
        );
        assert_eq!(
            commands,
            [
                Command::ToggleHeadphoneCue(DeckId::B),
                Command::ToggleMasterCue,
                Command::SetControl(Control::HeadphoneMix, 1.0),
                Command::SetControl(Control::HeadphoneLevel, 0.0),
                Command::TapTempo(DeckId::A),
            ]
        );
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
                shift_press(Button::Load(Deck::A)),
            ],
        );
        assert!(commands.is_empty(), "{commands:?}");
    }

    /// What a DDJ-FLX4 sent back on its own when asked for its positions: the left tempo and
    /// trims, EQs, CFX, faders and crossfader, the LEVEL/DEPTH knob, the CH SELECT lever on 2.
    const FLX4_POSITION_REPORT: [[u8; 3]; 42] = [
        [0xB0, 0x04, 0x47],
        [0xB0, 0x24, 0x58],
        [0xB1, 0x04, 0x3B],
        [0xB1, 0x24, 0x6F],
        [0xB0, 0x07, 0x40],
        [0xB0, 0x27, 0x00],
        [0xB1, 0x07, 0x40],
        [0xB1, 0x27, 0x00],
        [0xB0, 0x0B, 0x40],
        [0xB0, 0x2B, 0x00],
        [0xB1, 0x0B, 0x40],
        [0xB1, 0x2B, 0x00],
        [0xB0, 0x0F, 0x40],
        [0xB0, 0x2F, 0x00],
        [0xB1, 0x0F, 0x40],
        [0xB1, 0x2F, 0x00],
        [0xB0, 0x13, 0x7F],
        [0xB0, 0x33, 0x7F],
        [0xB1, 0x13, 0x00],
        [0xB1, 0x33, 0x00],
        [0xB0, 0x00, 0x40],
        [0xB0, 0x20, 0x00],
        [0xB1, 0x00, 0x55],
        [0xB1, 0x20, 0x45],
        [0xB6, 0x17, 0x40],
        [0xB6, 0x37, 0x00],
        [0xB6, 0x18, 0x40],
        [0xB6, 0x38, 0x00],
        [0xB6, 0x1F, 0x39],
        [0xB6, 0x3F, 0x6B],
        [0xB4, 0x02, 0x00],
        [0xB4, 0x22, 0x00],
        [0xB5, 0x02, 0x00],
        [0xB5, 0x22, 0x00],
        [0x90, 0x17, 0x7F],
        [0x91, 0x17, 0x7F],
        [0x9F, 0x47, 0x7F],
        [0x9F, 0x49, 0x7F],
        [0x94, 0x10, 0x00],
        [0x94, 0x11, 0x00],
        [0x95, 0x10, 0x00],
        [0x95, 0x11, 0x7F],
    ];

    fn report_fraction(most_significant: u8, least_significant: u8) -> f32 {
        f32::from(u16::from(most_significant) << 7 | u16::from(least_significant))
            / f32::from(U14::MAX.get())
    }

    fn replay_report(
        dispatcher: &mut Dispatcher,
        surface: &Surface,
        report: &[[u8; 3]],
    ) -> Vec<Command> {
        let mapping = Mapping::built_in_for_port("DDJ-FLX4").unwrap().unwrap();
        let mut decoder = Decoder::new(Arc::new(mapping));
        let now = Instant::now();
        report
            .iter()
            .filter_map(|bytes| decoder.decode(bytes))
            .filter_map(|event| dispatcher.dispatch(event, now, surface))
            .collect()
    }

    fn set_to(commands: &[Command], control: Control) -> f32 {
        commands
            .iter()
            .rev()
            .find_map(|command| match command {
                Command::SetControl(set, fraction) if *set == control => Some(*fraction),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{control:?} was not set"))
    }

    #[test]
    fn the_controllers_position_report_sets_every_control_it_covers() {
        let mut dispatcher = Dispatcher::default();
        let surface = fx_surface();
        let unit = EffectUnitId::ALL[1];
        let commands = replay_report(&mut dispatcher, &surface, &FLX4_POSITION_REPORT);
        let expected = [
            (Control::Trim(DeckId::A), 0x47, 0x58),
            (Control::Trim(DeckId::B), 0x3B, 0x6F),
            (
                Control::Eq(DeckId::A, dubplate_engine::EqBand::High),
                0x40,
                0x00,
            ),
            (
                Control::Eq(DeckId::A, dubplate_engine::EqBand::Mid),
                0x40,
                0x00,
            ),
            (
                Control::Eq(DeckId::A, dubplate_engine::EqBand::Low),
                0x40,
                0x00,
            ),
            (
                Control::Eq(DeckId::B, dubplate_engine::EqBand::High),
                0x40,
                0x00,
            ),
            (
                Control::Eq(DeckId::B, dubplate_engine::EqBand::Mid),
                0x40,
                0x00,
            ),
            (
                Control::Eq(DeckId::B, dubplate_engine::EqBand::Low),
                0x40,
                0x00,
            ),
            (Control::ChannelFader(DeckId::A), 0x7F, 0x7F),
            (Control::ChannelFader(DeckId::B), 0x00, 0x00),
            (Control::Tempo(DeckId::A), 0x40, 0x00),
            (Control::Tempo(DeckId::B), 0x55, 0x45),
            (Control::Filter(DeckId::A), 0x40, 0x00),
            (Control::Filter(DeckId::B), 0x40, 0x00),
        ];
        for (control, most_significant, least_significant) in expected {
            let fraction = set_to(&commands, control);
            let reported = report_fraction(most_significant, least_significant);
            assert!(
                (fraction - reported).abs() < 1e-4,
                "{control:?} is {fraction}, the controller reported {reported}"
            );
        }
        let crossfader = set_to(&commands, Control::Crossfader);
        assert!((crossfader - report_fraction(0x39, 0x6B)).abs() < 1e-4);
        let section = dispatcher.beat_fx_section();
        assert_eq!(section.lever, Some(EffectTarget::Deck(DeckId::B)));
        assert!(section.level.is_some_and(|level| level.abs() < 1e-4));
        assert!(commands.contains(&Command::Effect(
            unit,
            EffectEdit::Target(EffectTarget::Deck(DeckId::B))
        )));
        assert!(commands.contains(&Command::Effect(unit, EffectEdit::Depth(0.0))));
    }

    #[test]
    fn a_reported_tempo_does_not_jump_a_playing_deck() {
        let mut dispatcher = Dispatcher::default();
        let tempo_report = [[0xB0, 0x00, 0x55], [0xB0, 0x20, 0x45]];
        let mut surface = Surface::default();
        surface.positions.tempo_sliders = [0.5, 0.5];
        surface.playing = [true, false];
        let commands = replay_report(&mut dispatcher, &surface, &tempo_report);
        assert!(commands.is_empty());
        let reported = report_fraction(0x55, 0x45);
        assert_eq!(dispatcher.tempo_waiting(DeckId::A, 0.5), Some(reported));
        surface.playing = [false, false];
        let commands = replay_report(&mut dispatcher, &surface, &tempo_report);
        assert_eq!(
            commands,
            [Command::SetControl(Control::Tempo(DeckId::A), reported)]
        );
    }
}

#[cfg(test)]
mod lever_routing {
    use std::sync::Arc;
    use std::time::Instant;

    use dubplate_engine::{
        BeatGrid, Controller, DeckId, EffectKind, EffectTarget, EffectUnitId, Renderer, Track,
        offline_engine,
    };
    use dubplate_midi::{Decoder, Mapping};

    use super::{Dispatcher, Surface};
    use crate::commands::{Command, EffectEdit};

    const SAMPLE_RATE: u32 = 48_000;
    const BLOCK: usize = 256;
    const UNIT: EffectUnitId = EffectUnitId::ALL[0];
    /// The CH SELECT lever's two contacts, CH1 on 94 10 and CH2 on 95 11, as the DDJ-FLX4 sends
    /// them at each position.
    const LEVER_AT_1: [[u8; 3]; 2] = [[0x94, 0x10, 0x7F], [0x95, 0x11, 0x00]];
    const LEVER_AT_2: [[u8; 3]; 2] = [[0x94, 0x10, 0x00], [0x95, 0x11, 0x7F]];
    const LEVER_AT_BOTH: [[u8; 3]; 2] = [[0x94, 0x10, 0x7F], [0x95, 0x11, 0x7F]];

    fn decoder() -> Decoder {
        Decoder::new(Arc::new(
            Mapping::built_in_for_port("DDJ-FLX4")
                .expect("the built-in mappings parse")
                .expect("the DDJ-FLX4 has a mapping"),
        ))
    }

    /// Feeds raw controller messages through the FLX4 mapping and the dispatcher into the
    /// engine, as the app does.
    fn replay(dispatcher: &mut Dispatcher, controller: &mut Controller, messages: &[[u8; 3]]) {
        let mut decoder = decoder();
        let surface = Surface {
            focused_effect: Some(UNIT),
            ..Surface::default()
        };
        for message in messages {
            let event = decoder
                .decode(message)
                .expect("the mapping knows the lever");
            if let Some(Command::Effect(unit, EffectEdit::Target(target))) =
                dispatcher.dispatch(event, Instant::now(), &surface)
            {
                controller
                    .set_effect_target(unit, target)
                    .expect("set the target");
            }
        }
    }

    fn chord_track() -> Track {
        let samples: Vec<f32> = (0..SAMPLE_RATE as usize * 4)
            .flat_map(|frame| {
                let seconds = frame as f64 / f64::from(SAMPLE_RATE);
                let value = [220.0, 277.18, 329.63, 1_760.0]
                    .iter()
                    .map(|hertz| (std::f64::consts::TAU * hertz * seconds).sin() * 0.15)
                    .sum::<f64>() as f32;
                [value, value]
            })
            .collect();
        Track::from_interleaved_stereo(&samples, SAMPLE_RATE).expect("valid track")
    }

    /// Deck B playing alone, a crusher on the unit at full depth, switched on.
    fn deck_b_alone() -> (Controller, Renderer) {
        let (mut controller, renderer) = offline_engine(SAMPLE_RATE);
        controller
            .load_track(DeckId::B, chord_track())
            .expect("load");
        controller
            .set_beat_grid(DeckId::B, Some(BeatGrid::new(120.0, 0.0).expect("grid")))
            .expect("grid");
        controller.play(DeckId::B).expect("play");
        controller
            .set_effect_kind(UNIT, EffectKind::Crusher)
            .expect("kind");
        controller.set_effect_depth(UNIT, 1.0).expect("depth");
        (controller, renderer)
    }

    fn render(renderer: &mut Renderer, frames: usize) -> Vec<f32> {
        let mut output = vec![0.0; frames * 2];
        for block in output.chunks_mut(BLOCK * 2) {
            renderer.render(block, 2);
        }
        output
    }

    /// Whether deck B sounds processed after `messages`, against the same deck without them.
    fn deck_b_is_processed_after(saved: EffectTarget, messages: &[[u8; 3]]) -> bool {
        let (_reference_controller, mut reference) = deck_b_alone();
        let (mut controller, mut renderer) = deck_b_alone();
        controller.set_effect_target(UNIT, saved).expect("target");
        let mut dispatcher = Dispatcher::default();
        replay(&mut dispatcher, &mut controller, messages);
        controller.set_effect_enabled(UNIT, true).expect("on");
        let frames = SAMPLE_RATE as usize;
        let dry = render(&mut reference, frames);
        let wet = render(&mut renderer, frames);
        let changed = dry
            .iter()
            .zip(&wet)
            .filter(|(dry, wet)| (*dry - *wet).abs() > 1e-3)
            .count();
        changed > frames / 4
    }

    #[test]
    fn deck_b_alone_is_processed_where_the_lever_points() {
        for saved in EffectTarget::ALL {
            assert!(
                !deck_b_is_processed_after(saved, &LEVER_AT_1),
                "lever on 1 processed deck B (saved target {saved:?})"
            );
            assert!(
                deck_b_is_processed_after(saved, &LEVER_AT_2),
                "lever on 2 left deck B dry (saved target {saved:?})"
            );
            assert!(
                deck_b_is_processed_after(saved, &LEVER_AT_BOTH),
                "lever on 1&2 left deck B dry (saved target {saved:?})"
            );
        }
    }

    #[test]
    fn a_launch_then_the_connect_time_report_follows_the_lever_not_the_saved_target() {
        let (mut controller, _renderer) = offline_engine(SAMPLE_RATE);
        controller
            .set_effect_target(UNIT, EffectTarget::Deck(DeckId::A))
            .expect("restore the saved target");
        let mut dispatcher = Dispatcher::default();
        assert_eq!(dispatcher.beat_fx_section().lever, None);
        replay(&mut dispatcher, &mut controller, &LEVER_AT_BOTH);
        assert_eq!(controller.effect_unit(UNIT).target, EffectTarget::BothDecks);
        assert_eq!(
            dispatcher.beat_fx_section().lever,
            Some(EffectTarget::BothDecks)
        );
    }
}
