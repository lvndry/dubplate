//! The fader echo: a deck faded out fast echoes the last of its music as it goes.

mod common;

use common::{SAMPLE_RATE, left_channel, render_frames, sine_track};
use dubplate_engine::{BeatGrid, Controller, DeckId, Renderer, offline_engine};

const BLOCK: usize = 256;

fn playing_deck(fader_echo: bool) -> (Controller, Renderer) {
    let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
    controller
        .load_track(DeckId::A, sine_track(440.0, 0.5, 0.5, 20.0, SAMPLE_RATE))
        .expect("the offline engine takes it");
    controller
        .set_beat_grid(
            DeckId::A,
            Some(BeatGrid::new(124.0, 0.0).expect("124 BPM is a valid grid")),
        )
        .expect("the offline engine takes it");
    controller
        .set_crossfader(0.0)
        .expect("the offline engine takes it");
    controller
        .set_fader_echo(fader_echo)
        .expect("the offline engine takes it");
    controller
        .play(DeckId::A)
        .expect("the offline engine takes it");
    seconds(&mut renderer, 1.0);
    (controller, renderer)
}

fn seconds(renderer: &mut Renderer, seconds: f64) -> Vec<f32> {
    left_channel(&render_frames(
        renderer,
        (seconds * f64::from(SAMPLE_RATE)) as usize,
        BLOCK,
    ))
}

/// Moves deck A's channel fader from the top to `to` over about a tenth of a second.
fn pull_fader(controller: &mut Controller, renderer: &mut Renderer, to: f32) {
    for step in 1..=10 {
        let position = 1.0 - (1.0 - to) * step as f32 / 10.0;
        controller
            .set_channel_fader(DeckId::A, position)
            .expect("the offline engine takes it");
        seconds(renderer, 0.01);
    }
}

fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len() as f32).sqrt()
}

#[test]
fn a_fader_closed_fast_leaves_its_echoes_ringing_only_with_the_fader_echo_on() {
    let mut tails = Vec::new();
    for fader_echo in [false, true] {
        let (mut controller, mut renderer) = playing_deck(fader_echo);
        pull_fader(&mut controller, &mut renderer, 0.0);
        seconds(&mut renderer, 0.05);
        tails.push(rms(&seconds(&mut renderer, 0.5)));
    }
    let [without, with] = tails[..] else {
        unreachable!()
    };
    assert!(without < 1e-3, "a closed fader is silent: {without}");
    assert!(
        with > 0.02,
        "the echoes ring on after the fader shut: {with}"
    );
    assert!(controller_reports_the_setting());
}

fn controller_reports_the_setting() -> bool {
    let (mut controller, _renderer) = offline_engine(SAMPLE_RATE);
    let off = !controller.fader_echo();
    controller
        .set_fader_echo(true)
        .expect("the offline engine takes it");
    off && controller.fader_echo()
}

#[test]
fn a_fader_left_half_way_stops_echoing_and_plays_the_music_as_it_is() {
    let mut levels = Vec::new();
    for fader_echo in [false, true] {
        let (mut controller, mut renderer) = playing_deck(fader_echo);
        pull_fader(&mut controller, &mut renderer, 0.5);
        seconds(&mut renderer, 2.0);
        levels.push(rms(&seconds(&mut renderer, 1.0)));
    }
    let [plain, echoed] = levels[..] else {
        unreachable!()
    };
    let difference_db = 20.0 * (echoed / plain).log10();
    assert!(
        difference_db.abs() < 0.5,
        "a fader at rest plays as without the echo: {difference_db:.2} dB"
    );
}
