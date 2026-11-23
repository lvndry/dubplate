//! The sampler: one-shots, loops, gates and synced loops on their own bus to the master.

mod common;

use common::{SAMPLE_RATE, assert_close, constant_track, ramp_track, sine_track};
use dubplate_engine::{
    BANK_SLOTS, BeatGrid, Controller, DeckId, MasterTempo, Renderer, SAMPLER_BANKS, SAMPLER_SLOTS,
    SYNTH_BPM, SampleMode, SampleSettings, SynthSound, Track, offline_engine,
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

#[test]
fn every_bank_has_its_own_slots_and_they_play_together() {
    assert_eq!(SAMPLER_SLOTS, SAMPLER_BANKS * BANK_SLOTS);
    let (mut controller, mut renderer) = engine();
    let first_bank = 2;
    let last_bank = 3 * BANK_SLOTS + 2;
    let last_slot = SAMPLER_SLOTS - 1;
    for (slot, level) in [(first_bank, 0.1), (last_bank, 0.2), (last_slot, 0.05)] {
        controller
            .load_sample(
                slot,
                constant_track(level, level, 1.0, SAMPLE_RATE),
                SampleSettings::default(),
            )
            .unwrap();
        controller.trigger_sample(slot).unwrap();
    }
    let frames = render(&mut renderer, 0.2);
    assert_close(f64::from(frames[frames.len() - 1][0]), 0.35, 1e-5);
    let state = controller.sampler_state();
    for slot in [first_bank, last_bank, last_slot] {
        assert!(state.slots[slot].unwrap().playing, "slot {slot}");
    }
    assert!(!state.slots[BANK_SLOTS + 2].is_some_and(|slot| slot.playing));
    controller.stop_sample(last_bank).unwrap();
    let after = render(&mut renderer, 0.1);
    assert_close(f64::from(after[after.len() - 1][0]), 0.15, 1e-5);
}

#[test]
fn a_slots_gain_scales_its_sample_exactly() {
    let (mut controller, mut renderer) = engine();
    let half = SampleSettings {
        gain: 0.5,
        ..SampleSettings::default()
    };
    controller
        .load_sample(BANK_SLOTS + 4, one_shot(), half)
        .unwrap();
    controller.trigger_sample(BANK_SLOTS + 4).unwrap();
    let frames = render(&mut renderer, 0.2);
    assert_close(f64::from(frames[frames.len() - 1][0]), 0.125, 1e-6);
    controller
        .set_sample_settings(
            BANK_SLOTS + 4,
            SampleSettings {
                gain: 2.0,
                ..SampleSettings::default()
            },
        )
        .unwrap();
    let louder = render(&mut renderer, 0.1);
    assert_close(f64::from(louder[louder.len() - 1][0]), 0.5, 1e-6);
}

/// A track with a click of `level` on every beat at `bpm`, in one channel only.
fn click_track(bpm: f64, beats: usize, level: f32, right: bool) -> Track {
    let beat_frames = (f64::from(SAMPLE_RATE) * 60.0 / bpm).round() as usize;
    let samples: Vec<f32> = (0..beat_frames * beats)
        .flat_map(|frame| {
            let click = if frame % beat_frames == 0 { level } else { 0.0 };
            if right { [0.0, click] } else { [click, 0.0] }
        })
        .collect();
    Track::from_interleaved_stereo(&samples, SAMPLE_RATE).expect("valid click track")
}

/// The frame of the loudest sample in each `window` frames of one channel.
fn loudest_frames(frames: &[[f32; 4]], channel: usize, window: usize) -> Vec<usize> {
    frames
        .chunks(window)
        .enumerate()
        .map(|(index, chunk)| {
            let (offset, _) = chunk
                .iter()
                .enumerate()
                .max_by(|(_, first), (_, second)| {
                    first[channel].abs().total_cmp(&second[channel].abs())
                })
                .expect("a window is never empty");
            index * window + offset
        })
        .collect()
}

#[test]
fn a_synced_loop_lands_on_the_masters_beats_to_the_sample() {
    let (mut controller, mut renderer) = engine();
    let beat_frames = SAMPLE_RATE as usize / 2;
    controller
        .load_track(DeckId::A, click_track(120.0, 40, 0.5, false))
        .unwrap();
    controller
        .set_beat_grid(DeckId::A, Some(BeatGrid::new(120.0, 0.0).unwrap()))
        .unwrap();
    controller
        .set_master_tempo(MasterTempo::Deck(DeckId::A))
        .unwrap();
    controller.play(DeckId::A).unwrap();
    let synced = SampleSettings {
        mode: SampleMode::Loop,
        sync: true,
        bpm: Some(120.0),
        ..SampleSettings::default()
    };
    let last_bank_slot = 3 * BANK_SLOTS + 7;
    controller
        .load_sample(last_bank_slot, click_track(120.0, 1, 0.25, true), synced)
        .unwrap();
    let lead_in = render(&mut renderer, 0.37);
    controller.trigger_sample(last_bank_slot).unwrap();
    let frames = render(&mut renderer, 3.2);
    let start = lead_in.len();
    let all: Vec<[f32; 4]> = lead_in.into_iter().chain(frames).collect();
    let first_full_beat = start.div_ceil(beat_frames) * beat_frames;
    let measured = &all[first_full_beat..first_full_beat + 6 * beat_frames];
    let deck_clicks = loudest_frames(measured, 0, beat_frames);
    let sample_clicks = loudest_frames(measured, 1, beat_frames);
    assert_eq!(
        sample_clicks, deck_clicks,
        "the sample's clicks land on the deck's clicks as they are heard"
    );
    for pair in sample_clicks.windows(2) {
        assert_eq!(pair[1] - pair[0], beat_frames, "one beat apart exactly");
    }
    assert!(
        measured
            .iter()
            .map(|frame| frame[1].abs())
            .fold(0.0_f32, f32::max)
            > 0.2
    );
}

#[test]
fn a_one_shot_that_follows_the_master_lasts_its_beats_at_the_masters_tempo() {
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
    let riser = SynthSound::Riser;
    controller
        .load_sample(0, riser.track(SAMPLE_RATE).unwrap(), riser.settings())
        .unwrap();
    controller.trigger_sample(0).unwrap();
    let eight_beats_at_125 = 8.0 * 60.0 / 125.0;
    render(&mut renderer, eight_beats_at_125 - 0.02);
    assert!(controller.sampler_state().slots[0].unwrap().playing);
    render(&mut renderer, 0.04);
    assert!(
        !controller.sampler_state().slots[0].unwrap().playing,
        "a riser made at {SYNTH_BPM} BPM ends after 8 beats at 125"
    );
}

#[test]
fn the_synthesized_sounds_peak_at_minus_3_db_and_come_out_the_same_every_time() {
    for sample_rate in [44_100, 48_000, 96_000] {
        for sound in SynthSound::ALL {
            let track = sound.track(sample_rate).unwrap();
            let again = sound.track(sample_rate).unwrap();
            let frames: Vec<[f32; 2]> = (0..track.decoded_frames())
                .map(|index| track.frame(index).unwrap())
                .collect();
            let peak = frames
                .iter()
                .map(|frame| frame[0].abs().max(frame[1].abs()))
                .fold(0.0_f32, f32::max);
            assert!(
                (0.69..=0.7001).contains(&peak),
                "{} at {sample_rate} Hz peaks at {peak}",
                sound.name()
            );
            assert!(frames.iter().all(|frame| frame[0].is_finite()));
            assert_eq!(track.decoded_frames(), again.decoded_frames());
            assert!(
                (0..track.decoded_frames()).all(|index| track.frame(index) == again.frame(index))
            );
        }
    }
}

#[test]
fn the_tempo_locked_sounds_last_whole_beats_and_the_siren_loops_without_a_seam() {
    let rate = f64::from(SAMPLE_RATE);
    let beat_frames = rate * 60.0 / SYNTH_BPM;
    let riser = SynthSound::Riser.track(SAMPLE_RATE).unwrap();
    assert_eq!(riser.decoded_frames() as f64, 8.0 * beat_frames);
    let siren = SynthSound::Siren.track(SAMPLE_RATE).unwrap();
    assert_eq!(siren.decoded_frames() as f64, 2.0 * beat_frames);
    let samples: Vec<f32> = (0..siren.decoded_frames())
        .map(|index| siren.frame(index).unwrap()[0])
        .collect();
    let largest_step = samples
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).abs())
        .fold(0.0_f32, f32::max);
    let seam = (samples[0] - samples[samples.len() - 1]).abs();
    assert!(
        seam <= largest_step,
        "seam {seam} against the largest step {largest_step}"
    );
    assert_eq!(SynthSound::Siren.settings().mode, SampleMode::Loop);
    assert!(SynthSound::Riser.settings().sync);
    assert!(!SynthSound::AirHorn.settings().sync);
}

#[test]
fn a_clip_copies_a_span_of_a_track_to_the_frame() {
    let track = ramp_track(1e-6, SAMPLE_RATE as usize * 2, SAMPLE_RATE);
    let clip = track.clip(0.5, 1.0).unwrap();
    assert_eq!(clip.decoded_frames(), SAMPLE_RATE as usize / 2);
    assert_eq!(clip.frame(0), track.frame(SAMPLE_RATE as usize / 2));
    assert_eq!(
        clip.frame(clip.decoded_frames() - 1),
        track.frame(SAMPLE_RATE as usize - 1)
    );
    assert_eq!(clip.sample_rate(), SAMPLE_RATE);
    assert!(track.clip(1.5, 2.5).is_err(), "past the end");
    assert!(track.clip(1.0, 1.0).is_err(), "empty");
    assert!(track.clip(-0.1, 0.5).is_err(), "before the start");
}
