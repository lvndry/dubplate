//! What the controller's lights should show, and sending only the lights that changed.

use dubplate_engine::DECK_COUNT;
use dubplate_midi::{Button, Deck, Feedback, Layer, Mapping, PadIndex, PadMode, U7};

use crate::pads::{PAD_COUNT, PAD_MODES};

const LAYERS: [Layer; 2] = [Layer::Normal, Layer::Shift];
const DECKS: [Deck; DECK_COUNT] = [Deck::A, Deck::B];

/// The lights of one deck.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeckLights {
    /// PLAY/PAUSE.
    pub play: bool,
    /// CUE.
    pub cue: bool,
    /// The pad mode the pads are in, whose button is lit.
    pub pad_mode: PadMode,
    /// The pads lit in that mode, pad 1 first.
    pub pads: [bool; PAD_COUNT],
    /// The channel level meter.
    pub level: U7,
    /// LOOP IN.
    pub loop_in: bool,
    /// LOOP OUT.
    pub loop_out: bool,
    /// 4 BEAT/EXIT.
    pub four_beat: bool,
}

impl Default for DeckLights {
    fn default() -> Self {
        Self {
            play: false,
            cue: false,
            pad_mode: PadMode::HotCue,
            pads: [false; PAD_COUNT],
            level: U7::MIN,
            loop_in: false,
            loop_out: false,
            four_beat: false,
        }
    }
}

/// Every light Dubplate drives on a controller, as it should be now. Lights the app has no
/// feature behind yet are kept dark, so a controller that comes from another app starts clean.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Lights {
    /// Deck A, then deck B.
    pub decks: [DeckLights; DECK_COUNT],
    /// BEAT FX ON/OFF.
    pub beat_fx_on: bool,
}

impl Lights {
    /// Calls `show` with every light and output, in a fixed order. Lights a controller does not
    /// have encode to nothing in its mapping.
    pub fn for_each_feedback(&self, mut show: impl FnMut(Feedback)) {
        for (deck, lights) in DECKS.into_iter().zip(&self.decks) {
            let mut light = |button: Button, on: bool| {
                for layer in LAYERS {
                    show(Feedback::Light { button, layer, on });
                }
            };
            light(Button::Play(deck), lights.play);
            light(Button::Cue(deck), lights.cue);
            light(Button::LoopIn(deck), lights.loop_in);
            light(Button::LoopOut(deck), lights.loop_out);
            light(Button::FourBeatExit(deck), lights.four_beat);
            for unlit in [Button::Sync(deck), Button::HeadphoneCue(deck)] {
                light(unlit, false);
            }
            for mode in PAD_MODES {
                light(Button::PadMode { deck, mode }, mode == lights.pad_mode);
                for (pad_position, lit) in lights.pads.iter().enumerate() {
                    let Some(pad) = pad_index(pad_position) else {
                        continue;
                    };
                    light(
                        Button::Pad { deck, mode, pad },
                        *lit && mode == lights.pad_mode,
                    );
                }
            }
            show(Feedback::LevelMeter {
                deck,
                level: lights.level,
            });
            show(Feedback::VinylMode { deck, on: true });
        }
        for layer in LAYERS {
            show(Feedback::Light {
                button: Button::BeatFxOn,
                layer,
                on: self.beat_fx_on,
            });
            for unlit in [Button::SmartCfx, Button::SmartFader] {
                show(Feedback::Light {
                    button: unlit,
                    layer,
                    on: false,
                });
            }
        }
    }
}

/// The level at which each LED of a Pioneer channel level meter lights, in dBFS, with the
/// value that lights it from the DDJ-FLX4's MIDI list: two green, two orange and a red.
const METER_STEPS: [(f32, u8); 5] = [
    (-24.0, 0x30),
    (-12.0, 0x48),
    (-6.0, 0x5C),
    (-3.0, 0x6C),
    (-0.5, 0x7F),
];

/// The level meter value that shows a level of `decibels` dBFS.
pub fn meter_level(decibels: f32) -> U7 {
    METER_STEPS
        .iter()
        .rev()
        .find(|(threshold, _)| decibels >= *threshold)
        .and_then(|(_, value)| U7::new(*value))
        .unwrap_or(U7::MIN)
}

/// The pad at `position`, counted from 0.
pub fn pad_index(position: usize) -> Option<PadIndex> {
    PadIndex::new(u8::try_from(position + 1).ok()?)
}

/// What was last sent to each light and output of one controller, so only changes go out.
#[derive(Debug)]
pub struct LightCache {
    sent: Box<[Option<u8>]>,
}

/// Note and control change addresses on 16 channels.
const ADDRESS_COUNT: usize = 2 * 16 * 128;

impl Default for LightCache {
    fn default() -> Self {
        Self {
            sent: vec![None; ADDRESS_COUNT].into_boxed_slice(),
        }
    }
}

impl LightCache {
    /// Sends, through `send`, each message of `lights` whose value differs from what this
    /// controller last showed. Returns the first error `send` gave; what was not sent is sent
    /// again next time.
    pub fn show<E>(
        &mut self,
        mapping: &Mapping,
        lights: &Lights,
        mut send: impl FnMut(Feedback) -> Result<(), E>,
    ) -> Result<(), E> {
        let mut result = Ok(());
        lights.for_each_feedback(|feedback| {
            if result.is_err() {
                return;
            }
            let Some(message) = mapping.encode(feedback) else {
                return;
            };
            let [status, number, value] = message;
            let Some(slot) = address_slot(status, number).and_then(|slot| self.sent.get_mut(slot))
            else {
                return;
            };
            if *slot == Some(value) {
                return;
            }
            result = send(feedback);
            if result.is_ok() {
                *slot = Some(value);
            }
        });
        result
    }
}

/// Where a note on or control change status and number sit in the cache.
fn address_slot(status: u8, number: u8) -> Option<usize> {
    let kind = match status >> 4 {
        0x9 => 0,
        0xB => 1,
        _ => return None,
    };
    let channel = usize::from(status & 0x0F);
    Some((kind * 16 + channel) * 128 + usize::from(number & 0x7F))
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use super::*;

    fn flx4() -> Mapping {
        Mapping::built_in_for_port("DDJ-FLX4")
            .expect("the built-in mappings parse")
            .expect("the FLX4 has a built-in mapping")
    }

    fn sent(cache: &mut LightCache, mapping: &Mapping, lights: &Lights) -> Vec<[u8; 3]> {
        let mut messages = Vec::new();
        cache
            .show(mapping, lights, |feedback| {
                messages.extend(mapping.encode(feedback));
                Ok::<(), Infallible>(())
            })
            .expect("sending to a vector cannot fail");
        messages
    }

    #[test]
    fn the_first_frame_sets_every_light_and_later_frames_only_changes() {
        let mapping = flx4();
        let mut cache = LightCache::default();
        let mut lights = Lights::default();
        let first = sent(&mut cache, &mapping, &lights);
        assert!(first.len() > 100, "{} messages", first.len());
        assert!(first.contains(&[0x90, 0x0B, 0x00]), "deck A play is dark");
        assert!(first.contains(&[0x90, 0x1B, 0x7F]), "hot cue mode is lit");
        assert!(first.contains(&[0x90, 0x17, 0x7F]), "vinyl mode is on");
        assert!(sent(&mut cache, &mapping, &lights).is_empty());

        lights.decks[0].play = true;
        lights.decks[1].pads[2] = true;
        let changes = sent(&mut cache, &mapping, &lights);
        assert_eq!(
            changes,
            [
                [0x90, 0x0B, 0x7F],
                [0x90, 0x0E, 0x7F],
                [0x99, 0x02, 0x7F],
                [0x9A, 0x02, 0x7F],
            ]
        );
    }

    #[test]
    fn a_new_pad_mode_moves_the_lit_pads_to_its_bank() {
        let mapping = flx4();
        let mut cache = LightCache::default();
        let mut lights = Lights::default();
        lights.decks[0].pads[0] = true;
        sent(&mut cache, &mapping, &lights);
        lights.decks[0].pad_mode = PadMode::BeatJump;
        let changes = sent(&mut cache, &mapping, &lights);
        for expected in [
            [0x90, 0x1B, 0x00],
            [0x90, 0x20, 0x7F],
            [0x97, 0x00, 0x00],
            [0x97, 0x20, 0x7F],
        ] {
            assert!(
                changes.contains(&expected),
                "{expected:02X?} in {changes:02X?}"
            );
        }
    }

    #[test]
    fn the_level_meter_lights_more_leds_as_the_level_rises() {
        let values: Vec<u8> = [-60.0, -20.0, -10.0, -4.0, -1.0, 0.0]
            .map(|decibels| meter_level(decibels).get())
            .to_vec();
        assert_eq!(values, [0x00, 0x30, 0x48, 0x5C, 0x6C, 0x7F]);
    }

    #[test]
    fn a_failed_send_is_retried_next_time() {
        let mapping = flx4();
        let mut cache = LightCache::default();
        let lights = Lights::default();
        let mut attempts = 0;
        let failed = cache.show(&mapping, &lights, |_| {
            attempts += 1;
            Err("unplugged")
        });
        assert_eq!(failed, Err("unplugged"));
        assert_eq!(attempts, 1);
        assert!(!sent(&mut cache, &mapping, &lights).is_empty());
    }
}
