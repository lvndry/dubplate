//! The DDJ-FLX4 mapping against Pioneer's "DDJ-FLX4 List of MIDI messages" (Ver 1.0). Every
//! message here is typed in from that list, independently of the mapping file, and the mapping
//! must decode exactly these messages and no others.

use std::collections::HashSet;
use std::sync::Arc;

use dubplate_midi::{
    Action, Button, ButtonState, Continuous, ControlEvent, Deck, Decoder, Direction, Encoder,
    EqBand, FaderStart, Feedback, JogMode, Layer, Mapping, PadIndex, PadMode, Position, Switch,
    Ticks, U7, U14,
};

const DECKS: [Deck; 2] = [Deck::A, Deck::B];
const NORMAL: Layer = Layer::Normal;
const SHIFT: Layer = Layer::Shift;

fn mapping() -> Arc<Mapping> {
    Arc::new(
        Mapping::built_in_for_port("DDJ-FLX4")
            .expect("the built-in mappings parse")
            .expect("the DDJ-FLX4 has a mapping"),
    )
}

fn decoder() -> Decoder {
    Decoder::new(mapping())
}

/// Status byte of a deck's own channel: 1 for deck A, 2 for deck B.
fn deck_status(deck: Deck, base: u8) -> u8 {
    match deck {
        Deck::A => base,
        Deck::B => base + 1,
    }
}

fn u7(value: u8) -> U7 {
    U7::new(value).expect("a 7-bit value")
}

struct ButtonMessage {
    button: Button,
    layer: Layer,
    status: u8,
    number: u8,
}

fn button(button: Button, layer: Layer, status: u8, number: u8) -> ButtonMessage {
    ButtonMessage {
        button,
        layer,
        status,
        number,
    }
}

const PAD_MODES: [(PadMode, u8); 8] = [
    (PadMode::HotCue, 0x00),
    (PadMode::PadFx1, 0x10),
    (PadMode::BeatJump, 0x20),
    (PadMode::Sampler, 0x30),
    (PadMode::Keyboard, 0x40),
    (PadMode::PadFx2, 0x50),
    (PadMode::BeatLoop, 0x60),
    (PadMode::KeyShift, 0x70),
];

fn pad_status(deck: Deck, layer: Layer) -> u8 {
    match (deck, layer) {
        (Deck::A, Layer::Normal) => 0x97,
        (Deck::A, Layer::Shift) => 0x98,
        (Deck::B, Layer::Normal) => 0x99,
        (Deck::B, Layer::Shift) => 0x9A,
    }
}

fn pad_messages() -> Vec<ButtonMessage> {
    let mut messages = Vec::new();
    for deck in DECKS {
        for layer in [NORMAL, SHIFT] {
            for (mode, first) in PAD_MODES {
                for number in 1..=8 {
                    let pad = PadIndex::new(number).expect("pads 1 to 8 exist");
                    let control = Button::Pad { deck, mode, pad };
                    messages.push(button(
                        control,
                        layer,
                        pad_status(deck, layer),
                        first + number - 1,
                    ));
                }
            }
        }
    }
    messages
}

/// Every button in the list except the performance pads.
fn button_messages() -> Vec<ButtonMessage> {
    let mut messages = Vec::new();
    for deck in DECKS {
        let status = deck_status(deck, 0x90);
        let call = |direction| Button::CueLoopCall { deck, direction };
        let mode = |mode| Button::PadMode { deck, mode };
        let start = |start| Button::FaderStart { deck, start };
        messages.extend([
            button(Button::Play(deck), NORMAL, status, 0x0B),
            button(Button::Play(deck), SHIFT, status, 0x0E),
            button(Button::Cue(deck), NORMAL, status, 0x0C),
            button(Button::Cue(deck), SHIFT, status, 0x48),
            button(Button::Shift(deck), NORMAL, status, 0x3F),
            button(Button::JogTouch(deck), NORMAL, status, 0x36),
            button(Button::JogTouch(deck), SHIFT, status, 0x67),
            button(Button::LoopIn(deck), NORMAL, status, 0x10),
            button(Button::LoopIn(deck), SHIFT, status, 0x4C),
            button(Button::LoopOut(deck), NORMAL, status, 0x11),
            button(Button::LoopOut(deck), SHIFT, status, 0x4E),
            button(Button::FourBeatExit(deck), NORMAL, status, 0x4D),
            button(Button::FourBeatExit(deck), SHIFT, status, 0x50),
            button(call(Direction::Left), NORMAL, status, 0x51),
            button(call(Direction::Left), SHIFT, status, 0x3E),
            button(call(Direction::Right), NORMAL, status, 0x53),
            button(call(Direction::Right), SHIFT, status, 0x3D),
            button(Button::Sync(deck), NORMAL, status, 0x58),
            button(Button::SyncHold(deck), NORMAL, status, 0x5C),
            button(Button::Sync(deck), SHIFT, status, 0x60),
            button(Button::HeadphoneCue(deck), NORMAL, status, 0x54),
            button(Button::HeadphoneCue(deck), SHIFT, status, 0x68),
            button(start(FaderStart::Play), SHIFT, status, 0x66),
            button(start(FaderStart::Cue), SHIFT, status, 0x52),
            button(mode(PadMode::HotCue), NORMAL, status, 0x1B),
            button(mode(PadMode::Keyboard), SHIFT, status, 0x69),
            button(mode(PadMode::PadFx1), NORMAL, status, 0x1E),
            button(mode(PadMode::PadFx2), SHIFT, status, 0x6B),
            button(mode(PadMode::BeatJump), NORMAL, status, 0x20),
            button(mode(PadMode::BeatLoop), SHIFT, status, 0x6D),
            button(mode(PadMode::Sampler), NORMAL, status, 0x22),
            button(mode(PadMode::KeyShift), SHIFT, status, 0x6F),
        ]);
    }
    messages.extend([
        button(Button::BeatFxSelect, NORMAL, 0x94, 0x63),
        button(Button::BeatFxSelect, SHIFT, 0x94, 0x64),
        button(Button::BeatFxBeat(Direction::Left), NORMAL, 0x94, 0x4A),
        button(Button::BeatFxBeat(Direction::Left), SHIFT, 0x94, 0x66),
        button(Button::BeatFxBeat(Direction::Right), NORMAL, 0x94, 0x4B),
        button(Button::BeatFxBeat(Direction::Right), SHIFT, 0x94, 0x6B),
        button(Button::BeatFxOn, NORMAL, 0x94, 0x47),
        button(Button::BeatFxOn, NORMAL, 0x95, 0x47),
        button(Button::BeatFxOn, SHIFT, 0x94, 0x43),
        button(Button::BeatFxOn, SHIFT, 0x95, 0x43),
        button(Button::MasterCue, NORMAL, 0x96, 0x63),
        button(Button::MasterCue, SHIFT, 0x96, 0x78),
        button(Button::SmartCfx, NORMAL, 0x96, 0x00),
        button(Button::SmartCfx, SHIFT, 0x96, 0x08),
        button(Button::SmartFader, NORMAL, 0x96, 0x01),
        button(Button::SmartFader, SHIFT, 0x96, 0x09),
        button(Button::BrowsePush, NORMAL, 0x96, 0x41),
        button(Button::BrowsePush, SHIFT, 0x96, 0x42),
        button(Button::Load(Deck::A), NORMAL, 0x96, 0x46),
        button(Button::Load(Deck::A), SHIFT, 0x96, 0x68),
        button(Button::Load(Deck::B), NORMAL, 0x96, 0x47),
        button(Button::Load(Deck::B), SHIFT, 0x96, 0x7A),
    ]);
    messages.extend(pad_messages());
    messages
}

/// Every 14-bit fader and knob: the control, its status and its two control change numbers.
fn continuous_messages() -> Vec<(Continuous, u8, u8, u8)> {
    let mut messages = Vec::new();
    for deck in DECKS {
        let status = deck_status(deck, 0xB0);
        let eq = |band| Continuous::Eq { deck, band };
        messages.extend([
            (Continuous::Tempo(deck), status, 0x00, 0x20),
            (Continuous::Trim(deck), status, 0x04, 0x24),
            (eq(EqBand::High), status, 0x07, 0x27),
            (eq(EqBand::Mid), status, 0x0B, 0x2B),
            (eq(EqBand::Low), status, 0x0F, 0x2F),
            (Continuous::ChannelFader(deck), status, 0x13, 0x33),
        ]);
    }
    messages.extend([
        (Continuous::Filter(Deck::A), 0xB6, 0x17, 0x37),
        (Continuous::Filter(Deck::B), 0xB6, 0x18, 0x38),
        (Continuous::BeatFxLevelDepth, 0xB4, 0x02, 0x22),
        (Continuous::MasterLevel, 0xB6, 0x08, 0x28),
        (Continuous::Crossfader, 0xB6, 0x1F, 0x3F),
        (Continuous::MicLevel, 0xB6, 0x05, 0x25),
        (Continuous::HeadphoneMix, 0xB6, 0x0C, 0x2C),
        (Continuous::HeadphoneLevel, 0xB6, 0x0D, 0x2D),
    ]);
    messages
}

/// Every jog wheel and encoder message.
fn encoder_messages() -> Vec<(Encoder, Layer, u8, u8)> {
    let mut messages = Vec::new();
    for deck in DECKS {
        let status = deck_status(deck, 0xB0);
        let jog = |mode| Encoder::Jog { deck, mode };
        messages.extend([
            (jog(JogMode::Scratch), NORMAL, status, 0x22),
            (jog(JogMode::Nudge), NORMAL, status, 0x23),
            (jog(JogMode::Search), SHIFT, status, 0x29),
            (jog(JogMode::Nudge), NORMAL, status, 0x21),
        ]);
    }
    messages.extend([
        (Encoder::Browse, NORMAL, 0xB6, 0x40),
        (Encoder::Browse, SHIFT, 0xB6, 0x64),
    ]);
    messages
}

fn switch_messages() -> [(Switch, u8, u8); 3] {
    [
        (Switch::BeatFxChannel(Deck::A), 0x94, 0x10),
        (Switch::BeatFxChannel(Deck::B), 0x95, 0x11),
        (Switch::MonoOutput, 0x96, 0x6D),
    ]
}

fn assert_decodes(decoder: &mut Decoder, bytes: [u8; 3], action: Action, layer: Layer) {
    assert_eq!(
        decoder.decode(&bytes),
        Some(ControlEvent { action, layer }),
        "decoding {bytes:02X?}"
    );
}

#[test]
fn every_button_and_pad_decodes_press_and_release() {
    let mut decoder = decoder();
    for message in button_messages() {
        for (value, state) in [(0x7F, ButtonState::Pressed), (0x00, ButtonState::Released)] {
            let action = Action::Button {
                button: message.button,
                state,
            };
            assert_decodes(
                &mut decoder,
                [message.status, message.number, value],
                action,
                message.layer,
            );
        }
    }
}

#[test]
fn every_fourteen_bit_control_joins_its_halves() {
    let mut decoder = decoder();
    for (control, status, most_significant, least_significant) in continuous_messages() {
        for (high, low) in [(0x00, 0x00), (0x12, 0x34), (0x40, 0x00), (0x7F, 0x7F)] {
            assert_eq!(decoder.decode(&[status, most_significant, high]), None);
            let position = Position::new(U14::from_halves(u7(high), u7(low)));
            let action = Action::Continuous { control, position };
            assert_decodes(
                &mut decoder,
                [status, least_significant, low],
                action,
                NORMAL,
            );
        }
    }
}

#[test]
fn fourteen_bit_halves_of_different_controls_do_not_mix() {
    let mut decoder = decoder();
    assert_eq!(decoder.decode(&[0xB0, 0x13, 0x7F]), None);
    assert_eq!(decoder.decode(&[0xB1, 0x13, 0x00]), None);
    let deck_a = Action::Continuous {
        control: Continuous::ChannelFader(Deck::A),
        position: Position::new(U14::from_halves(u7(0x7F), u7(0x7F))),
    };
    assert_decodes(&mut decoder, [0xB0, 0x33, 0x7F], deck_a, NORMAL);
}

#[test]
fn tempo_fader_reads_minus_at_zero_and_centre_at_0x2000() {
    let mut decoder = decoder();
    decoder.decode(&[0xB0, 0x00, 0x40]);
    let Some(ControlEvent {
        action: Action::Continuous { position, .. },
        ..
    }) = decoder.decode(&[0xB0, 0x20, 0x00])
    else {
        panic!("tempo did not decode");
    };
    assert_eq!(position.bipolar(), 0.0);
    decoder.decode(&[0xB0, 0x00, 0x00]);
    let Some(ControlEvent {
        action: Action::Continuous { position, .. },
        ..
    }) = decoder.decode(&[0xB0, 0x20, 0x00])
    else {
        panic!("tempo did not decode");
    };
    assert_eq!(position.bipolar(), -1.0);
}

#[test]
fn jog_wheels_count_signed_ticks_around_0x40() {
    let mut decoder = decoder();
    for (encoder, layer, status, number) in encoder_messages() {
        let Encoder::Jog { .. } = encoder else {
            continue;
        };
        for (value, ticks) in [
            (0x41, 1),
            (0x3F, -1),
            (0x45, 5),
            (0x3B, -5),
            (0x7F, 63),
            (0x00, -64),
        ] {
            let action = Action::Encoder {
                encoder,
                ticks: Ticks::new(ticks),
            };
            assert_decodes(&mut decoder, [status, number, value], action, layer);
        }
    }
}

#[test]
fn browse_knob_counts_in_twos_complement() {
    let mut decoder = decoder();
    for (layer, number) in [(NORMAL, 0x40), (SHIFT, 0x64)] {
        for (value, ticks) in [(0x01, 1), (0x03, 3), (0x7F, -1), (0x7D, -3)] {
            let action = Action::Encoder {
                encoder: Encoder::Browse,
                ticks: Ticks::new(ticks),
            };
            assert_decodes(&mut decoder, [0xB6, number, value], action, layer);
        }
    }
}

#[test]
fn jog_touch_presses_and_releases_with_and_without_shift() {
    let mut decoder = decoder();
    let touch = |state| Action::Button {
        button: Button::JogTouch(Deck::B),
        state,
    };
    assert_decodes(
        &mut decoder,
        [0x91, 0x36, 0x7F],
        touch(ButtonState::Pressed),
        NORMAL,
    );
    assert_decodes(
        &mut decoder,
        [0x91, 0x36, 0x00],
        touch(ButtonState::Released),
        NORMAL,
    );
    assert_decodes(
        &mut decoder,
        [0x81, 0x36, 0x40],
        touch(ButtonState::Released),
        NORMAL,
    );
    assert_decodes(
        &mut decoder,
        [0x91, 0x67, 0x7F],
        touch(ButtonState::Pressed),
        SHIFT,
    );
}

#[test]
fn shift_layer_keeps_the_control_and_marks_the_layer() {
    let mut decoder = decoder();
    let cue = Action::Button {
        button: Button::Cue(Deck::A),
        state: ButtonState::Pressed,
    };
    assert_decodes(&mut decoder, [0x90, 0x0C, 0x7F], cue, NORMAL);
    assert_decodes(&mut decoder, [0x90, 0x48, 0x7F], cue, SHIFT);
    let hot_cue_one = Action::Button {
        button: Button::Pad {
            deck: Deck::B,
            mode: PadMode::HotCue,
            pad: PadIndex::new(1).unwrap(),
        },
        state: ButtonState::Pressed,
    };
    assert_decodes(&mut decoder, [0x9A, 0x00, 0x7F], hot_cue_one, SHIFT);
    let event = decoder.decode(&[0x90, 0x48, 0x7F]).unwrap();
    assert_eq!(event.to_string(), "shift + deck A cue pressed");
}

#[test]
fn fader_start_sends_the_play_and_cue_notes_with_shift() {
    let mut decoder = decoder();
    let start = |deck: Deck, start: FaderStart| Action::Button {
        button: Button::FaderStart { deck, start },
        state: ButtonState::Pressed,
    };
    // The channel faders and the crossfader report fader start with the same notes, so
    // both drive their deck's transport.
    assert_decodes(
        &mut decoder,
        [0x90, 0x66, 0x7F],
        start(Deck::A, FaderStart::Play),
        SHIFT,
    );
    assert_decodes(
        &mut decoder,
        [0x90, 0x52, 0x7F],
        start(Deck::A, FaderStart::Cue),
        SHIFT,
    );
    assert_decodes(
        &mut decoder,
        [0x91, 0x66, 0x7F],
        start(Deck::B, FaderStart::Play),
        SHIFT,
    );
    assert_decodes(
        &mut decoder,
        [0x91, 0x52, 0x7F],
        start(Deck::B, FaderStart::Cue),
        SHIFT,
    );
}

#[test]
fn switches_read_on_and_off() {
    let mut decoder = decoder();
    for (switch, status, number) in switch_messages() {
        assert_decodes(
            &mut decoder,
            [status, number, 0x7F],
            Action::Switch { switch, on: true },
            NORMAL,
        );
        assert_decodes(
            &mut decoder,
            [status, number, 0x00],
            Action::Switch { switch, on: false },
            NORMAL,
        );
    }
}

#[test]
fn the_mapping_holds_exactly_the_listed_messages() {
    let mut listed = HashSet::new();
    for message in button_messages() {
        assert!(
            listed.insert((message.status, message.number)),
            "{:02X} {:02X} listed twice",
            message.status,
            message.number
        );
    }
    for (_, status, most_significant, least_significant) in continuous_messages() {
        assert!(listed.insert((status, most_significant)));
        assert!(listed.insert((status, least_significant)));
    }
    for (_, _, status, number) in encoder_messages() {
        assert!(listed.insert((status, number)));
    }
    for (_, status, number) in switch_messages() {
        assert!(listed.insert((status, number)));
    }

    let mapping = mapping();
    for status in (0x90..=0x9F).chain(0xB0..=0xBF) {
        for number in 0..=0x7F {
            assert_eq!(
                mapping.recognizes(&[status, number, 0x7F]),
                listed.contains(&(status, number)),
                "{status:02X} {number:02X}"
            );
        }
    }
    // A note off reads as a release of the note on at the same note number.
    for status in 0x80..=0x8F {
        for number in 0..=0x7F {
            assert_eq!(
                mapping.recognizes(&[status, number, 0x40]),
                listed.contains(&(status | 0x10, number)),
                "{status:02X} {number:02X}"
            );
        }
    }
}

#[test]
fn display_reads_like_the_hardware() {
    let mut decoder = decoder();
    let jog = decoder.decode(&[0xB0, 0x22, 0x41]).unwrap();
    assert_eq!(jog.to_string(), "deck A jog turn +1 (scratch)");
    let pad = decoder.decode(&[0x99, 0x23, 0x7F]).unwrap();
    assert_eq!(pad.to_string(), "deck B pad 4 (beat jump) pressed");
    decoder.decode(&[0xB6, 0x1F, 0x40]);
    let crossfader = decoder.decode(&[0xB6, 0x3F, 0x00]).unwrap();
    assert_eq!(crossfader.to_string(), "crossfader 50.0% (8192)");
    decoder.decode(&[0xB1, 0x00, 0x00]);
    let tempo = decoder.decode(&[0xB1, 0x20, 0x00]).unwrap();
    assert_eq!(tempo.to_string(), "deck B tempo -100.0% (0)");
}

#[test]
fn lights_encode_to_the_listed_midi_out_messages() {
    let mapping = mapping();
    let light = |button, layer, on| Feedback::Light { button, layer, on };
    let mut lit = Vec::new();
    for deck in DECKS {
        let status = deck_status(deck, 0x90);
        let mode = |mode| Button::PadMode { deck, mode };
        lit.extend([
            (Button::Play(deck), NORMAL, status, 0x0B),
            (Button::Play(deck), SHIFT, status, 0x0E),
            (Button::Cue(deck), NORMAL, status, 0x0C),
            (Button::Cue(deck), SHIFT, status, 0x48),
            (Button::LoopIn(deck), NORMAL, status, 0x10),
            (Button::LoopIn(deck), SHIFT, status, 0x4C),
            (Button::LoopOut(deck), NORMAL, status, 0x11),
            (Button::LoopOut(deck), SHIFT, status, 0x4E),
            (Button::FourBeatExit(deck), NORMAL, status, 0x4D),
            (Button::FourBeatExit(deck), SHIFT, status, 0x50),
            (Button::Sync(deck), NORMAL, status, 0x58),
            (Button::Sync(deck), SHIFT, status, 0x60),
            (Button::HeadphoneCue(deck), NORMAL, status, 0x54),
            (Button::HeadphoneCue(deck), SHIFT, status, 0x68),
            (mode(PadMode::HotCue), NORMAL, status, 0x1B),
            (mode(PadMode::Keyboard), SHIFT, status, 0x69),
            (mode(PadMode::PadFx1), NORMAL, status, 0x1E),
            (mode(PadMode::PadFx2), SHIFT, status, 0x6B),
            (mode(PadMode::BeatJump), NORMAL, status, 0x20),
            (mode(PadMode::BeatLoop), SHIFT, status, 0x6D),
            (mode(PadMode::Sampler), NORMAL, status, 0x22),
            (mode(PadMode::KeyShift), SHIFT, status, 0x6F),
        ]);
    }
    lit.extend([
        (Button::SmartCfx, NORMAL, 0x96, 0x00),
        (Button::SmartFader, NORMAL, 0x96, 0x01),
        (Button::BeatFxOn, NORMAL, 0x94, 0x47),
        (Button::BeatFxOn, SHIFT, 0x94, 0x43),
    ]);
    lit.extend(pad_messages().into_iter().map(|message| {
        (
            message.button,
            message.layer,
            message.status,
            message.number,
        )
    }));
    for (button, layer, status, number) in lit {
        assert_eq!(
            mapping.encode(light(button, layer, true)),
            Some([status, number, 0x7F])
        );
        assert_eq!(
            mapping.encode(light(button, layer, false)),
            Some([status, number, 0x00])
        );
    }

    for unlit in [
        Button::Shift(Deck::A),
        Button::Load(Deck::B),
        Button::BrowsePush,
        Button::MasterCue,
    ] {
        assert_eq!(mapping.encode(light(unlit, NORMAL, true)), None, "{unlit}");
    }
}

#[test]
fn meters_track_loaded_and_vinyl_mode_encode() {
    let mapping = mapping();
    let level = Feedback::LevelMeter {
        deck: Deck::B,
        level: u7(0x60),
    };
    assert_eq!(mapping.encode(level), Some([0xB1, 0x02, 0x60]));
    assert_eq!(
        mapping.encode(Feedback::TrackLoaded(Deck::A)),
        Some([0x9F, 0x00, 0x7F])
    );
    assert_eq!(
        mapping.encode(Feedback::TrackLoaded(Deck::B)),
        Some([0x9F, 0x01, 0x7F])
    );
    let vinyl_off = Feedback::VinylMode {
        deck: Deck::A,
        on: false,
    };
    assert_eq!(mapping.encode(vinyl_off), Some([0x90, 0x17, 0x00]));
}

#[test]
fn the_flx4_is_found_by_its_port_name_only() {
    assert!(
        Mapping::built_in_for_port("DDJ-FLX4 MIDI 1")
            .unwrap()
            .is_some()
    );
    assert!(Mapping::built_in_for_port("ddj-flx4").unwrap().is_some());
    assert!(
        Mapping::built_in_for_port("IAC Driver Bus 1")
            .unwrap()
            .is_none()
    );
    assert_eq!(mapping().name(), "Pioneer DDJ-FLX4");
    assert_eq!(mapping().jog_ticks_per_revolution(), Some(720));
}

#[test]
fn a_pad_released_with_a_real_note_off_decodes_like_its_note_on_release() {
    let mut decoder = decoder();
    let pad = Button::Pad {
        deck: Deck::A,
        mode: PadMode::HotCue,
        pad: PadIndex::new(5).unwrap(),
    };
    assert_decodes(
        &mut decoder,
        [0x97, 0x04, 0x7F],
        Action::Button {
            button: pad,
            state: ButtonState::Pressed,
        },
        NORMAL,
    );
    assert_decodes(
        &mut decoder,
        [0x87, 0x04, 0x40],
        Action::Button {
            button: pad,
            state: ButtonState::Released,
        },
        NORMAL,
    );
}
