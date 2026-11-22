//! Set preparation: editing a set in the store, suggesting the next track from a library of
//! 800, and analysing one short track, all on files made in a temporary folder.

use std::collections::HashSet;
use std::f32::consts::TAU;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use criterion::{Criterion, criterion_group, criterion_main};
use dubplate_prep::{
    AnalysisCache, EnergyDirection, Key, PrepTrack, SetEntry, SetList, SetStore, analyse_track,
    suggest_next,
};

const SET_LENGTH: usize = 40;
const LIBRARY_TRACKS: usize = 800;
const SAMPLE_RATE: u32 = 44_100;
const ANALYSED_SECONDS: f32 = 20.0;
const CLICK_BPM: f32 = 124.0;

fn entry(index: usize) -> SetEntry {
    SetEntry {
        track_path: PathBuf::from(format!(
            "/Music/Artist {}/Track {index:03}.flac",
            index % 60
        )),
        rekordbox_id: Some(format!("{}", 10_000 + index)),
        mix_in_ms: Some(32_000),
        mix_out_ms: Some(360_000),
        note: "long blend".to_owned(),
        ..SetEntry::default()
    }
}

/// A store holding one set of [`SET_LENGTH`] tracks.
fn store_with_a_set(directory: &Path) -> (SetStore, dubplate_prep::SetId) {
    let mut store = SetStore::open(&directory.join("dubplate.db")).expect("open the store");
    let set = store.create_set("Friday", 120).expect("create a set");
    for index in 0..SET_LENGTH {
        store
            .insert_entry(set, SetList::Main, index, entry(index))
            .expect("insert");
    }
    (store, set)
}

fn store(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("set_store_40_tracks");
    let directory = tempfile::tempdir().expect("a temporary folder");
    let (mut store, set) = store_with_a_set(directory.path());
    group.bench_function("insert_at_the_top", |bencher| {
        bencher.iter_custom(|iterations| {
            let mut elapsed = Duration::ZERO;
            for _ in 0..iterations {
                let started = Instant::now();
                store
                    .insert_entry(set, SetList::Main, 0, black_box(entry(SET_LENGTH)))
                    .expect("insert");
                elapsed += started.elapsed();
                store.remove_entry(set, SetList::Main, 0).expect("remove");
            }
            elapsed
        });
    });
    let mut forwards = true;
    group.bench_function("move_first_to_last", |bencher| {
        bencher.iter(|| {
            let (from, to) = if forwards {
                (0, SET_LENGTH - 1)
            } else {
                (SET_LENGTH - 1, 0)
            };
            forwards = !forwards;
            store
                .move_entry(set, SetList::Main, from, to)
                .expect("move");
        });
    });
    group.bench_function("load_set", |bencher| {
        bencher.iter(|| store.load_set(set).expect("load"));
    });
    group.finish();
}

/// A library of 800 tracks spread over every key, 110 to 134 BPM and every energy.
fn library() -> Vec<PrepTrack> {
    let keys = [
        "1A", "2A", "3A", "4A", "5A", "6A", "7A", "8A", "9A", "10A", "11A", "12A", "1B", "2B",
        "3B", "4B", "5B", "6B", "7B", "8B", "9B", "10B", "11B", "12B",
    ];
    (0..LIBRARY_TRACKS)
        .map(|index| PrepTrack {
            track_path: PathBuf::from(format!("/Music/Track {index:03}.flac")),
            rekordbox_id: Some(index.to_string()),
            title: format!("Track {index:03}"),
            artist: format!("Artist {}", index % 60),
            key: Key::parse_file_key(keys[index * 7 % keys.len()]),
            bpm: Some(110.0 + (index * 37 % 240) as f32 / 10.0),
            energy: (index % 9 != 0).then_some((index % 10) as f32 + 1.0),
        })
        .collect()
}

fn suggestions(criterion: &mut Criterion) {
    let candidates = library();
    let reference = candidates[123].clone();
    let in_set: HashSet<PathBuf> = candidates
        .iter()
        .step_by(20)
        .map(|track| track.track_path.clone())
        .collect();
    criterion.bench_function("suggest_next_800_tracks", |bencher| {
        bencher.iter(|| {
            suggest_next(
                black_box(&reference),
                black_box(&candidates),
                EnergyDirection::Up,
                |candidate| in_set.contains(&candidate.track_path),
                12,
            )
        });
    });
}

/// A click on every beat at [`CLICK_BPM`] over a held A minor chord, as a 16-bit stereo WAV.
fn write_click_track(path: &Path) {
    let sample_count = (ANALYSED_SECONDS * SAMPLE_RATE as f32) as usize;
    let beat_seconds = 60.0 / CLICK_BPM;
    let mut bytes = Vec::with_capacity(44 + sample_count * 4);
    let data_length = u32::try_from(sample_count * 4).expect("a short file");
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_length).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    bytes.extend_from_slice(&(SAMPLE_RATE * 4).to_le_bytes());
    bytes.extend_from_slice(&4_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_length.to_le_bytes());
    for index in 0..sample_count {
        let seconds = index as f32 / SAMPLE_RATE as f32;
        let since_beat = seconds % beat_seconds;
        let click = 0.7 * (TAU * 1_500.0 * since_beat).sin() * (-since_beat * 120.0).exp();
        let chord: f32 = [220.0, 261.63, 329.63]
            .iter()
            .map(|frequency| 0.08 * (TAU * frequency * seconds).sin())
            .sum();
        let sample = ((click + chord).clamp(-1.0, 1.0) * 32_767.0).round() as i16;
        bytes.extend_from_slice(&sample.to_le_bytes());
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    std::fs::write(path, bytes).expect("write the click track");
}

fn analysis(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("analysis_20_second_track");
    group.sample_size(10);
    let directory = tempfile::tempdir().expect("a temporary folder");
    let track = directory.path().join("click.wav");
    write_click_track(&track);
    group.bench_function("decode_and_analyse", |bencher| {
        bencher.iter(|| analyse_track(black_box(&track)).expect("analyse"));
    });
    let mut cache =
        AnalysisCache::open(&directory.path().join("analysis.db")).expect("open the cache");
    let analysed = analyse_track(&track).expect("analyse");
    cache.put(&track, &analysed).expect("cache the analysis");
    group.bench_function("cache_hit", |bencher| {
        bencher.iter(|| cache.get(black_box(&track)).expect("read the cache"));
    });
    group.bench_function("cache_store", |bencher| {
        bencher.iter(|| cache.put(black_box(&track), &analysed).expect("store"));
    });
    group.finish();
}

criterion_group!(benches, store, suggestions, analysis);
criterion_main!(benches);
