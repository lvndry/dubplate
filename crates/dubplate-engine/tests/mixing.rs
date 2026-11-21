//! Offline tests of the mixing core: positions, tempo, crossfader law, gains, cue and seek.

mod common;

use std::f32::consts::FRAC_PI_2;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{
    DECLICK_FRAMES, SAMPLE_RATE, assert_close, constant_track, largest_step, left_channel,
    mixer_latency_frames, ramp_track, render_frames, right_channel,
};
use dubplate_engine::{
    AudioProcessor, DeckId, EngineError, MAX_BLOCK_FRAMES, ProcessContext, StereoBlock,
    offline_engine,
};

const BLOCK: usize = 480;

#[test]
fn playing_advances_the_position_by_the_frames_rendered() {
    let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
    controller
        .load_track(DeckId::A, constant_track(0.1, 0.1, 10.0, SAMPLE_RATE))
        .unwrap();
    controller.play(DeckId::A).unwrap();
    render_frames(&mut renderer, SAMPLE_RATE as usize, BLOCK);
    let state = controller.deck_state(DeckId::A);
    assert!(state.loaded);
    assert!(state.playing);
    assert_close(state.position_seconds, 1.0, 1e-9);
    assert!(!controller.deck_state(DeckId::B).loaded);
}

#[test]
fn tempo_scales_the_position_and_resamples_other_rates() {
    let track_rate = 44_100;
    let frame_count = track_rate as usize * 4;
    let slope = 1.0 / frame_count as f32;
    let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
    controller
        .load_track(DeckId::A, ramp_track(slope, frame_count, track_rate))
        .unwrap();
    controller.set_crossfader(0.0).unwrap();
    controller.set_tempo(DeckId::A, 1.08).unwrap();
    controller.play(DeckId::A).unwrap();
    let output = render_frames(&mut renderer, SAMPLE_RATE as usize, BLOCK);

    let state = controller.deck_state(DeckId::A);
    assert_close(state.tempo, 1.08, 0.0);
    assert_close(state.position_seconds, 1.08, 1e-9);

    let source_frames_per_output_frame = 1.08 * f64::from(track_rate) / f64::from(SAMPLE_RATE);
    let left = left_channel(&output);
    let latency = mixer_latency_frames(&controller);
    for output_frame in [DECLICK_FRAMES, 10_000, 47_000] {
        let expected = output_frame as f64 * source_frames_per_output_frame * f64::from(slope);
        assert_close(f64::from(left[output_frame + latency]), expected, 1e-5);
    }
}

#[test]
fn crossfader_follows_a_constant_power_curve() {
    let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
    controller
        .load_track(DeckId::A, constant_track(0.5, 0.0, 10.0, SAMPLE_RATE))
        .unwrap();
    controller
        .load_track(DeckId::B, constant_track(0.0, 0.5, 10.0, SAMPLE_RATE))
        .unwrap();
    controller.play(DeckId::A).unwrap();
    controller.play(DeckId::B).unwrap();
    for position in [0.0_f32, 0.25, 0.5, 0.75, 1.0] {
        controller.set_crossfader(position).unwrap();
        let output = render_frames(&mut renderer, BLOCK * 2, BLOCK);
        let deck_a_gain = output[output.len() - 2] / 0.5;
        let deck_b_gain = output[output.len() - 1] / 0.5;
        let angle = position * FRAC_PI_2;
        assert_close(f64::from(deck_a_gain), f64::from(angle.cos()), 1e-5);
        assert_close(f64::from(deck_b_gain), f64::from(angle.sin()), 1e-5);
        assert_close(
            f64::from(deck_a_gain.powi(2) + deck_b_gain.powi(2)),
            1.0,
            1e-5,
        );
    }
}

#[test]
fn a_crossfader_cut_moves_without_a_click() {
    let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
    controller
        .load_track(DeckId::A, constant_track(0.8, 0.8, 10.0, SAMPLE_RATE))
        .unwrap();
    controller.set_crossfader(0.0).unwrap();
    controller.play(DeckId::A).unwrap();
    render_frames(&mut renderer, BLOCK, BLOCK);
    controller.set_crossfader(1.0).unwrap();
    let output = left_channel(&render_frames(&mut renderer, BLOCK, BLOCK));
    assert!(largest_step(&output) <= 0.8 / DECLICK_FRAMES as f32 + 1e-6);
    assert_eq!(output[DECLICK_FRAMES], 0.0);
}

#[test]
fn trim_channel_fader_and_master_gain_scale_the_mix() {
    let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
    controller
        .load_track(DeckId::A, constant_track(0.8, -0.8, 10.0, SAMPLE_RATE))
        .unwrap();
    controller.set_crossfader(0.0).unwrap();
    controller.set_gain(DeckId::A, 0.5).unwrap();
    controller.set_channel_fader(DeckId::A, 0.5).unwrap();
    controller.set_master_gain(0.5).unwrap();
    controller.play(DeckId::A).unwrap();
    render_frames(&mut renderer, BLOCK, BLOCK);
    controller.take_meters();
    let output = render_frames(&mut renderer, BLOCK, BLOCK);
    let expected = 0.8 * 0.5 * 0.25 * 0.5;
    assert_close(f64::from(output[output.len() - 2]), expected, 1e-6);
    assert_close(f64::from(output[output.len() - 1]), -expected, 1e-6);

    let meters = controller.take_meters();
    assert_close(f64::from(meters.deck_peaks[0]), 0.8 * 0.5 * 0.25, 1e-6);
    assert_close(f64::from(meters.master_peak), expected, 1e-6);
    assert_eq!(controller.take_meters().master_peak, 0.0);
}

#[test]
fn cue_sets_the_point_when_stopped_and_returns_to_it_when_playing() {
    let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
    controller
        .load_track(DeckId::A, constant_track(0.1, 0.1, 10.0, SAMPLE_RATE))
        .unwrap();
    controller.seek(DeckId::A, 2.0).unwrap();
    controller.cue(DeckId::A).unwrap();
    render_frames(&mut renderer, BLOCK, BLOCK);
    assert_close(
        controller.deck_state(DeckId::A).cue_point_seconds,
        2.0,
        1e-9,
    );

    controller.play(DeckId::A).unwrap();
    render_frames(&mut renderer, SAMPLE_RATE as usize / 2, BLOCK);
    assert_close(controller.deck_state(DeckId::A).position_seconds, 2.5, 1e-9);

    controller.cue(DeckId::A).unwrap();
    let output = left_channel(&render_frames(&mut renderer, BLOCK, BLOCK));
    let state = controller.deck_state(DeckId::A);
    assert!(!state.playing);
    assert_close(state.position_seconds, 2.0, 1e-9);
    assert!(largest_step(&output) <= 0.1 / DECLICK_FRAMES as f32 + 1e-6);
    assert_eq!(output[BLOCK - 1], 0.0);

    controller.set_cue_point(DeckId::A, 4.0).unwrap();
    controller.play(DeckId::A).unwrap();
    render_frames(&mut renderer, BLOCK, BLOCK);
    controller.cue(DeckId::A).unwrap();
    render_frames(&mut renderer, BLOCK, BLOCK);
    assert_close(controller.deck_state(DeckId::A).position_seconds, 4.0, 1e-9);
}

#[test]
fn seeking_while_playing_crossfades_instead_of_jumping() {
    let frame_count = SAMPLE_RATE as usize * 10;
    let slope = 1.0 / frame_count as f32;
    let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
    controller
        .load_track(DeckId::A, ramp_track(slope, frame_count, SAMPLE_RATE))
        .unwrap();
    controller.set_crossfader(0.0).unwrap();
    controller.play(DeckId::A).unwrap();
    render_frames(&mut renderer, SAMPLE_RATE as usize, BLOCK);
    controller.seek(DeckId::A, 9.0).unwrap();
    let latency = mixer_latency_frames(&controller);
    let output = left_channel(&render_frames(&mut renderer, BLOCK + latency, BLOCK));

    let jump = 8.0 * SAMPLE_RATE as f32 * slope;
    let step = largest_step(&output);
    assert!(
        step <= jump / DECLICK_FRAMES as f32 + 2.0 * slope,
        "step {step}"
    );
    assert_close(
        f64::from(output[BLOCK - 1 + latency]),
        0.9 + f64::from((BLOCK - 1) as f32 * slope),
        1e-4,
    );
    assert_close(
        controller.deck_state(DeckId::A).position_seconds,
        9.0 + (BLOCK + latency) as f64 / f64::from(SAMPLE_RATE),
        1e-9,
    );
}

#[test]
fn seeking_while_stopped_moves_without_playing() {
    let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
    controller
        .load_track(DeckId::A, constant_track(0.1, 0.1, 10.0, SAMPLE_RATE))
        .unwrap();
    controller.seek(DeckId::A, 3.25).unwrap();
    let output = render_frames(&mut renderer, BLOCK, BLOCK);
    assert!(output.iter().all(|sample| *sample == 0.0));
    let state = controller.deck_state(DeckId::A);
    assert!(!state.playing);
    assert_close(state.position_seconds, 3.25, 1e-9);
}

#[test]
fn pause_fades_out_and_keeps_the_position() {
    let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
    controller
        .load_track(DeckId::A, constant_track(1.0, 1.0, 10.0, SAMPLE_RATE))
        .unwrap();
    controller.set_crossfader(0.0).unwrap();
    render_frames(&mut renderer, BLOCK, BLOCK);
    controller.play(DeckId::A).unwrap();
    let start = left_channel(&render_frames(&mut renderer, BLOCK, BLOCK));
    assert!(largest_step(&start) <= 1.0 / DECLICK_FRAMES as f32 + 1e-6);
    controller.pause(DeckId::A).unwrap();
    let output = left_channel(&render_frames(&mut renderer, BLOCK, BLOCK));
    assert!(largest_step(&output) <= 1.0 / DECLICK_FRAMES as f32 + 1e-6);
    assert_eq!(
        output[DECLICK_FRAMES + mixer_latency_frames(&controller)],
        0.0
    );
    let state = controller.deck_state(DeckId::A);
    assert!(!state.playing);
    assert_close(
        state.position_seconds,
        BLOCK as f64 / f64::from(SAMPLE_RATE),
        1e-9,
    );
}

#[test]
fn a_jump_while_the_pause_fades_out_does_not_cut_the_fade() {
    let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
    controller
        .load_track(DeckId::A, constant_track(1.0, 1.0, 10.0, SAMPLE_RATE))
        .unwrap();
    controller.set_crossfader(0.0).unwrap();
    controller.play(DeckId::A).unwrap();
    render_frames(&mut renderer, BLOCK, BLOCK);
    controller.pause(DeckId::A).unwrap();
    let mut output = left_channel(&render_frames(&mut renderer, 100, 100));
    controller.seek(DeckId::A, 5.0).unwrap();
    controller.cue(DeckId::A).unwrap();
    output.extend(left_channel(&render_frames(&mut renderer, BLOCK, BLOCK)));
    assert!(largest_step(&output) <= 1.0 / DECLICK_FRAMES as f32 + 1e-6);
    assert_close(
        controller.deck_state(DeckId::A).cue_point_seconds,
        5.0,
        1e-9,
    );
}

#[test]
fn jumps_in_quick_succession_across_callbacks_stay_click_free() {
    let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
    controller
        .load_track(DeckId::A, constant_track(0.5, 0.5, 10.0, SAMPLE_RATE))
        .unwrap();
    controller.set_crossfader(0.0).unwrap();
    controller.play(DeckId::A).unwrap();
    let mut output = left_channel(&render_frames(&mut renderer, BLOCK, BLOCK));
    for jump in 0..8 {
        controller.seek(DeckId::A, 1.0 + f64::from(jump)).unwrap();
        output.extend(left_channel(&render_frames(&mut renderer, 48, 48)));
    }
    controller.pause(DeckId::A).unwrap();
    output.extend(left_channel(&render_frames(&mut renderer, 64, 64)));
    controller.cue(DeckId::A).unwrap();
    output.extend(left_channel(&render_frames(&mut renderer, BLOCK, BLOCK)));
    assert!(largest_step(&output) <= 0.5 * 2.0 / DECLICK_FRAMES as f32 + 1e-6);
    assert_eq!(output[output.len() - 1], 0.0);
}

#[test]
fn a_seek_far_past_a_track_still_decoding_is_rejected_or_clamped() {
    let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
    controller
        .load_track(DeckId::A, constant_track(0.5, 0.5, 1.0, SAMPLE_RATE))
        .unwrap();
    assert!(controller.seek(DeckId::A, 1e15).is_err());
    assert!(controller.set_cue_point(DeckId::A, 1e15).is_err());
    controller
        .seek(DeckId::A, dubplate_engine::MAX_TRACK_SECONDS as f64)
        .unwrap();
    controller.play(DeckId::A).unwrap();
    render_frames(&mut renderer, BLOCK, BLOCK);
    assert!(!controller.deck_state(DeckId::A).playing);
}

#[test]
fn playback_stops_at_the_end_of_the_track() {
    let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
    controller
        .load_track(DeckId::A, constant_track(0.1, 0.1, 0.1, SAMPLE_RATE))
        .unwrap();
    controller.play(DeckId::A).unwrap();
    render_frames(&mut renderer, SAMPLE_RATE as usize / 5, BLOCK);
    let state = controller.deck_state(DeckId::A);
    assert!(!state.playing);
    assert_close(state.position_seconds, 0.1, 1e-9);
    controller.play(DeckId::A).unwrap();
    render_frames(&mut renderer, BLOCK, BLOCK);
    assert!(!controller.deck_state(DeckId::A).playing);
}

struct Gain {
    gain: f32,
    largest_block: Arc<AtomicUsize>,
}

impl AudioProcessor for Gain {
    fn process(&mut self, block: &mut StereoBlock, context: &ProcessContext) {
        assert_eq!(context.sample_rate, SAMPLE_RATE);
        self.largest_block
            .fetch_max(block.frame_count(), Ordering::Relaxed);
        let (left, right) = block.channels_mut();
        for sample in left.iter_mut().chain(right.iter_mut()) {
            *sample *= self.gain;
        }
    }
}

#[test]
fn deck_and_master_inserts_process_the_audio() {
    let largest_block = Arc::new(AtomicUsize::new(0));
    let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
    controller
        .load_track(DeckId::A, constant_track(0.5, 0.5, 10.0, SAMPLE_RATE))
        .unwrap();
    controller.set_crossfader(0.0).unwrap();
    controller
        .set_deck_insert(
            DeckId::A,
            0,
            Some(Box::new(Gain {
                gain: 0.5,
                largest_block: Arc::clone(&largest_block),
            })),
        )
        .unwrap();
    controller
        .set_master_insert(
            3,
            Some(Box::new(Gain {
                gain: 0.25,
                largest_block: Arc::clone(&largest_block),
            })),
        )
        .unwrap();
    controller.play(DeckId::A).unwrap();
    let output = render_frames(&mut renderer, 4096, 2048);
    assert_close(f64::from(output[output.len() - 1]), 0.5 * 0.5 * 0.25, 1e-6);
    assert_eq!(largest_block.load(Ordering::Relaxed), MAX_BLOCK_FRAMES);

    controller.set_deck_insert(DeckId::A, 0, None).unwrap();
    let output = render_frames(&mut renderer, BLOCK * 2, BLOCK);
    assert_close(f64::from(output[output.len() - 1]), 0.5 * 0.25, 1e-6);
}

struct DropProbe {
    dropped_on: Arc<std::sync::Mutex<Option<std::thread::ThreadId>>>,
}

impl AudioProcessor for DropProbe {
    fn process(&mut self, _block: &mut StereoBlock, _context: &ProcessContext) {}
}

impl Drop for DropProbe {
    fn drop(&mut self) {
        *self.dropped_on.lock().expect("probe lock") = Some(std::thread::current().id());
    }
}

#[test]
fn replaced_processors_are_dropped_on_the_control_thread() {
    let dropped_on = Arc::new(std::sync::Mutex::new(None));
    let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
    controller
        .set_master_insert(
            0,
            Some(Box::new(DropProbe {
                dropped_on: Arc::clone(&dropped_on),
            })),
        )
        .unwrap();
    controller.set_master_insert(0, None).unwrap();
    let audio_thread = std::thread::spawn(move || {
        render_frames(&mut renderer, BLOCK, BLOCK);
        (std::thread::current().id(), renderer)
    });
    let (audio_thread_id, _renderer) = audio_thread.join().unwrap();
    assert_eq!(*dropped_on.lock().unwrap(), None);
    controller.collect_garbage();
    let dropped_thread = dropped_on.lock().unwrap().unwrap();
    assert_eq!(dropped_thread, std::thread::current().id());
    assert_ne!(dropped_thread, audio_thread_id);
}

#[test]
fn loading_a_new_track_resets_the_deck() {
    let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
    controller
        .load_track(DeckId::A, constant_track(0.5, 0.5, 10.0, SAMPLE_RATE))
        .unwrap();
    controller.set_crossfader(0.0).unwrap();
    controller.play(DeckId::A).unwrap();
    render_frames(&mut renderer, SAMPLE_RATE as usize, BLOCK);
    controller
        .load_track(DeckId::A, constant_track(0.25, 0.25, 5.0, SAMPLE_RATE))
        .unwrap();
    let output = left_channel(&render_frames(&mut renderer, BLOCK, BLOCK));
    assert!(largest_step(&output) <= 0.5 / DECLICK_FRAMES as f32 + 1e-6);
    let state = controller.deck_state(DeckId::A);
    assert!(state.loaded);
    assert!(!state.playing);
    assert_close(state.position_seconds, 0.0, 0.0);
    controller.eject(DeckId::A).unwrap();
    render_frames(&mut renderer, BLOCK, BLOCK);
    assert!(!controller.deck_state(DeckId::A).loaded);
    assert!(controller.track(DeckId::A).is_none());
}

#[test]
fn the_mix_goes_to_the_first_two_channels_or_is_summed_to_mono() {
    let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
    controller
        .load_track(DeckId::A, constant_track(0.5, 0.25, 10.0, SAMPLE_RATE))
        .unwrap();
    controller.set_crossfader(0.0).unwrap();
    controller.play(DeckId::A).unwrap();
    render_frames(&mut renderer, BLOCK, BLOCK);

    let mut four_channels = vec![1.0; BLOCK * 4];
    renderer.render(&mut four_channels, 4);
    assert_eq!(
        &four_channels[four_channels.len() - 4..],
        &[0.5, 0.25, 0.0, 0.0]
    );

    let mut mono = vec![1.0; BLOCK];
    renderer.render(&mut mono, 1);
    assert_close(f64::from(mono[BLOCK - 1]), 0.375, 1e-6);
    assert_eq!(right_channel(&[0.0, 1.0]), vec![1.0]);
}

#[test]
fn both_sides_can_move_to_their_own_threads() {
    fn assert_send<T: Send>() {}
    assert_send::<dubplate_engine::Controller>();
    assert_send::<dubplate_engine::Renderer>();
    assert_send::<dubplate_engine::Track>();
}

#[test]
fn out_of_range_parameters_are_rejected() {
    let (mut controller, _renderer) = offline_engine(SAMPLE_RATE);
    assert!(matches!(
        controller.set_tempo(DeckId::A, 0.0),
        Err(EngineError::InvalidParameter { name: "tempo", .. })
    ));
    assert!(controller.set_tempo(DeckId::A, f64::NAN).is_err());
    assert!(controller.set_crossfader(1.5).is_err());
    assert!(controller.set_channel_fader(DeckId::B, -0.1).is_err());
    assert!(controller.seek(DeckId::A, -1.0).is_err());
    assert!(matches!(
        controller.set_master_insert(99, None),
        Err(EngineError::InvalidInsertSlot { slot: 99, .. })
    ));
}

#[test]
fn a_full_command_queue_is_reported() {
    let (mut controller, _renderer) = offline_engine(SAMPLE_RATE);
    let mut result = Ok(());
    for _ in 0..10_000 {
        result = controller.set_crossfader(0.5);
        if result.is_err() {
            break;
        }
    }
    assert!(matches!(result, Err(EngineError::CommandQueueFull)));
}
