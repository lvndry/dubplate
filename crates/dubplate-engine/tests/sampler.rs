//! The sampler: one-shots, loops, gates and synced loops on their own bus to the master.

mod common;

use common::{SAMPLE_RATE, assert_close, constant_track, sine_track};
use dubplate_engine::{
    BeatGrid, Controller, DeckId, MasterTempo, Renderer, SAMPLER_SLOTS, SampleMode, SampleSettings,
    Track, offline_engine,
};

const BLOCK: usize = 256;

fn engine() -> (Controller, Renderer) {
    let (mut controller, renderer) = offline_engine(SAMPLE_RATE);
    controller.set_sampler_volume(1.0).expect("volume");
    controller.set_headphone_level(1.0).expect("level");
    (controller, renderer)
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

fn one_shot() -> Track {
    constant_track(0.25, 0.25, 0.5, SAMPLE_RATE)
}

#[test]
fn a_one_shot_plays_once_and_restarts_when_pressed_again() {
    let (mut controller, mut renderer) = engine();
    controller
        .load_sample(3, one_shot(), SampleSettings::default())
        .unwrap();
    controller.trigger_sample(3).unwrap();
    let frames = render(&mut renderer, 0.3);
    assert_close(f64::from(frames[frames.len() - 1][0]), 0.25, 1e-6);
    assert!(controller.sampler_state().slots[3].unwrap().playing);
    render(&mut renderer, 0.3);
    assert!(!controller.sampler_state().slots[3].unwrap().playing);
    let silent = render(&mut renderer, 0.05);
    assert!(silent.iter().all(|frame| frame[0] == 0.0));
    controller.trigger_sample(3).unwrap();
    let again = render(&mut renderer, 0.1);
    assert_close(f64::from(again[again.len() - 1][0]), 0.25, 1e-6);
}

#[test]
fn a_loop_goes_round_until_stopped_and_a_gate_stops_on_release() {
    let (mut controller, mut renderer) = engine();
    let looping = SampleSettings {
        mode: SampleMode::Loop,
        ..SampleSettings::default()
    };
    controller.load_sample(0, one_shot(), looping).unwrap();
    controller.trigger_sample(0).unwrap();
    let frames = render(&mut renderer, 2.0);
    assert!(
        frames[SAMPLE_RATE as usize..]
            .iter()
            .all(|frame| frame[0] > 0.2)
    );
    controller.release_sample(0).unwrap();
    render(&mut renderer, 0.05);
    assert!(
        controller.sampler_state().slots[0].unwrap().playing,
        "only gates stop on release"
    );
    controller.stop_sample(0).unwrap();
    let stopped = render(&mut renderer, 0.05);
    assert_eq!(stopped[stopped.len() - 1][0], 0.0);

    let gate = SampleSettings {
        mode: SampleMode::Loop,
        gate: true,
        ..SampleSettings::default()
    };
    controller.set_sample_settings(0, gate).unwrap();
    controller.trigger_sample(0).unwrap();
    render(&mut renderer, 0.2);
    controller.release_sample(0).unwrap();
    let released = render(&mut renderer, 0.05);
    assert_eq!(released[released.len() - 1][0], 0.0);
}

#[test]
fn a_synced_loop_plays_at_the_master_tempo_in_phase() {
    let (mut controller, mut renderer) = engine();
    controller
        .load_track(DeckId::A, constant_track(0.0, 0.0, 60.0, SAMPLE_RATE))
        .unwrap();
    controller
        .set_beat_grid(DeckId::A, Some(BeatGrid::new(125.0, 0.0).unwrap()))
        .unwrap();
    controller
        .set_master_tempo(MasterTempo::Deck(DeckId::A))
        .unwrap();
    controller.play(DeckId::A).unwrap();
    let ramp: Vec<f32> = (0..SAMPLE_RATE as usize * 2)
        .flat_map(|index| {
            let value = index as f32 / (SAMPLE_RATE as f32 * 2.0);
            [value, value]
        })
        .collect();
    let two_beats_at_60 = Track::from_interleaved_stereo(&ramp, SAMPLE_RATE).unwrap();
    let synced = SampleSettings {
        mode: SampleMode::Loop,
        sync: true,
        bpm: Some(60.0),
        ..SampleSettings::default()
    };
    controller.load_sample(5, two_beats_at_60, synced).unwrap();
    render(&mut renderer, 0.12);
    let master_beat = controller.deck_state(DeckId::A).position_seconds * 125.0 / 60.0;
    controller.trigger_sample(5).unwrap();
    let frames = render(&mut renderer, 0.2);
    let settled = 1_000;
    let last = frames.len() - 1;
    let per_frame = f64::from(frames[last][0] - frames[settled][0]) / (last - settled) as f64;
    let per_second = per_frame * f64::from(SAMPLE_RATE);
    assert_close(per_second, 0.5 * 125.0 / 120.0, 0.01);
    let at_trigger = f64::from(frames[settled][0]) - per_frame * settled as f64;
    let expected_beat = (master_beat * 0.5).rem_euclid(2.0);
    assert_close(at_trigger * 2.0, expected_beat, 0.02);
}

#[test]
fn the_sampler_reaches_the_master_past_the_faders_and_the_headphones_with_its_cue() {
    let (mut controller, mut renderer) = engine();
    for deck in DeckId::ALL {
        controller.set_channel_fader(deck, 0.0).unwrap();
    }
    controller.set_headphone_mix(0.0).unwrap();
    controller
        .load_sample(
            9,
            sine_track(330.0, 0.3, 0.3, 2.0, SAMPLE_RATE),
            SampleSettings::default(),
        )
        .unwrap();
    controller.trigger_sample(9).unwrap();
    let frames = render(&mut renderer, 0.2);
    assert!(frames.iter().any(|frame| frame[0].abs() > 0.25));
    assert!(
        frames[480..].iter().all(|frame| frame[2] == 0.0),
        "not cued"
    );
    controller.set_sampler_cue(true).unwrap();
    let cued = render(&mut renderer, 0.2);
    assert!(cued.iter().any(|frame| frame[2].abs() > 0.25));
    controller.set_sampler_volume(0.0).unwrap();
    let silent = render(&mut renderer, 0.2);
    assert!(
        silent[silent.len() / 2..]
            .iter()
            .all(|frame| frame[0] == 0.0)
    );
    assert!(controller.take_meters().sampler_peak >= 0.0);
}

#[test]
fn sampler_slots_and_settings_are_checked() {
    let (mut controller, _renderer) = engine();
    assert!(controller.trigger_sample(SAMPLER_SLOTS).is_err());
    let loud = SampleSettings {
        gain: -1.0,
        ..SampleSettings::default()
    };
    assert!(controller.load_sample(0, one_shot(), loud).is_err());
    let fast = SampleSettings {
        bpm: Some(0.0),
        ..SampleSettings::default()
    };
    assert!(controller.load_sample(0, one_shot(), fast).is_err());
    controller
        .load_sample(1, one_shot(), SampleSettings::default())
        .unwrap();
    controller.eject_sample(1).unwrap();
    assert!(controller.sampler_state().slots[1].is_none());
}

#[test]
fn resampled_loops_keep_their_level_across_the_file_seam() {
    for source_rate in [44_100, 96_000, 192_000, 384_000] {
        let (mut controller, mut renderer) = engine();
        let sample = constant_track(0.25, 0.25, 0.1, source_rate);
        controller
            .load_sample(
                0,
                sample,
                SampleSettings {
                    mode: SampleMode::Loop,
                    ..SampleSettings::default()
                },
            )
            .unwrap();
        controller.trigger_sample(0).unwrap();
        let frames = render(&mut renderer, 0.5);
        for (index, frame) in frames.iter().enumerate().skip(SAMPLE_RATE as usize / 20) {
            assert!(
                (frame[0] - 0.25).abs() < 1e-5,
                "source rate {source_rate}, output frame {index}: {}",
                frame[0]
            );
            assert!((frame[1] - 0.25).abs() < 1e-5);
        }
    }
}
