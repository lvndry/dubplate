//! The preview bus: a seam auditioned in the headphones, never heard on the master.

mod common;

use common::{SAMPLE_RATE, assert_close, constant_track, sine_track};
use dubplate_engine::{
    Controller, DeckId, MAX_PREVIEW_SECONDS, Preview, PreviewTrack, Renderer, Track, offline_engine,
};

const BLOCK: usize = 256;

fn preview(first: Track, second: Track, offset: f64, tempo: f64, key_lock: bool) -> Preview {
    Preview {
        first: PreviewTrack {
            track: first,
            start_seconds: 0.5,
            tempo,
        },
        second: PreviewTrack {
            track: second,
            start_seconds: 0.0,
            tempo,
        },
        second_offset_seconds: offset,
        key_lock,
    }
}

/// Renders `seconds` on four channels and returns the frames.
fn render(renderer: &mut Renderer, seconds: f64) -> Vec<[f32; 4]> {
    let blocks = (seconds * f64::from(SAMPLE_RATE)) as usize / BLOCK;
    let mut output = vec![0.0_f32; BLOCK * 4];
    let mut frames = Vec::new();
    for _ in 0..blocks {
        renderer.render(&mut output, 4);
        frames.extend(
            output
                .chunks(4)
                .map(|frame| [frame[0], frame[1], frame[2], frame[3]]),
        );
    }
    frames
}

fn mixing_engine() -> (Controller, Renderer) {
    let (mut controller, renderer) = offline_engine(SAMPLE_RATE);
    controller
        .load_track(DeckId::A, sine_track(220.0, 0.6, 0.6, 20.0, SAMPLE_RATE))
        .expect("load A");
    controller
        .load_track(DeckId::B, sine_track(330.0, 0.6, 0.6, 20.0, SAMPLE_RATE))
        .expect("load B");
    controller.set_headphone_cue(DeckId::A, true).expect("cue");
    controller.set_headphone_level(1.0).expect("level");
    for deck in DeckId::ALL {
        controller.play(deck).expect("play");
    }
    (controller, renderer)
}

#[test]
fn the_master_stays_bit_identical_while_a_preview_plays() {
    let (_quiet_controller, mut quiet_renderer) = mixing_engine();
    let (mut previewing, mut previewing_renderer) = mixing_engine();
    previewing
        .preview(Some(preview(
            sine_track(440.0, 0.5, 0.5, 10.0, SAMPLE_RATE),
            sine_track(550.0, 0.5, 0.5, 10.0, SAMPLE_RATE),
            1.0,
            1.04,
            true,
        )))
        .unwrap();
    let quiet = render(&mut quiet_renderer, 3.0);
    let with_preview = render(&mut previewing_renderer, 3.0);
    for (plain, previewed) in quiet.iter().zip(&with_preview) {
        assert_eq!(plain[..2], previewed[..2], "the master changed");
    }
    assert!(
        quiet
            .iter()
            .zip(&with_preview)
            .any(|(plain, previewed)| plain[2] != previewed[2])
    );
    assert!(previewing.preview_state().playing);
}

#[test]
fn the_second_track_comes_in_at_the_overlap_and_the_preview_ends_by_itself() {
    let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
    controller.set_headphone_level(1.0).unwrap();
    controller
        .preview(Some(preview(
            constant_track(0.2, 0.2, 2.5, SAMPLE_RATE),
            constant_track(0.3, 0.3, 2.0, SAMPLE_RATE),
            1.0,
            1.0,
            false,
        )))
        .unwrap();
    let frames = render(&mut renderer, 1.5);
    let rate = SAMPLE_RATE as usize;
    assert_close(f64::from(frames[rate / 2][2]), 0.2, 1e-4);
    assert_close(f64::from(frames[rate * 5 / 4][2]), 0.5, 1e-4);
    assert!(
        frames
            .iter()
            .all(|frame| frame[0] == 0.0 && frame[1] == 0.0)
    );
    let state = controller.preview_state();
    assert!(state.playing);
    assert_close(state.position_seconds, 1.5, 0.01);
    render(&mut renderer, 2.0);
    let ended = controller.preview_state();
    assert!(!ended.playing, "both tracks ended at 3 s");
    let after = render(&mut renderer, 0.1);
    assert!(after.iter().all(|frame| frame[2] == 0.0));
}

#[test]
fn a_preview_stops_after_thirty_seconds_and_on_request() {
    let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
    let long = constant_track(0.1, 0.1, 60.0, SAMPLE_RATE);
    controller
        .preview(Some(preview(long.clone(), long.clone(), 2.0, 1.0, false)))
        .unwrap();
    render(&mut renderer, MAX_PREVIEW_SECONDS + 0.5);
    assert!(!controller.preview_state().playing);
    controller
        .preview(Some(preview(long.clone(), long, 2.0, 1.0, false)))
        .unwrap();
    render(&mut renderer, 0.5);
    assert!(controller.preview_state().playing);
    controller.preview(None).unwrap();
    render(&mut renderer, 0.1);
    assert!(!controller.preview_state().playing);
}

#[test]
fn a_key_locked_preview_keeps_the_pitch_at_its_tempo() {
    let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
    controller.set_headphone_level(1.0).unwrap();
    let tone = sine_track(1_000.0, 0.4, 0.4, 10.0, SAMPLE_RATE);
    controller
        .preview(Some(preview(tone.clone(), tone, 20.0, 1.08, true)))
        .unwrap();
    let frames = render(&mut renderer, 2.0);
    let headphones: Vec<f32> = frames[SAMPLE_RATE as usize..]
        .iter()
        .map(|frame| frame[2])
        .collect();
    let crossings: Vec<usize> = headphones
        .windows(2)
        .enumerate()
        .filter(|(_, pair)| pair[0] < 0.0 && pair[1] >= 0.0)
        .map(|(index, _)| index)
        .collect();
    let pitch = (crossings.len() - 1) as f64 * f64::from(SAMPLE_RATE)
        / (crossings[crossings.len() - 1] - crossings[0]) as f64;
    assert!((pitch - 1_000.0).abs() < 2.0, "pitch {pitch}");
}

#[test]
fn bad_previews_are_rejected() {
    let (mut controller, _renderer) = offline_engine(SAMPLE_RATE);
    let track = constant_track(0.1, 0.1, 1.0, SAMPLE_RATE);
    assert!(
        controller
            .preview(Some(preview(track.clone(), track.clone(), 1.0, 0.0, false)))
            .is_err()
    );
    assert!(
        controller
            .preview(Some(preview(track.clone(), track, -1.0, 1.0, false)))
            .is_err()
    );
}
