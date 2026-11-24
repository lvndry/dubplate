//! Key lock through the whole engine: tempo changes without pitch change, lined up with the
//! playhead, through loops and jumps.

mod common;

use common::{SAMPLE_RATE, largest_step, left_channel, render_frames, sine_track};
use dubplate_engine::{BeatGrid, BeatLength, Controller, DeckId, Renderer, Track, offline_engine};

const BLOCK: usize = 256;
const BEAT_SECONDS: f64 = 0.5;
const SHORT_LOOP_START: f64 = 1.0;
const SHORT_LOOP_END: f64 = SHORT_LOOP_START + 0.5 / 32.0;

/// A track of short 2 kHz bursts on every beat at 120 BPM, starting at 0.
fn burst_track(seconds: f64) -> Track {
    let rate = f64::from(SAMPLE_RATE);
    let frames = (seconds * rate) as usize;
    let burst_frames = (0.01 * rate) as usize;
    let beat_frames = (BEAT_SECONDS * rate) as usize;
    let samples: Vec<f32> = (0..frames)
        .flat_map(|index| {
            let into_beat = index % beat_frames;
            let value = if into_beat < burst_frames {
                let window = (std::f64::consts::PI * into_beat as f64 / burst_frames as f64).sin();
                (std::f64::consts::TAU * 2_000.0 * index as f64 / rate).sin() * window * 0.8
            } else {
                0.0
            };
            [value as f32, value as f32]
        })
        .collect();
    Track::from_interleaved_stereo(&samples, SAMPLE_RATE).expect("valid test track")
}

fn deck(track: Track, tempo: f64, key_lock: bool) -> (Controller, Renderer) {
    let (mut controller, renderer) = offline_engine(SAMPLE_RATE);
    controller.load_track(DeckId::A, track).expect("load");
    controller
        .set_beat_grid(DeckId::A, Some(BeatGrid::new(120.0, 0.0).expect("grid")))
        .expect("grid");
    controller.set_crossfader(0.0).expect("crossfader");
    controller.set_tempo(DeckId::A, tempo).expect("tempo");
    controller
        .set_key_lock(DeckId::A, key_lock)
        .expect("key lock");
    (controller, renderer)
}

fn render(renderer: &mut Renderer, seconds: f64) -> Vec<f32> {
    left_channel(&render_frames(
        renderer,
        (seconds * f64::from(SAMPLE_RATE)) as usize,
        BLOCK,
    ))
}

/// The strongest frequency of `signal`, from zero crossings.
fn frequency(signal: &[f32]) -> f64 {
    let crossings: Vec<usize> = signal
        .windows(2)
        .enumerate()
        .filter(|(_, pair)| pair[0] < 0.0 && pair[1] >= 0.0)
        .map(|(index, _)| index)
        .collect();
    let first = crossings[0];
    let last = crossings[crossings.len() - 1];
    (crossings.len() - 1) as f64 * f64::from(SAMPLE_RATE) / (last - first) as f64
}

/// The frames where bursts start: where the 1 ms energy first passes half the loudest burst's.
fn onsets(signal: &[f32]) -> Vec<usize> {
    let window = 48;
    let energy: Vec<f32> = signal
        .chunks(window)
        .map(|chunk| chunk.iter().map(|sample| sample * sample).sum::<f32>())
        .collect();
    let threshold = energy.iter().copied().fold(0.0, f32::max) * 0.25;
    let mut found = Vec::new();
    let mut quiet = true;
    for (index, value) in energy.iter().enumerate() {
        if quiet && *value > threshold {
            found.push(index * window);
            quiet = false;
        } else if *value < threshold * 0.2 {
            quiet = true;
        }
    }
    found
}

#[test]
fn key_lock_changes_the_tempo_and_keeps_the_pitch() {
    let tone = sine_track(1_000.0, 0.5, 0.5, 10.0, SAMPLE_RATE);
    let (mut controller, mut renderer) = deck(tone.clone(), 1.08, true);
    controller.play(DeckId::A).unwrap();
    let locked = render(&mut renderer, 3.0);
    let pitch = frequency(&locked[SAMPLE_RATE as usize..]);
    let cents = 1_200.0 * (pitch / 1_000.0).log2();
    assert!(
        cents.abs() < 2.0,
        "key lock moved the pitch by {cents} cents"
    );
    assert!((controller.deck_state(DeckId::A).position_seconds - 3.0 * 1.08).abs() < 0.01);

    let (mut controller, mut renderer) = deck(tone, 1.08, false);
    controller.play(DeckId::A).unwrap();
    let plain = render(&mut renderer, 3.0);
    assert!((frequency(&plain[SAMPLE_RATE as usize..]) - 1_080.0).abs() < 1.0);
}

#[test]
fn the_stretched_beats_land_where_the_playhead_is() {
    for tempo in [0.92, 1.08, 1.16] {
        let (mut plain_controller, mut plain_renderer) = deck(burst_track(30.0), tempo, false);
        let (mut locked_controller, mut locked_renderer) = deck(burst_track(30.0), tempo, true);
        plain_controller.play(DeckId::A).unwrap();
        locked_controller.play(DeckId::A).unwrap();
        let plain = onsets(&render(&mut plain_renderer, 8.0));
        let locked = onsets(&render(&mut locked_renderer, 8.0));
        let plain_late: Vec<usize> = plain
            .iter()
            .copied()
            .filter(|onset| *onset > 48_000)
            .collect();
        for onset in plain_late {
            let nearest = locked
                .iter()
                .map(|other| *other as i64 - onset as i64)
                .min_by_key(|difference| difference.abs())
                .unwrap();
            assert!(
                nearest.abs() <= 96,
                "at {tempo}: a beat is off by {nearest} frames"
            );
        }
    }
}

#[test]
fn a_loop_and_a_hot_cue_keep_the_stretched_beats_in_place() {
    let (mut controller, mut renderer) = deck(burst_track(30.0), 1.08, true);
    controller.seek(DeckId::A, 4.25).unwrap();
    controller.play(DeckId::A).unwrap();
    render(&mut renderer, 1.0);
    controller.beat_loop(DeckId::A, BeatLength::TWO).unwrap();
    let looped = onsets(&render(&mut renderer, 6.0));
    let beat_frames = BEAT_SECONDS / 1.08 * f64::from(SAMPLE_RATE);
    let late: Vec<usize> = looped.into_iter().filter(|onset| *onset > 24_000).collect();
    assert!(
        late.len() > 8,
        "beats keep coming round the loop: {}",
        late.len()
    );
    for pair in late.windows(2) {
        let beats = (pair[1] - pair[0]) as f64 / beat_frames;
        assert!((beats - beats.round()).abs() < 0.01, "beats {beats} apart");
    }
    let position = controller.deck_state(DeckId::A).position_seconds;
    assert!((5.0..6.0).contains(&position), "position {position}");

    controller.reloop_or_exit(DeckId::A).unwrap();
    controller.hot_cue(DeckId::A, 12.0).unwrap();
    let after_jump = onsets(&render(&mut renderer, 2.0));
    let first = after_jump
        .iter()
        .find(|onset| **onset > 4_800)
        .copied()
        .unwrap();
    let second = after_jump
        .iter()
        .find(|onset| **onset > first + 4_800)
        .copied()
        .unwrap();
    let beats = (second - first) as f64 / beat_frames;
    assert!(
        (beats - 1.0).abs() < 0.01,
        "beats {beats} apart after the jump"
    );
}

/// The loop is the only audible part of the track, so reading past it produces silence.
fn loop_tone_track(start: f64, end: f64) -> Track {
    let rate = f64::from(SAMPLE_RATE);
    let samples: Vec<f32> = (0..SAMPLE_RATE * 4)
        .flat_map(|frame| {
            let seconds = f64::from(frame) / rate;
            let value = if (start..end).contains(&seconds) {
                (std::f64::consts::TAU * 1_024.0 * seconds).sin() as f32 * 0.5
            } else {
                0.0
            };
            [value, value]
        })
        .collect();
    Track::from_interleaved_stereo(&samples, SAMPLE_RATE).expect("valid loop tone track")
}

#[test]
fn manual_loop_out_redirects_stretched_audio_that_read_past_it() {
    let tempo = 1.0248;
    let (mut controller, mut renderer) = deck(loop_tone_track(1.0, 2.0), tempo, true);
    controller.seek(DeckId::A, 1.0).unwrap();
    controller.loop_in(DeckId::A).unwrap();
    controller.play(DeckId::A).unwrap();
    // Quantize places OUT at 2 s while the playhead is still inside and read-ahead is past it.
    render(&mut renderer, 0.95 / tempo);
    controller.loop_out(DeckId::A).unwrap();
    let output = render(&mut renderer, 1.5);
    let state = controller.loop_state(DeckId::A);
    assert_eq!(state.end_seconds, Some(2.0));
    let position = controller.deck_state(DeckId::A).position_seconds;
    assert!(
        (1.0..2.0).contains(&position),
        "loop playhead is at {position}"
    );
    assert_loop_tone_audible(&output);
}

fn assert_short_loop_audible(controller: &Controller, renderer: &mut Renderer) {
    let output = render(renderer, 0.5);
    let position = controller.deck_state(DeckId::A).position_seconds;
    assert!(
        (SHORT_LOOP_START..SHORT_LOOP_END).contains(&position),
        "loop playhead is at {position}"
    );
    assert_loop_tone_audible(&output);
}

fn assert_loop_tone_audible(output: &[f32]) {
    let settled = &output[output.len() / 2..];
    let power = settled.iter().map(|sample| sample * sample).sum::<f32>() / settled.len() as f32;
    assert!(power > 0.01, "the repeated loop tone has power {power}");
}

#[test]
fn setting_a_short_loop_redirects_stretched_audio_that_read_past_it() {
    let track = loop_tone_track(SHORT_LOOP_START, SHORT_LOOP_END);
    let (mut controller, mut renderer) = deck(track, 1.25, true);
    controller.play(DeckId::A).unwrap();
    render(&mut renderer, 0.8);
    controller
        .beat_loop(DeckId::A, BeatLength::THIRTY_SECOND)
        .unwrap();
    assert_short_loop_audible(&controller, &mut renderer);

    controller.reloop_or_exit(DeckId::A).unwrap();
    let output = render(&mut renderer, 0.5);
    let peak = output[output.len() / 2..]
        .iter()
        .map(|sample| sample.abs())
        .fold(0.0, f32::max);
    assert!(
        peak < 0.001,
        "loop tone persisted after exit at peak {peak}"
    );
}

#[test]
fn shrinking_a_loop_redirects_stretched_audio_to_its_new_end() {
    let track = loop_tone_track(SHORT_LOOP_START, SHORT_LOOP_END);
    let (mut controller, mut renderer) = deck(track, 1.25, true);
    controller.seek(DeckId::A, SHORT_LOOP_START).unwrap();
    controller.beat_loop(DeckId::A, BeatLength::FOUR).unwrap();
    controller.play(DeckId::A).unwrap();
    // Enter the second pass with the playhead before the new end and the feeder beyond it.
    render(&mut renderer, 1.604);
    controller
        .beat_loop(DeckId::A, BeatLength::THIRTY_SECOND)
        .unwrap();
    assert_short_loop_audible(&controller, &mut renderer);
}

#[test]
fn a_short_hot_loop_primes_the_stretcher_with_its_repeating_audio() {
    let track = loop_tone_track(SHORT_LOOP_START, SHORT_LOOP_END);
    let (mut controller, mut renderer) = deck(track, 1.25, true);
    controller
        .hot_loop(DeckId::A, SHORT_LOOP_START, SHORT_LOOP_END)
        .unwrap();
    assert_short_loop_audible(&controller, &mut renderer);
}

#[test]
fn switching_key_lock_never_clicks() {
    let tone = sine_track(220.0, 0.5, 0.5, 20.0, SAMPLE_RATE);
    let (mut controller, mut renderer) = deck(tone, 1.06, false);
    controller.play(DeckId::A).unwrap();
    render(&mut renderer, 0.5);
    let natural_step = 0.5 * std::f32::consts::TAU * 220.0 * 1.06 / SAMPLE_RATE as f32;
    for enabled in [true, false, true] {
        controller.set_key_lock(DeckId::A, enabled).unwrap();
        let output = render(&mut renderer, 1.0);
        assert!(
            largest_step(&output) < natural_step * 2.0,
            "step {} switching to {enabled}",
            largest_step(&output)
        );
    }
    assert!(controller.key_lock(DeckId::A));
}
