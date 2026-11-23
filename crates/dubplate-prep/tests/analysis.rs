//! Dubplate's track analysis, its cache and the background analysis queue, on synthetic audio.

use std::f32::consts::TAU;
use std::fs;
use std::path::Path;

use dubplate_prep::{
    AnalysisCache, AnalysisCacheError, AnalysisQueue, AudioAnalysis, Key, Priority, analyse_track,
};

const SAMPLE_RATE: u32 = 44_100;
const SECONDS: f32 = 30.0;
const AFRO_HOUSE_BPM: f32 = 122.0;
const AMAPIANO_BPM: f32 = 113.0;
const FIRST_CLICK_SECONDS: f32 = 0.37;
const BEATS_PER_BAR: usize = 4;
/// How far a grid beat may sit from the click it marks. Euphoria puts a beat where a listener
/// taps, a little after the onset, so beats land up to about 20 ms after the clicks.
const BEAT_TOLERANCE_SECONDS: f64 = 0.02;
/// The notes of an A minor chord, in hertz: A3, C4 and E4.
const A_MINOR_CHORD: [f32; 3] = [220.0, 261.63, 329.63];

/// A click track at `bpm`: a short, bright click on every beat from [`FIRST_CLICK_SECONDS`],
/// louder and lower on the first beat of each bar, plus a little noise, and `chord` held
/// quietly under it.
fn click_track(bpm: f32, chord: &[f32]) -> Vec<f32> {
    let sample_count = (SECONDS * SAMPLE_RATE as f32) as usize;
    let beat_seconds = 60.0 / bpm;
    (0..sample_count)
        .map(|index| {
            let seconds = index as f32 / SAMPLE_RATE as f32;
            let since_first = seconds - FIRST_CLICK_SECONDS;
            let click = if since_first < 0.0 {
                0.0
            } else {
                let beat = (since_first / beat_seconds).floor();
                let since_beat = since_first - beat * beat_seconds;
                let downbeat = (beat as usize).is_multiple_of(BEATS_PER_BAR);
                let (level, pitch) = if downbeat {
                    (0.9, 1_000.0)
                } else {
                    (0.5, 2_000.0)
                };
                level * (TAU * pitch * since_beat).sin() * (-since_beat * 120.0).exp()
            };
            let held: f32 = chord
                .iter()
                .map(|frequency| 0.08 * (TAU * frequency * seconds).sin())
                .sum();
            let noise = 0.01 * ((index as f64 * 0.618_033_988_749_895) % 1.0 - 0.5) as f32;
            click + held + noise
        })
        .collect()
}

/// A 30 second silence.
fn silent_samples() -> Vec<f32> {
    vec![0.0; (SECONDS * SAMPLE_RATE as f32) as usize]
}

/// Writes a 16-bit stereo WAV from samples in -1.0 to 1.0, interleaved left and right.
fn write_wav(path: &Path, samples: &[f32]) {
    let data_length = u32::try_from(samples.len()).expect("WAV fits in 4 GB") * 4;
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

fn analysed_click_track(bpm: f32, chord: &[f32]) -> AudioAnalysis {
    let directory = temp_dir();
    let path = directory.path().join("clicks.wav");
    write_wav(&path, &click_track(bpm, chord));
    analyse_track(&path).expect("the click track is analysed")
}

/// Checks that the analysis found `bpm` and put a beat on every click and a downbeat on every
/// first click of a bar.
fn assert_beats_on_the_clicks(analysis: &AudioAnalysis, bpm: f32) {
    let found = analysis.bpm.expect("the click track has a tempo");
    assert!((found - bpm).abs() < 0.05, "tempo {found}, clicks at {bpm}");
    let grid = analysis
        .rekordbox_grid()
        .expect("the click track has a beat grid");
    let beat_seconds = 60.0 / f64::from(bpm);
    let clicks = ((f64::from(SECONDS) - f64::from(FIRST_CLICK_SECONDS)) / beat_seconds) as usize;
    for click in 0..clicks {
        let click_seconds = f64::from(FIRST_CLICK_SECONDS) + click as f64 * beat_seconds;
        let nearest = grid
            .beats
            .iter()
            .min_by(|first, second| {
                let distance = |beat: &&dubplate_rekordbox::analysis::Beat| {
                    (f64::from(beat.time_ms) / 1000.0 - click_seconds).abs()
                };
                distance(first).total_cmp(&distance(second))
            })
            .expect("the grid has beats");
        let off = f64::from(nearest.time_ms) / 1000.0 - click_seconds;
        assert!(
            off.abs() < BEAT_TOLERANCE_SECONDS,
            "click {click} at {click_seconds:.3}s has its nearest beat {off:+.3}s away"
        );
        assert_eq!(
            nearest.is_downbeat(),
            click.is_multiple_of(BEATS_PER_BAR),
            "click {click} at {click_seconds:.3}s"
        );
    }
}

#[test]
fn an_afro_house_click_track_gets_its_tempo_and_a_beat_on_every_click() {
    let analysis = analysed_click_track(AFRO_HOUSE_BPM, &[]);
    assert_beats_on_the_clicks(&analysis, AFRO_HOUSE_BPM);
    assert!((analysis.duration_seconds - SECONDS).abs() < 0.01);
}

#[test]
fn an_amapiano_click_track_gets_its_tempo_and_a_beat_on_every_click() {
    let analysis = analysed_click_track(AMAPIANO_BPM, &[]);
    assert_beats_on_the_clicks(&analysis, AMAPIANO_BPM);
}

#[test]
fn a_held_a_minor_chord_reads_as_eight_a() {
    let analysis = analysed_click_track(AFRO_HOUSE_BPM, &A_MINOR_CHORD);
    let key = analysis.key.expect("the chord has a key");
    assert_eq!(key, Key::parse_file_key("8A").expect("8A is a key"));
    assert_eq!(key.camelot(), "8A");
    assert!(analysis.energy.is_some(), "the track has energy");
}

#[test]
fn silence_has_no_tempo_grid_key_energy_or_loudness() {
    let directory = temp_dir();
    let path = directory.path().join("silence.wav");
    write_wav(&path, &silent_samples());
    let analysis = analyse_track(&path).expect("silence is analysed");
    assert_eq!(analysis.bpm, None);
    assert_eq!(analysis.beat_grid, None);
    assert_eq!(analysis.key, None);
    assert_eq!(analysis.energy, None);
    assert_eq!(analysis.loudness, None);
}

#[test]
fn a_file_that_is_not_audio_fails_to_decode() {
    let directory = temp_dir();
    let path = directory.path().join("notes.wav");
    fs::write(&path, "not audio").expect("write the file");
    assert!(matches!(
        analyse_track(&path),
        Err(dubplate_prep::AnalysisError::Decode { .. })
    ));
}

#[test]
fn the_cache_returns_what_was_put() {
    let directory = temp_dir();
    let path = directory.path().join("track.wav");
    write_wav(&path, &click_track(AFRO_HOUSE_BPM, &A_MINOR_CHORD));
    let analysis = analyse_track(&path).expect("the track is analysed");
    let mut cache =
        AnalysisCache::open(&directory.path().join("cache.db")).expect("the cache opens");
    cache.put(&path, &analysis).expect("the analysis is stored");

    let cached = cache
        .get(&path)
        .expect("the cached analysis is read")
        .expect("the cached analysis is fresh");
    assert_eq!(cached.bpm, analysis.bpm);
    assert_eq!(cached.beat_grid, analysis.beat_grid);
    assert_eq!(cached.key, analysis.key);
    assert_eq!(cached.energy, analysis.energy);
    assert!(analysis.loudness.is_some(), "the clicks have a loudness");
    assert_eq!(cached.loudness, analysis.loudness);
    assert!((cached.duration_seconds - analysis.duration_seconds).abs() < 1e-3);
}

#[test]
fn the_cache_keeps_an_analysis_with_nothing_found() {
    let directory = temp_dir();
    let path = directory.path().join("silence.wav");
    write_wav(&path, &silent_samples());
    let analysis = analyse_track(&path).expect("silence is analysed");
    let mut cache =
        AnalysisCache::open(&directory.path().join("cache.db")).expect("the cache opens");
    cache.put(&path, &analysis).expect("the analysis is stored");
    let cached = cache.get(&path).expect("the cache is read");
    assert_eq!(cached.map(|cached| cached.bpm), Some(None));
}

#[test]
fn the_cache_is_stale_after_the_file_changes() {
    let directory = temp_dir();
    let path = directory.path().join("track.wav");
    write_wav(&path, &click_track(AFRO_HOUSE_BPM, &[]));
    let analysis = analyse_track(&path).expect("the track is analysed");
    let mut cache =
        AnalysisCache::open(&directory.path().join("cache.db")).expect("the cache opens");
    cache.put(&path, &analysis).expect("the analysis is stored");

    write_wav(&path, &silent_samples()[..1_000]);
    let cached = cache
        .get(&path)
        .expect("the cache is read after the change");
    assert!(
        cached.is_none(),
        "the analysis of the changed file is stale"
    );
}

#[test]
fn the_cache_is_stale_under_another_analysis_version() {
    let directory = temp_dir();
    let path = directory.path().join("track.wav");
    let cache_path = directory.path().join("cache.db");
    write_wav(&path, &click_track(AFRO_HOUSE_BPM, &[]));
    let analysis = analyse_track(&path).expect("the track is analysed");
    let mut cache = AnalysisCache::open(&cache_path).expect("the cache opens");
    cache.put(&path, &analysis).expect("the analysis is stored");
    drop(cache);

    let connection = rusqlite::Connection::open(&cache_path).expect("the cache file opens");
    connection
        .execute("UPDATE analysis SET analysis_version = 'older'", [])
        .expect("the version is changed");
    drop(connection);

    let cache = AnalysisCache::open(&cache_path).expect("the cache opens again");
    let cached = cache.get(&path).expect("the cache is read");
    assert!(
        cached.is_none(),
        "an analysis from another version is stale"
    );
}

#[test]
fn a_forgotten_track_is_not_answered() {
    let directory = temp_dir();
    let path = directory.path().join("track.wav");
    write_wav(&path, &silent_samples());
    let analysis = analyse_track(&path).expect("silence is analysed");
    let mut cache =
        AnalysisCache::open(&directory.path().join("cache.db")).expect("the cache opens");
    cache.put(&path, &analysis).expect("the analysis is stored");
    cache.forget(&path).expect("the track is forgotten");
    assert!(cache.get(&path).expect("the cache is read").is_none());
}

#[test]
fn the_cache_refuses_a_path_under_a_rekordbox_folder() {
    let directory = temp_dir();
    let result = AnalysisCache::open(&directory.path().join("rekordbox").join("cache.db"));
    assert!(
        matches!(result, Err(AnalysisCacheError::InsideRekordbox(_))),
        "a rekordbox path is refused"
    );
}

#[test]
fn dropping_the_queue_returns_promptly_with_tracks_still_waiting() {
    let directory = temp_dir();
    let paths: Vec<_> = (0..5)
        .map(|index| {
            let path = directory.path().join(format!("waiting-{index}.wav"));
            write_wav(&path, &click_track(AFRO_HOUSE_BPM, &[]));
            path
        })
        .collect();
    let queue = AnalysisQueue::start(directory.path().join("cache.db")).expect("the queue starts");
    queue.request_all(paths.iter().map(|path| (path.clone(), Priority::Library)));
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
    for path in [&library_path, &set_path, &shortlist_path] {
        write_wav(path, &click_track(AFRO_HOUSE_BPM, &[]));
    }

    let queue = AnalysisQueue::start(directory.path().join("cache.db")).expect("the queue starts");
    queue.request_all([
        (library_path.clone(), Priority::Library),
        (shortlist_path.clone(), Priority::Shortlist),
        (set_path.clone(), Priority::Set),
    ]);

    let first = queue.results().recv().expect("the first answer arrives");
    assert_eq!(first.path, set_path, "the set track is answered first");
    let second = queue.results().recv().expect("the second answer arrives");
    assert_eq!(second.path, shortlist_path, "the shortlist track is second");
    let third = queue.results().recv().expect("the third answer arrives");
    assert_eq!(third.path, library_path, "the library track is last");
    for result in [&first, &second, &third] {
        let analysis = result.outcome.as_ref().expect("the track is analysed");
        assert!(analysis.bpm.is_some(), "the track has a tempo");
        assert!(analysis.energy.is_some(), "the track has energy");
    }
    let decodes = queue.decoded_count();
    assert_eq!(decodes, 3, "each track was decoded once");

    queue.request(set_path.clone(), Priority::Set);
    let again = queue.results().recv().expect("the cached answer arrives");
    assert_eq!(again.path, set_path);
    assert_eq!(
        again
            .outcome
            .as_ref()
            .ok()
            .and_then(|analysis| analysis.bpm),
        first
            .outcome
            .as_ref()
            .ok()
            .and_then(|analysis| analysis.bpm)
    );
    assert_eq!(
        queue.decoded_count(),
        decodes,
        "the cached track was answered without decoding"
    );
}

#[test]
fn a_set_track_asked_for_during_the_library_is_analysed_next() {
    let directory = temp_dir();
    let library_paths: Vec<_> = (0..3)
        .map(|index| {
            let path = directory.path().join(format!("library-{index}.wav"));
            write_wav(&path, &click_track(AFRO_HOUSE_BPM, &[]));
            path
        })
        .collect();
    let set_path = directory.path().join("set.wav");
    write_wav(&set_path, &click_track(AMAPIANO_BPM, &[]));

    let queue = AnalysisQueue::start(directory.path().join("cache.db")).expect("the queue starts");
    queue.request_all(
        library_paths
            .iter()
            .map(|path| (path.clone(), Priority::Library)),
    );
    let first = queue
        .results()
        .recv()
        .expect("the first library track arrives");
    assert_eq!(first.path, library_paths[0]);
    queue.request(set_path.clone(), Priority::Set);
    let answered: Vec<_> = (0..3)
        .map(|_| queue.results().recv().expect("an answer arrives").path)
        .collect();
    let set_position = answered
        .iter()
        .position(|path| *path == set_path)
        .expect("the set track is answered");
    assert!(
        set_position <= 1,
        "the set track waits at most for the track in progress: {answered:?}"
    );
}

#[test]
fn an_unreadable_track_is_answered_with_why() {
    let directory = temp_dir();
    let queue = AnalysisQueue::start(directory.path().join("cache.db")).expect("the queue starts");
    let missing = directory.path().join("missing.wav");
    queue.request(missing.clone(), Priority::Set);
    let answer = queue.results().recv().expect("the answer arrives");
    assert_eq!(answer.path, missing);
    assert!(answer.outcome.is_err());
    assert_eq!(queue.decoded_count(), 0);
}

#[test]
fn a_cache_from_before_loudness_opens_and_keeps_loudness_from_then_on() {
    let directory = temp_dir();
    let cache_path = directory.path().join("cache.db");
    {
        let connection = rusqlite::Connection::open(&cache_path).expect("the old cache opens");
        connection
            .execute_batch(
                "CREATE TABLE analysis (track_path TEXT PRIMARY KEY, file_size INTEGER NOT NULL,
                    modified_ms INTEGER NOT NULL, analysis_version TEXT NOT NULL,
                    duration_seconds REAL NOT NULL, bpm REAL, grid_bpm REAL,
                    grid_first_downbeat_seconds REAL, camelot_key TEXT, energy_rating INTEGER,
                    energy_score REAL, energy_curve_json TEXT, analysed_at INTEGER NOT NULL);
                 PRAGMA user_version = 1;",
            )
            .expect("the old schema is written");
    }
    let path = directory.path().join("track.wav");
    write_wav(&path, &click_track(AFRO_HOUSE_BPM, &[]));
    let analysis = analyse_track(&path).expect("the track is analysed");
    let mut cache = AnalysisCache::open(&cache_path).expect("the old cache upgrades");
    cache.put(&path, &analysis).expect("the analysis is stored");
    let cached = cache
        .get(&path)
        .expect("the cached analysis is read")
        .expect("the cached analysis is fresh");
    assert_eq!(cached.loudness, analysis.loudness);
}
