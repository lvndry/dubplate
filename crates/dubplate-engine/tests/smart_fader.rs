//! Smart Fader: the crossfader moves a shared tempo and swaps the bass.

mod common;

use common::{SAMPLE_RATE, assert_close, constant_track, render_frames};
use dubplate_engine::{
    BeatGrid, Controller, CrossfaderAssign, DeckId, EqBand, Renderer, SmartFaderSource, eq_gain,
    offline_engine,
};

const BLOCK: usize = 256;

/// Both decks play a constant, which is all bass, on 120 and 130 BPM grids.
fn two_decks() -> (Controller, Renderer) {
    let (mut controller, renderer) = offline_engine(SAMPLE_RATE);
    for (deck, bpm) in DeckId::ALL.into_iter().zip([120.0, 130.0]) {
        controller
            .load_track(deck, constant_track(0.5, 0.5, 60.0, SAMPLE_RATE))
            .expect("load");
        controller
            .set_beat_grid(deck, Some(BeatGrid::new(bpm, 0.0).expect("grid")))
            .expect("grid");
        controller.play(deck).expect("play");
    }
    (controller, renderer)
}

fn seconds(renderer: &mut Renderer, seconds: f64) {
    render_frames(renderer, (seconds * f64::from(SAMPLE_RATE)) as usize, BLOCK);
}

fn channel_levels(controller: &Controller, renderer: &mut Renderer) -> [f32; 2] {
    seconds(renderer, 0.3);
    controller.take_meters();
    seconds(renderer, 0.1);
    controller.take_meters().channel_peaks
}

#[test]
fn the_bass_swaps_across_the_middle_of_the_crossfader_and_a_turned_low_knob_wins() {
    let (mut controller, mut renderer) = two_decks();
    controller.set_smart_fader(true).unwrap();
    assert!(controller.smart_fader());
    controller.set_crossfader(0.5).unwrap();
    let middle = channel_levels(&controller, &mut renderer);
    let half_cut = 0.5 * eq_gain(0.25);
    assert_close(f64::from(middle[0]), f64::from(half_cut), 0.01);
    assert_close(f64::from(middle[1]), f64::from(half_cut), 0.01);
    controller.set_crossfader(0.9).unwrap();
    let right = channel_levels(&controller, &mut renderer);
    assert!(right[0] < 0.01, "deck A's bass is killed: {}", right[0]);
    assert_close(f64::from(right[1]), 0.5, 0.01);
    controller.set_eq(DeckId::A, EqBand::Low, 0.75).unwrap();
    let overridden = channel_levels(&controller, &mut renderer);
    assert!(
        overridden[0] > 0.5,
        "the DJ's LOW knob wins: {}",
        overridden[0]
    );
    controller.set_smart_fader(false).unwrap();
    controller.set_eq(DeckId::A, EqBand::Low, 0.5).unwrap();
    let off = channel_levels(&controller, &mut renderer);
    assert_close(f64::from(off[0]), 0.5, 0.01);
    assert_close(f64::from(off[1]), 0.5, 0.01);
}

#[test]
fn the_crossfader_moves_one_shared_tempo_from_deck_to_deck() {
    let (mut controller, mut renderer) = two_decks();
    controller.set_crossfader(0.0).unwrap();
    controller.set_smart_fader(true).unwrap();
    seconds(&mut renderer, 3.0);
    for deck in DeckId::ALL {
        assert_close(controller.tempo_state(deck).bpm.unwrap(), 120.0, 0.05);
    }
    controller.set_crossfader(0.5).unwrap();
    seconds(&mut renderer, 3.0);
    let middle = (120.0_f64 * 130.0).sqrt();
    for deck in DeckId::ALL {
        assert_close(controller.tempo_state(deck).bpm.unwrap(), middle, 0.05);
    }
    controller.set_crossfader(1.0).unwrap();
    seconds(&mut renderer, 3.0);
    for deck in DeckId::ALL {
        assert_close(controller.tempo_state(deck).bpm.unwrap(), 130.0, 0.05);
    }
    let beat = |deck: DeckId| {
        controller
            .beat_grid(deck)
            .unwrap()
            .beat_at(controller.deck_state(deck).position_seconds)
    };
    let difference = beat(DeckId::A) - beat(DeckId::B);
    assert!(
        (difference - difference.round()).abs() < 0.01,
        "the beats stay locked"
    );

    controller.set_smart_fader(false).unwrap();
    seconds(&mut renderer, 1.0);
    assert_close(controller.tempo_state(DeckId::B).bpm.unwrap(), 130.0, 0.05);
    assert_close(controller.tempo_state(DeckId::A).bpm.unwrap(), 130.0, 0.05);
    assert!(controller.tempo_state(DeckId::A).fader_waits_for_pickup);
}

#[test]
fn smart_fader_rests_while_a_deck_is_on_thru() {
    let (mut controller, mut renderer) = two_decks();
    controller
        .set_crossfader_assign(DeckId::B, CrossfaderAssign::Thru)
        .unwrap();
    controller.set_crossfader(0.9).unwrap();
    controller.set_smart_fader(true).unwrap();
    seconds(&mut renderer, 3.0);
    assert_close(controller.tempo_state(DeckId::A).bpm.unwrap(), 120.0, 0.05);
    assert_close(controller.tempo_state(DeckId::B).bpm.unwrap(), 130.0, 0.05);
    let levels = channel_levels(&controller, &mut renderer);
    assert_close(f64::from(levels[0]), 0.5, 0.01);
    assert_close(f64::from(levels[1]), 0.5, 0.01);
}

#[test]
fn smart_fader_follows_the_decks_to_the_sides_they_are_assigned() {
    let (mut controller, mut renderer) = two_decks();
    controller
        .set_crossfader_assign(DeckId::A, CrossfaderAssign::B)
        .unwrap();
    controller
        .set_crossfader_assign(DeckId::B, CrossfaderAssign::A)
        .unwrap();
    controller.set_crossfader(0.0).unwrap();
    controller.set_smart_fader(true).unwrap();
    seconds(&mut renderer, 3.0);
    for deck in DeckId::ALL {
        assert_close(controller.tempo_state(deck).bpm.unwrap(), 130.0, 0.05);
    }
    let levels = channel_levels(&controller, &mut renderer);
    assert!(
        levels[0] < 0.01,
        "deck A, on the far side, has its bass swapped out"
    );
    assert_close(f64::from(levels[1]), 0.5, 0.01);
}

#[test]
fn the_channel_faders_alone_move_the_blend_even_with_a_deck_on_thru() {
    let (mut controller, mut renderer) = two_decks();
    controller
        .set_crossfader_assign(DeckId::B, CrossfaderAssign::Thru)
        .unwrap();
    controller
        .set_smart_fader_source(SmartFaderSource::ChannelFaders)
        .unwrap();
    assert_eq!(
        controller.smart_fader_source(),
        SmartFaderSource::ChannelFaders
    );
    controller.set_smart_fader(true).unwrap();
    seconds(&mut renderer, 3.0);
    let middle = (120.0_f64 * 130.0).sqrt();
    for deck in DeckId::ALL {
        assert_close(controller.tempo_state(deck).bpm.unwrap(), middle, 0.05);
    }
    controller.set_channel_fader(DeckId::A, 0.0).unwrap();
    seconds(&mut renderer, 3.0);
    for deck in DeckId::ALL {
        assert_close(controller.tempo_state(deck).bpm.unwrap(), 130.0, 0.05);
    }
}

#[test]
fn with_both_sources_a_lowered_channel_fader_leans_the_crossfaders_blend() {
    let (mut controller, mut renderer) = two_decks();
    controller.set_crossfader(0.5).unwrap();
    controller.set_smart_fader(true).unwrap();
    controller.set_channel_fader(DeckId::B, 0.0).unwrap();
    seconds(&mut renderer, 3.0);
    for deck in DeckId::ALL {
        assert_close(controller.tempo_state(deck).bpm.unwrap(), 120.0, 0.05);
    }
    controller
        .set_smart_fader_source(SmartFaderSource::Crossfader)
        .unwrap();
    seconds(&mut renderer, 3.0);
    let middle = (120.0_f64 * 130.0).sqrt();
    for deck in DeckId::ALL {
        assert_close(controller.tempo_state(deck).bpm.unwrap(), middle, 0.05);
    }
}
