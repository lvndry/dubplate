//! The recording tap: the master output exactly as it goes to the speakers.

mod common;

use common::{SAMPLE_RATE, sine_track};
use dubplate_engine::{DeckId, offline_engine};

const BLOCK: usize = 256;

#[test]
fn the_tap_records_exactly_what_goes_to_the_speakers() {
    let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
    controller
        .load_track(DeckId::A, sine_track(440.0, 0.9, 0.7, 10.0, SAMPLE_RATE))
        .unwrap();
    controller.set_master_gain(3.0).unwrap();
    controller.play(DeckId::A).unwrap();
    let mut tap = controller.start_recording(2.0).unwrap();
    assert_eq!(tap.sample_rate(), SAMPLE_RATE);
    let mut speakers = Vec::new();
    let mut output = vec![0.0_f32; BLOCK * 4];
    for _ in 0..100 {
        renderer.render(&mut output, 4);
        speakers.extend(output.chunks(4).flat_map(|frame| [frame[0], frame[1]]));
    }
    assert_eq!(tap.available_frames(), BLOCK * 100);
    let mut recorded = vec![0.0_f32; speakers.len()];
    assert_eq!(tap.read_into(&mut recorded), BLOCK * 100);
    assert_eq!(recorded, speakers);
    assert!(recorded.iter().any(|sample| sample.abs() > 0.9));
    assert_eq!(tap.dropped_frames(), 0);
}

#[test]
fn frames_that_do_not_fit_are_dropped_and_counted() {
    let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
    let tap = controller.start_recording(0.1).unwrap();
    let mut output = vec![0.0_f32; BLOCK * 2];
    for _ in 0..40 {
        renderer.render(&mut output, 2);
    }
    let capacity = (0.1 * f64::from(SAMPLE_RATE)) as usize;
    assert_eq!(tap.available_frames(), capacity);
    assert_eq!(tap.dropped_frames(), (BLOCK * 40 - capacity) as u64);
}

#[test]
fn stopping_the_tap_keeps_what_was_recorded() {
    let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
    let mut tap = controller.start_recording(1.0).unwrap();
    let mut output = vec![0.0_f32; BLOCK * 2];
    renderer.render(&mut output, 2);
    controller.stop_recording().unwrap();
    renderer.render(&mut output, 2);
    renderer.render(&mut output, 2);
    let mut recorded = vec![0.0_f32; BLOCK * 8];
    assert_eq!(tap.read_into(&mut recorded), BLOCK);
    assert!(controller.start_recording(0.0).is_err());
    assert!(controller.start_recording(f64::NAN).is_err());
}
