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
    /// That unit's level and depth, from 0.0 to 1.0.
    pub effect_depth: f32,
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
#[derive(Debug, Clone, Copy, Default)]
struct Pickup {
    last_hardware: Option<f32>,
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
        free || close || was_holding || crossed
    }
}

/// The controller's side of the conversation: LOAD presses for instant doubles, where the BEAT
/// FX channel lever sits, and the pick-up of the tempo sliders and the effect level knob.
#[derive(Debug)]
pub struct Dispatcher {
    last_load: [Option<Instant>; DECK_COUNT],
    tempo_pickups: [Pickup; DECK_COUNT],
    fx_lever: [bool; DECK_COUNT],
    depth_pickup: Pickup,
    depth_unit: Option<EffectUnitId>,
}

impl Default for Dispatcher {
    fn default() -> Self {
        Self {
            last_load: [None; DECK_COUNT],
            tempo_pickups: [Pickup::default(); DECK_COUNT],
            fx_lever: [false; DECK_COUNT],
            depth_pickup: Pickup::default(),
            depth_unit: None,
        }
    }
}

impl Dispatcher {
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
                self.fx_lever[deck_id(deck).index()] = on;
                let target = match self.fx_lever {
                    [true, false] => EffectTarget::Deck(DeckId::A),
                    [false, true] => EffectTarget::Deck(DeckId::B),
                    [false, false] | [true, true] => EffectTarget::Master,
                };
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
                    EffectEdit::Parameter(direction),
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
            Continuous::Eq { deck, band } => Control::Eq(deck_id(deck), eq_band(band)),
            Continuous::Filter(deck) => Control::Filter(deck_id(deck)),
            Continuous::BeatFxLevelDepth => {
                let unit = surface.focused_effect?;
                if self.depth_unit.replace(unit) != Some(unit) {
                    self.depth_pickup = Pickup::default();
                }
                let takes_over =
                    self.depth_pickup
                        .takes_over(fraction, surface.effect_depth, false);
                return takes_over.then_some(Command::Effect(unit, EffectEdit::Depth(fraction)));
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
    use dubplate_midi::{PadMode, Ticks, U14};

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
            effect_depth: 0.5,
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
                lever(Deck::A, false),
                lever(Deck::B, true),
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
                Command::Effect(unit, EffectEdit::Parameter(Direction::Up)),
                Command::Effect(unit, EffectEdit::Parameter(Direction::Down)),
                Command::FocusEffectUnit(1),
                Command::Effect(unit, EffectEdit::Target(EffectTarget::Deck(DeckId::A))),
                Command::Effect(unit, EffectEdit::Target(EffectTarget::Master)),
                Command::Effect(unit, EffectEdit::Target(EffectTarget::Deck(DeckId::B))),
                Command::Effect(unit, EffectEdit::Toggle),
                Command::ToggleSmartCfx,
                Command::NextSmartCfx,
                Command::ToggleSmartFader,
            ]
        );
    }

    #[test]
    fn the_level_knob_picks_up_the_focused_units_depth() {
        let mut dispatcher = Dispatcher::default();
        let surface = fx_surface();
        let level = |fraction| moved(Continuous::BeatFxLevelDepth, fraction);
        assert!(run(&mut dispatcher, &surface, &[level(0.9), level(0.8)]).is_empty());
        let commands = run(&mut dispatcher, &surface, &[level(0.4)]);
        assert!(
            matches!(
                commands[..],
                [Command::Effect(_, EffectEdit::Depth(depth))] if (depth - 0.4).abs() < 1e-3
            ),
            "{commands:?}"
        );
        let other_unit = Surface {
            focused_effect: Some(EffectUnitId::ALL[2]),
            effect_depth: 0.1,
            ..surface
        };
        assert!(
            run(&mut dispatcher, &other_unit, &[level(0.4)]).is_empty(),
            "a newly focused unit waits for the knob to reach its depth"
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
}
