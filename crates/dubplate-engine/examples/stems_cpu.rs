//! Measures how long the audio thread takes per 256-frame block with both decks playing, with
//! and without stems, with and without key lock. Renders offline and plays nothing aloud.
//!
//! ```text
//! cargo run --profile fast -p dubplate-engine --example stems_cpu -- [--seconds 30]
//! ```

use std::time::{Duration, Instant};

use dubplate_engine::{DeckId, Stem, StemSettings, StemTracks, Track, offline_engine};

const OUTPUT_RATE: u32 = 48_000;
const TRACK_RATE: u32 = 48_000;
const STEM_RATE: u32 = TRACK_RATE;
const BLOCK: usize = 256;
const TRACK_SECONDS: f64 = 60.0;

fn main() {
    let mut arguments = std::env::args().skip(1);
    let mut seconds = 30.0;
    while let Some(argument) = arguments.next() {
        if argument == "--seconds" {
            seconds = arguments
                .next()
                .and_then(|value| value.parse().ok())
                .unwrap_or(seconds);
        }
    }
    let budget = Duration::from_secs_f64(BLOCK as f64 / f64::from(OUTPUT_RATE));
    println!(
        "{BLOCK}-frame blocks at {OUTPUT_RATE} Hz, a budget of {:.0} µs each, both decks playing:",
        budget.as_secs_f64() * 1e6
    );
    for (stems, key_lock) in [(false, false), (true, false), (false, true), (true, true)] {
        let times = measure(stems, key_lock, seconds);
        let mean = times.iter().sum::<Duration>() / times.len().max(1) as u32;
        let mut sorted = times.clone();
        sorted.sort();
        let p99 = sorted[sorted.len() * 99 / 100];
        let worst = sorted[sorted.len() - 1];
        println!(
            "  stems {:<3}  key lock {:<3}  mean {:>6.1} µs ({:>4.1}% of the budget)  p99 {:>6.1} µs  worst {:>6.1} µs",
            if stems { "on" } else { "off" },
            if key_lock { "on" } else { "off" },
            mean.as_secs_f64() * 1e6,
            100.0 * mean.as_secs_f64() / budget.as_secs_f64(),
            p99.as_secs_f64() * 1e6,
            worst.as_secs_f64() * 1e6,
        );
    }
}

/// Every block's render time over `seconds` of both decks playing.
fn measure(stems: bool, key_lock: bool, seconds: f64) -> Vec<Duration> {
    let (mut controller, mut renderer) = offline_engine(OUTPUT_RATE);
    for (deck, tempo) in [(DeckId::A, 1.05), (DeckId::B, 0.96)] {
        let track = noise_track();
        if stems {
            track
                .attach_stems(noise_stems())
                .expect("stems as long as the track");
        }
        controller.load_track(deck, track).expect("load");
        controller.set_tempo(deck, tempo).expect("tempo");
        controller.set_key_lock(deck, key_lock).expect("key lock");
        if stems {
            controller
                .set_stems(deck, StemSettings::UNTOUCHED.without(Stem::Vocals))
                .expect("stems");
        }
        controller.play(deck).expect("play");
    }
    controller.set_crossfader(0.5).expect("crossfader");
    let mut output = vec![0.0; BLOCK * 2];
    for _ in 0..200 {
        renderer.render(&mut output, 2);
    }
    let blocks = (seconds * f64::from(OUTPUT_RATE) / BLOCK as f64) as usize;
    (0..blocks)
        .map(|_| {
            let started = Instant::now();
            renderer.render(&mut output, 2);
            started.elapsed()
        })
        .collect()
}

/// A deterministic noise sequence from -1 to 1.
fn noise(seed: u32, count: usize) -> impl Iterator<Item = f32> {
    let mut state = seed;
    (0..count).map(move |_| {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state as f32 / u32::MAX as f32 * 2.0 - 1.0
    })
}

fn noise_track() -> Track {
    let samples: Vec<f32> = noise(7, (TRACK_SECONDS * f64::from(TRACK_RATE)) as usize * 2)
        .map(|sample| sample * 0.3)
        .collect();
    Track::from_interleaved_stereo(&samples, TRACK_RATE).expect("valid track")
}

fn noise_stems() -> StemTracks {
    let samples: Vec<Vec<i16>> = (0..Stem::COUNT as u32)
        .map(|stem| {
            noise(
                11 + stem,
                (TRACK_SECONDS * f64::from(STEM_RATE)) as usize * 2,
            )
            .map(|sample| (sample * 8_000.0) as i16)
            .collect()
        })
        .collect();
    StemTracks::from_interleaved_stereo(
        std::array::from_fn(|stem| samples[stem].as_slice()),
        STEM_RATE,
    )
    .expect("valid stems")
}
