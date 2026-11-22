//! Which controller buttons are down, so their twins on screen show the press: a pad hit on
//! the FLX4 lights its pad on screen as a click would. A quick tap stays lit for a moment
//! after it is let go, so a fast hit is never missed.

use std::time::{Duration, Instant};

use dubplate_engine::{DECK_COUNT, DeckId};
use dubplate_midi::{Button, ButtonState, Deck};

use super::dispatch::deck_id;
use crate::pads::PAD_COUNT;

/// How long a press stays lit on screen after the button is let go.
pub const PRESS_FLASH: Duration = Duration::from_millis(120);

/// A control on screen that has a twin on the controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeldControl {
    /// A performance pad, 0 for pad 1, in whichever pad mode.
    Pad(DeckId, usize),
    /// CUE.
    Cue(DeckId),
    /// PLAY/PAUSE.
    Play(DeckId),
    /// BEAT SYNC.
    Sync(DeckId),
    /// LOOP IN.
    LoopIn(DeckId),
    /// LOOP OUT.
    LoopOut(DeckId),
    /// 4 BEAT/EXIT.
    FourBeat(DeckId),
    /// The channel's headphone CUE.
    HeadphoneCue(DeckId),
    /// BEAT FX ON/OFF.
    BeatFxOn,
    /// SMART CFX.
    SmartCfx,
    /// SMART FADER.
    SmartFader,
}

/// One control's press: whether it is down, and when it was last let go.
#[derive(Debug, Clone, Copy, Default)]
struct Press {
    down: bool,
    released: Option<Instant>,
}

impl Press {
    fn note(&mut self, state: ButtonState, now: Instant) {
        match state {
            ButtonState::Pressed => self.down = true,
            ButtonState::Released => {
                self.down = false;
                self.released = Some(now);
            }
        }
    }

    fn shows(&self, now: Instant) -> bool {
        self.down
            || self
                .released
                .is_some_and(|released| now.saturating_duration_since(released) < PRESS_FLASH)
    }
}

/// The presses of one deck's controls.
#[derive(Debug, Clone, Copy, Default)]
struct DeckPresses {
    pads: [Press; PAD_COUNT],
    cue: Press,
    play: Press,
    sync: Press,
    loop_in: Press,
    loop_out: Press,
    four_beat: Press,
    headphone_cue: Press,
}

/// The presses of a controller deck's controls.
fn deck_presses(decks: &mut [DeckPresses; DECK_COUNT], controller: Deck) -> &mut DeckPresses {
    &mut decks[deck_id(controller).index()]
}

/// The presses of every control with a twin on screen, set from the controller's note on and
/// note off as they arrive.
#[derive(Debug, Clone, Copy, Default)]
pub struct HeldControls {
    decks: [DeckPresses; DECK_COUNT],
    beat_fx_on: Press,
    smart_cfx: Press,
    smart_fader: Press,
}

impl HeldControls {
    /// Notes that `button` went down or came up at `now`. Buttons without a twin on screen
    /// are left alone.
    pub fn note(&mut self, button: Button, state: ButtonState, now: Instant) {
        if let Some(press) = self.press_of(button) {
            press.note(state, now);
        }
    }

    /// Whether `control` shows pressed at `now`: down, or let go less than
    /// [`PRESS_FLASH`] ago.
    pub fn shows(&self, control: HeldControl, now: Instant) -> bool {
        self.press(control).is_some_and(|press| press.shows(now))
    }

    /// Whether any control shows pressed at `now`, so the window keeps drawing until the last
    /// flash is over.
    pub fn any_shown(&self, now: Instant) -> bool {
        let deck_presses = self.decks.iter().flat_map(|deck| {
            deck.pads.iter().chain([
                &deck.cue,
                &deck.play,
                &deck.sync,
                &deck.loop_in,
                &deck.loop_out,
                &deck.four_beat,
                &deck.headphone_cue,
            ])
        });
        deck_presses
            .chain([&self.beat_fx_on, &self.smart_cfx, &self.smart_fader])
            .any(|press| press.shows(now))
    }

    fn press(&self, control: HeldControl) -> Option<&Press> {
        let deck = |deck: DeckId| &self.decks[deck.index()];
        Some(match control {
            HeldControl::Pad(id, pad) => deck(id).pads.get(pad)?,
            HeldControl::Cue(id) => &deck(id).cue,
            HeldControl::Play(id) => &deck(id).play,
            HeldControl::Sync(id) => &deck(id).sync,
            HeldControl::LoopIn(id) => &deck(id).loop_in,
            HeldControl::LoopOut(id) => &deck(id).loop_out,
            HeldControl::FourBeat(id) => &deck(id).four_beat,
            HeldControl::HeadphoneCue(id) => &deck(id).headphone_cue,
            HeldControl::BeatFxOn => &self.beat_fx_on,
            HeldControl::SmartCfx => &self.smart_cfx,
            HeldControl::SmartFader => &self.smart_fader,
        })
    }

    fn press_of(&mut self, button: Button) -> Option<&mut Press> {
        let deck = deck_presses;
        Some(match button {
            Button::Pad {
                deck: controller,
                pad,
                ..
            } => {
                let position = usize::from(pad.number()).checked_sub(1)?;
                deck(&mut self.decks, controller).pads.get_mut(position)?
            }
            Button::Cue(controller) => &mut deck(&mut self.decks, controller).cue,
            Button::Play(controller) => &mut deck(&mut self.decks, controller).play,
            Button::Sync(controller) => &mut deck(&mut self.decks, controller).sync,
            Button::LoopIn(controller) => &mut deck(&mut self.decks, controller).loop_in,
            Button::LoopOut(controller) => &mut deck(&mut self.decks, controller).loop_out,
            Button::FourBeatExit(controller) => &mut deck(&mut self.decks, controller).four_beat,
            Button::HeadphoneCue(controller) => {
                &mut deck(&mut self.decks, controller).headphone_cue
            }
            Button::BeatFxOn => &mut self.beat_fx_on,
            Button::SmartCfx => &mut self.smart_cfx,
            Button::SmartFader => &mut self.smart_fader,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use dubplate_midi::{Action, Decoder, Mapping};

    use super::*;
    use crate::hardware::{Dispatcher, Surface};

    /// Plays raw FLX4 messages through the FLX4 mapping and the dispatcher, as they arrive
    /// from the controller.
    fn replay(dispatcher: &mut Dispatcher, messages: &[[u8; 3]], now: Instant) {
        let mapping = Arc::new(Mapping::built_in_for_port("DDJ-FLX4").unwrap().unwrap());
        let mut decoder = Decoder::new(mapping);
        for message in messages {
            if let Some(event) = decoder.decode(message)
                && matches!(event.action, Action::Button { .. })
            {
                dispatcher.dispatch(event, now, &Surface::default());
            }
        }
    }

    #[test]
    fn a_held_pad_shows_pressed_and_a_tap_flashes_briefly() {
        let mut dispatcher = Dispatcher::default();
        let start = Instant::now();
        let pad_three = HeldControl::Pad(DeckId::A, 2);
        replay(&mut dispatcher, &[[0x97, 0x02, 0x7F]], start);
        assert!(
            dispatcher
                .held
                .shows(pad_three, start + Duration::from_secs(2))
        );
        replay(&mut dispatcher, &[[0x97, 0x02, 0x00]], start);
        assert!(dispatcher.held.shows(pad_three, start + PRESS_FLASH / 2));
        assert!(!dispatcher.held.shows(pad_three, start + PRESS_FLASH * 2));
        assert!(!dispatcher.held.any_shown(start + PRESS_FLASH * 2));
    }

    #[test]
    fn every_pad_mode_and_the_shift_layer_show_the_press() {
        let mut dispatcher = Dispatcher::default();
        let now = Instant::now();
        let beat_jump_pad_one_deck_b = [0x9A, 0x20, 0x7F];
        let shift_hot_cue_pad_eight_deck_a = [0x98, 0x07, 0x7F];
        replay(
            &mut dispatcher,
            &[beat_jump_pad_one_deck_b, shift_hot_cue_pad_eight_deck_a],
            now,
        );
        assert!(dispatcher.held.shows(HeldControl::Pad(DeckId::B, 0), now));
        assert!(dispatcher.held.shows(HeldControl::Pad(DeckId::A, 7), now));
        assert!(!dispatcher.held.shows(HeldControl::Pad(DeckId::A, 0), now));
    }

    #[test]
    fn a_beat_jump_pad_on_the_flx4_shows_pressed_then_lets_go() {
        let mut dispatcher = Dispatcher::default();
        let now = Instant::now();
        let beat_jump_pad_four_deck_a = 0x23;
        replay(
            &mut dispatcher,
            &[[0x97, beat_jump_pad_four_deck_a, 0x7F]],
            now,
        );
        assert!(dispatcher.held.shows(HeldControl::Pad(DeckId::A, 3), now));
        replay(
            &mut dispatcher,
            &[[0x97, beat_jump_pad_four_deck_a, 0x00]],
            now,
        );
        assert!(
            !dispatcher
                .held
                .shows(HeldControl::Pad(DeckId::A, 3), now + PRESS_FLASH)
        );
    }

    #[test]
    fn deck_buttons_with_a_twin_on_screen_show_the_press() {
        let mut dispatcher = Dispatcher::default();
        let now = Instant::now();
        replay(
            &mut dispatcher,
            &[[0x90, 0x0B, 0x7F], [0x91, 0x0C, 0x7F], [0x90, 0x10, 0x7F]],
            now,
        );
        assert!(dispatcher.held.shows(HeldControl::Play(DeckId::A), now));
        assert!(dispatcher.held.shows(HeldControl::Cue(DeckId::B), now));
        assert!(dispatcher.held.shows(HeldControl::LoopIn(DeckId::A), now));
        replay(&mut dispatcher, &[[0x90, 0x0B, 0x00]], now);
        assert!(
            !dispatcher
                .held
                .shows(HeldControl::Play(DeckId::A), now + PRESS_FLASH * 2)
        );
    }
}
