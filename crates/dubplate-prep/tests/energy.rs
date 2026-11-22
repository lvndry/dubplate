//! Energy measurement, the energy cache and the background energy queue.

use std::fs;
use std::path::Path;

use dubplate_prep::{EnergyCache, EnergyQueue, Priority, measure_track_energy};

const SAMPLE_RATE: u32 = 44_100;
const SECONDS: f32 = 20.0;
const BEATS_PER_MINUTE: f32 = 122.0;

/// The samples of a 20 second kick-like pattern at 122 BPM plus noise.
fn kick_samples() -> Vec<f32> {
    let sample_count = (SECONDS * SAMPLE_RATE as f32) as usize;
    let beat_samples = SAMPLE_RATE as f32 * 60.0 / BEATS_PER_MINUTE;
    (0..sample_count)
        .map(|index| {
            let phase = (index as f32 % beat_samples) / beat_samples;
            let kick = 0.8
                * (2.0 * std::f32::consts::PI * 55.0 * (index as f32) / SAMPLE_RATE as f32).sin()
                * (-phase * 12.0).exp();
            let noise = 0.02 * ((index as f64 * 0.618033988749895) % 1.0 - 0.5);
            kick + noise as f32
        })
        .collect()
}

/// A 20 second silence.
fn silent_samples() -> Vec<f32> {
    vec![0.0; (SECONDS * SAMPLE_RATE as f32) as usize]
}

/// Writes a 16-bit stereo WAV from samples in -1.0 to 1.0, interleaved left and right.
fn write_wav(path: &Path, samples: &[f32]) {
    let data_length = u32::try_from(samples.len()).expect("WAV fits in 4 GB") * 2;
    let mut bytes = Vec::with_capacity(44 + data_length as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_length).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    bytes.extend_from_slice(&(SAMPLE_RATE * 4_u32).to_le_bytes());
    bytes.extend_from_slice(&4_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_length.to_le_bytes());
    for sample in samples {
        let left = (sample.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        let right = ((sample * 0.9).clamp(-1.0, 1.0) * 32767.0).round() as i16;
        bytes.extend_from_slice(&left.to_le_bytes());
        bytes.extend_from_slice(&right.to_le_bytes());
    }
    fs::write(path, bytes).expect("write WAV file");
}

fn temp_dir() -> tempfile::TempDir {
    tempfile::tempdir().expect("temp directory")
}

#[test]
fn a_kick_pattern_rates_between_one_and_ten_with_a_curve() {
    let directory = temp_dir();
    let path = directory.path().join("kick.wav");
    write_wav(&path, &kick_samples());

    let energy = measure_track_energy(&path, Some(BEATS_PER_MINUTE))
        .expect("the kick track rates")
        .expect("the kick track is not silence");
    assert!(
        (1..=10).contains(&energy.rating),
        "rating {} out of scale",
        energy.rating
    );
    assert!(!energy.curve.is_empty(), "the curve is empty");
    for window in &energy.curve {
        assert!(window.rating.is_none() || (1..=10).contains(&window.rating.unwrap_or(0)));
    }
}

#[test]
fn silence_rates_as_nothing() {
    let directory = temp_dir();
    let path = directory.path().join("silence.wav");
    write_wav(&path, &silent_samples());

    let energy = measure_track_energy(&path, Some(BEATS_PER_MINUTE)).expect("silence is rated");
    assert!(energy.is_none(), "silence should have no energy");
}

#[test]
fn the_cache_returns_what_was_put() {
    let directory = temp_dir();
    let path = directory.path().join("track.wav");
    write_wav(&path, &kick_samples());

    let energy = measure_track_energy(&path, Some(BEATS_PER_MINUTE))
        .expect("the track rates")
        .expect("the track is not silence");
    let mut cache = EnergyCache::open(&directory.path().join("cache.db")).expect("the cache opens");
    cache.put(&path, &energy).expect("the rating is stored");

    let cached = cache
        .get(&path)
        .expect("the cached rating is read")
        .expect("the cached rating is fresh");
    assert_eq!(cached, energy);
}

#[test]
fn the_cache_is_stale_after_the_file_changes() {
    let directory = temp_dir();
    let path = directory.path().join("track.wav");
    write_wav(&path, &kick_samples());

    let energy = measure_track_energy(&path, Some(BEATS_PER_MINUTE))
        .expect("the track rates")
        .expect("the track is not silence");
    let mut cache = EnergyCache::open(&directory.path().join("cache.db")).expect("the cache opens");
    cache.put(&path, &energy).expect("the rating is stored");

    write_wav(&path, &silent_samples());
    let cached = cache
        .get(&path)
        .expect("the cached rating is read after the change");
    assert!(cached.is_none(), "the rating of the changed file is stale");
}

#[test]
fn the_cache_refuses_a_path_under_a_rekordbox_folder() {
    let directory = temp_dir();
    let result = EnergyCache::open(&directory.path().join("rekordbox").join("cache.db"));
    assert!(
        matches!(
            result,
            Err(dubplate_prep::EnergyCacheError::InsideRekordbox(_))
        ),
        "a rekordbox path is refused"
    );
}

#[test]
fn dropping_the_queue_returns_promptly_with_tracks_still_waiting() {
    let directory = temp_dir();
    let paths: Vec<_> = (0..5)
        .map(|index| {
            let path = directory.path().join(format!("waiting-{index}.wav"));
            write_wav(&path, &kick_samples());
            path
        })
        .collect();
    let queue = EnergyQueue::start(directory.path().join("cache.db")).expect("the queue starts");
    queue.request_all(
        paths
            .iter()
            .map(|path| (path.clone(), Some(BEATS_PER_MINUTE), Priority::Library)),
    );
    let started = std::time::Instant::now();
    drop(queue);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "dropping waited for the whole queue"
    );
}

#[test]
fn the_queue_answers_set_tracks_before_library_tracks_and_reads_the_cache() {
    let directory = temp_dir();
    let library_path = directory.path().join("library.wav");
    let set_path = directory.path().join("set.wav");
    let shortlist_path = directory.path().join("shortlist.wav");
    write_wav(&library_path, &kick_samples());
    write_wav(&set_path, &kick_samples());
    write_wav(&shortlist_path, &kick_samples());

    let queue = EnergyQueue::start(directory.path().join("cache.db")).expect("the queue starts");
    queue.request_all([
        (
            library_path.clone(),
            Some(BEATS_PER_MINUTE),
            Priority::Library,
        ),
        (
            shortlist_path.clone(),
            Some(BEATS_PER_MINUTE),
            Priority::Shortlist,
        ),
        (set_path.clone(), Some(BEATS_PER_MINUTE), Priority::Set),
    ]);

    let first = queue.results().recv().expect("the first answer arrives");
    assert_eq!(first.path, set_path, "the set track is answered first");
    let second = queue.results().recv().expect("the second answer arrives");
    assert_eq!(second.path, shortlist_path, "the shortlist track is second");
    let third = queue.results().recv().expect("the third answer arrives");
    assert_eq!(third.path, library_path, "the library track is last");
    for result in [&first, &second, &third] {
        let energy = result.outcome.as_ref().expect("the track is rated");
        assert!(energy.is_some(), "the track is not silence");
    }
    let decodes = queue.decoded_count();
    assert_eq!(decodes, 3, "each track was decoded once");

    queue.request(set_path.clone(), Some(BEATS_PER_MINUTE), Priority::Set);
    let again = queue.results().recv().expect("the cached answer arrives");
    assert_eq!(again.path, set_path);
    let cached_again = again.outcome.as_ref().expect("the cached track is rated");
    assert!(cached_again.is_some(), "the cached track is not silence");
    assert_eq!(
        queue.decoded_count(),
        decodes,
        "the cached track was answered without decoding"
    );
}
