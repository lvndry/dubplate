//! Where a beat loop starts when it is pressed mid-beat and mid-bar: on the beat nearest to
//! what the DJ hears with quantize on, exactly there with it off, as rekordbox does.

mod common;

use common::{SAMPLE_RATE, assert_close, left_channel, mixer_latency_frames, sine_track};
use dubplate_engine::cpal::{OutputCallbackInfo, OutputStreamTimestamp, StreamInstant};
use dubplate_engine::{
    BeatGrid, BeatLength, Controller, DeckId, GridBeat, Renderer, offline_engine,
};

const BLOCK: usize = 256;
const TRACK_SECONDS: f64 = 60.0;

fn deck_with_grid(grid: BeatGrid) -> (Controller, Renderer) {
    let (mut controller, renderer) = offline_engine(SAMPLE_RATE);
    controller
        .load_track(
            DeckId::A,
            sine_track(440.0, 0.5, 0.5, TRACK_SECONDS, SAMPLE_RATE),
        )
        .expect("load the test track");
    controller
        .set_beat_grid(DeckId::A, Some(grid))
        .expect("set the grid");
    controller.set_crossfader(0.0).expect("set the crossfader");
    (controller, renderer)
}

fn steady_grid() -> BeatGrid {
    BeatGrid::new(120.0, 0.0).expect("grid")
}

fn render(renderer: &mut Renderer, frames: usize) {
    let mut rendered = 0;
    while rendered < frames {
        let block = BLOCK.min(frames - rendered);
        let mut output = vec![0.0_f32; block * 2];
        renderer.render(&mut output, 2);
        rendered += block;
    }
}

fn position(controller: &Controller) -> f64 {
    controller.deck_state(DeckId::A).position_seconds
}

struct Press {
    position: f64,
    start: f64,
    end: f64,
}

fn press_beat_loop(
    controller: &mut Controller,
    renderer: &mut Renderer,
    seconds: f64,
    length: BeatLength,
) -> Press {
    controller
        .seek(DeckId::A, seconds)
        .expect("send the command");
    controller.play(DeckId::A).expect("send the command");
    render(renderer, BLOCK * 4);
    let pressed_at = position(controller);
    controller
        .beat_loop(DeckId::A, length)
        .expect("send the command");
    render(renderer, BLOCK);
    let state = controller.loop_state(DeckId::A);
    Press {
        position: pressed_at,
        start: state.start_seconds.expect("the loop has a start"),
        end: state.end_seconds.expect("the loop has an end"),
    }
}

fn press_on(grid: BeatGrid, seconds: f64, quantize: bool) -> Press {
    let (mut controller, mut renderer) = deck_with_grid(grid);
    controller.set_quantize(quantize).expect("send the command");
    press_beat_loop(&mut controller, &mut renderer, seconds, BeatLength::FOUR)
}

fn pressed_before(seconds: f64) -> f64 {
    seconds - (BLOCK * 4) as f64 / f64::from(SAMPLE_RATE)
}

#[test]
fn mid_beat_with_quantize_the_loop_starts_on_the_nearest_beat_behind_the_playhead() {
    let press = press_on(steady_grid(), pressed_before(10.2), true);
    assert_close(press.start, 10.0, 1e-9);
    assert_close(press.end, 12.0, 1e-9);
    assert!(press.start < press.position);
}

#[test]
fn mid_beat_with_quantize_the_loop_starts_on_the_nearest_beat_ahead_of_the_playhead() {
    let press = press_on(steady_grid(), pressed_before(10.4), true);
    assert_close(press.start, 10.5, 1e-9);
    assert_close(press.end, 12.5, 1e-9);
    assert!(press.start > press.position);
}

#[test]
fn mid_bar_the_loop_snaps_to_a_beat_not_to_the_bar() {
    let late_in_the_bar = press_on(steady_grid(), pressed_before(11.4), true);
    assert_close(late_in_the_bar.start, 11.5, 1e-9);
    let early_in_the_bar = press_on(steady_grid(), pressed_before(10.6), true);
    assert_close(early_in_the_bar.start, 10.5, 1e-9);
    let across_the_bar = press_on(steady_grid(), pressed_before(11.8), true);
    assert_close(across_the_bar.start, 12.0, 1e-9);
}

#[test]
fn without_quantize_the_loop_starts_exactly_at_the_press() {
    for seconds in [10.2, 10.4, 11.8] {
        let press = press_on(steady_grid(), pressed_before(seconds), false);
        assert_close(press.start, press.position, 1e-9);
        assert_close(press.end, press.position + 2.0, 1e-9);
    }
}

#[test]
fn a_fractional_beat_loop_snaps_to_its_own_length_up_to_a_beat() {
    let (mut controller, mut renderer) = deck_with_grid(steady_grid());
    let press = press_beat_loop(
        &mut controller,
        &mut renderer,
        pressed_before(10.4),
        BeatLength::ONE,
    );
    assert_close(press.start, 10.5, 1e-9);
    assert_close(press.end, 11.0, 1e-9);
}

#[test]
fn on_a_grid_that_changes_tempo_the_loop_starts_on_the_nearest_beat_of_that_grid() {
    let beats = [
        GridBeat {
            seconds: 0.0,
            bpm: 120.0,
        },
        GridBeat {
            seconds: 0.5,
            bpm: 120.0,
        },
        GridBeat {
            seconds: 1.0,
            bpm: 100.0,
        },
        GridBeat {
            seconds: 1.6,
            bpm: 100.0,
        },
    ];
    let grid = BeatGrid::from_beats(&beats).unwrap();
    let nearer_the_next_beat = press_on(grid.clone(), pressed_before(1.95), true);
    assert_close(nearer_the_next_beat.start, 2.2, 1e-9);
    assert_close(nearer_the_next_beat.end, 2.2 + 4.0 * 0.6, 1e-9);
    let nearer_the_last_beat = press_on(grid, pressed_before(1.75), true);
    assert_close(nearer_the_last_beat.start, 1.6, 1e-9);
    assert_close(nearer_the_last_beat.end, 1.6 + 4.0 * 0.6, 1e-9);
}

#[test]
fn a_loop_follows_the_grid_the_dj_fixed_not_the_one_analysed() {
    let (mut controller, mut renderer) = deck_with_grid(BeatGrid::new(120.0, 0.0).unwrap());
    let analysed = press_beat_loop(
        &mut controller,
        &mut renderer,
        pressed_before(10.2),
        BeatLength::FOUR,
    );
    assert_close(analysed.start, 10.0, 1e-9);

    controller.reloop_or_exit(DeckId::A).unwrap();
    render(&mut renderer, BLOCK);
    let fixed_beats: Vec<GridBeat> = (0..8)
        .map(|index| GridBeat {
            seconds: 0.13 + f64::from(index) * 0.5,
            bpm: 120.0,
        })
        .collect();
    let fixed = BeatGrid::from_beats_with_downbeat(&fixed_beats, 3).unwrap();
    controller.set_beat_grid(DeckId::A, Some(fixed)).unwrap();
    render(&mut renderer, BLOCK);
    let after_the_fix = press_beat_loop(
        &mut controller,
        &mut renderer,
        pressed_before(10.2),
        BeatLength::FOUR,
    );
    assert_close(after_the_fix.start, 10.13, 1e-9);
    assert_close(after_the_fix.end, 12.13, 1e-9);
}

fn callback_info(callback_nanos: u64, latency_nanos: u64) -> OutputCallbackInfo {
    OutputCallbackInfo::new(OutputStreamTimestamp {
        callback: StreamInstant::from_nanos(callback_nanos),
        playback: StreamInstant::from_nanos(callback_nanos + latency_nanos),
    })
}

type DeviceCallback = Box<dyn FnMut(&mut [f32], &OutputCallbackInfo) + Send>;

struct Device {
    callback: DeviceCallback,
    callbacks: u64,
    latency_nanos: u64,
}

impl Device {
    fn new(renderer: Renderer, latency_nanos: u64) -> Self {
        Self {
            callback: Box::new(renderer.into_device_callback::<f32>(2)),
            callbacks: 0,
            latency_nanos,
        }
    }

    fn run(&mut self, callbacks: u64) {
        let buffer_nanos = BLOCK as u64 * 1_000_000_000 / u64::from(SAMPLE_RATE);
        let mut output = vec![0.0_f32; BLOCK * 2];
        for _ in 0..callbacks {
            let info = callback_info(self.callbacks * buffer_nanos, self.latency_nanos);
            (self.callback)(&mut output, &info);
            self.callbacks += 1;
        }
    }
}

fn press_through_a_device(latency_seconds: f64, quantize: bool) -> (Press, f64) {
    let (mut controller, renderer) = deck_with_grid(steady_grid());
    controller.set_quantize(quantize).expect("send the command");
    let mut device = Device::new(renderer, (latency_seconds * 1e9) as u64);
    controller.seek(DeckId::A, 10.0).expect("send the command");
    controller.play(DeckId::A).expect("send the command");
    device.run(4);
    let mixer_latency = mixer_latency_frames(&controller) as f64 / f64::from(SAMPLE_RATE);
    controller
        .beat_loop(DeckId::A, BeatLength::FOUR)
        .expect("send the command");
    let pressed_at = position(&controller);
    device.run(1);
    let state = controller.loop_state(DeckId::A);
    let press = Press {
        position: pressed_at,
        start: state.start_seconds.expect("the loop has a start"),
        end: state.end_seconds.expect("the loop has an end"),
    };
    (press, mixer_latency)
}

#[test]
fn without_quantize_the_loop_starts_where_the_dj_heard_the_music() {
    let (press, mixer_latency) = press_through_a_device(0.1, false);
    assert_close(press.start, press.position - 0.1 - mixer_latency, 1e-6);
    assert!(press.start < press.position);
}

#[test]
fn with_quantize_the_beat_is_chosen_from_what_the_dj_heard() {
    let (mut controller, renderer) = deck_with_grid(steady_grid());
    let mut device = Device::new(renderer, 100_000_000);
    controller
        .seek(DeckId::A, 10.78 - 4.0 * BLOCK as f64 / 48_000.0)
        .unwrap();
    controller.play(DeckId::A).unwrap();
    device.run(4);
    assert_close(position(&controller), 10.78, 1e-6);
    controller.beat_loop(DeckId::A, BeatLength::FOUR).unwrap();
    device.run(1);
    let state = controller.loop_state(DeckId::A);
    assert_close(state.start_seconds.unwrap(), 10.5, 1e-9);
}

#[test]
fn a_stopped_deck_has_nothing_in_flight_to_account_for() {
    let (mut controller, renderer) = deck_with_grid(steady_grid());
    let mut device = Device::new(renderer, 100_000_000);
    controller.seek(DeckId::A, 10.3).unwrap();
    controller.set_quantize(false).unwrap();
    device.run(1);
    controller.beat_loop(DeckId::A, BeatLength::FOUR).unwrap();
    device.run(1);
    assert_close(
        controller.loop_state(DeckId::A).start_seconds.unwrap(),
        10.3,
        1e-9,
    );
}

#[test]
fn the_loop_leaves_the_music_unbroken_when_it_starts_ahead_of_the_playhead() {
    let (mut controller, mut renderer) = deck_with_grid(steady_grid());
    controller.seek(DeckId::A, 10.4).unwrap();
    controller.play(DeckId::A).unwrap();
    render(&mut renderer, BLOCK);
    controller.beat_loop(DeckId::A, BeatLength::TWO).unwrap();
    let mut output = vec![0.0_f32; 48_000 * 2];
    renderer.render(&mut output, 2);
    let left = left_channel(&output);
    let natural_step = 0.5 * std::f32::consts::TAU * 440.0 / SAMPLE_RATE as f32;
    assert!(common::largest_step(&left) < natural_step * 1.5);
    assert!(position(&controller) > 10.4);
}

#[test]
fn a_loop_roll_still_repeats_the_beat_just_passed() {
    let (mut controller, mut renderer) = deck_with_grid(steady_grid());
    controller.seek(DeckId::A, pressed_before(10.4)).unwrap();
    controller.play(DeckId::A).unwrap();
    render(&mut renderer, BLOCK * 4);
    controller.beat_roll(DeckId::A, BeatLength::ONE).unwrap();
    render(&mut renderer, BLOCK);
    let state = controller.loop_state(DeckId::A);
    assert_close(state.start_seconds.unwrap(), 10.0, 1e-9);
    assert_close(state.end_seconds.unwrap(), 10.5, 1e-9);
}
