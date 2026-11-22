//! The separator and the engine's key lock live in one executable, since the app runs itself as
//! the separation worker. Both must work side by side there: two copies of a C++ library with
//! the same names in one executable would let one call the other's code and crash.

use std::path::Path;

use dubplate_engine::{DeckId, Stem, StemSettings, StemTracks, Track, offline_engine};
use dubplate_stems::{CacheLimit, StemsLocation, separate_in_process};

const SAMPLE_RATE: u32 = 48_000;

#[test]
fn key_lock_plays_stems_in_an_executable_that_also_separates() {
    let folder = tempfile::tempdir().unwrap();
    let weights = folder.path().join("weights.safetensors");
    std::fs::write(&weights, b"not loaded").unwrap();
    let location = StemsLocation::new(
        folder.path().join("stems"),
        weights,
        CacheLimit::from_bytes(1_000_000),
    );
    let missing = separate_in_process(&location, Path::new("/nonexistent/track.m4a"), &|_| {});
    assert!(missing.is_err());

    let seconds = 2;
    let tone: Vec<f32> = (0..SAMPLE_RATE * seconds)
        .flat_map(|index| {
            let value = (index as f32 * 0.05).sin() * 0.2;
            [value, value]
        })
        .collect();
    let track = Track::from_interleaved_stereo(&tone, SAMPLE_RATE).unwrap();
    let stem: Vec<i16> = tone
        .iter()
        .map(|sample| (sample * 8_000.0) as i16)
        .collect();
    track
        .attach_stems(
            StemTracks::from_interleaved_stereo([&stem; Stem::COUNT], SAMPLE_RATE).unwrap(),
        )
        .unwrap();
    let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
    controller.load_track(DeckId::A, track).unwrap();
    controller.set_key_lock(DeckId::A, true).unwrap();
    controller.set_tempo(DeckId::A, 1.05).unwrap();
    controller
        .set_stems(DeckId::A, StemSettings::UNTOUCHED.without(Stem::Vocals))
        .unwrap();
    controller.play(DeckId::A).unwrap();
    let mut output = vec![0.0; 256 * 2];
    let mut loudest: f32 = 0.0;
    for _ in 0..200 {
        renderer.render(&mut output, 2);
        loudest = output
            .iter()
            .fold(loudest, |peak, sample| peak.max(sample.abs()));
    }
    assert!(loudest > 0.01, "the deck stayed silent");
}
