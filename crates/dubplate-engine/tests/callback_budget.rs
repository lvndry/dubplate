//! Manual callback timing with two key-locked decks and eight sample voices.
//!
//! Run in an optimized build with `--ignored --nocapture`; timing is diagnostic rather than
//! a CI assertion because host scheduling and CPU load are outside the engine's control.

mod common;

use std::time::{Duration, Instant};

use common::{SAMPLE_RATE, sine_track};
use dubplate_engine::{DeckId, SampleMode, SampleSettings, offline_engine};

// Keep the timing tool dependency-free. macOS's clock id is 16; Linux's is 3.
// CPU time excludes time descheduled by other builds, while wall time shows actual deadlines.
#[cfg(any(
    target_os = "macos",
    all(target_os = "linux", target_pointer_width = "64")
))]
fn thread_cpu_time() -> Duration {
    #[repr(C)]
    struct Time {
        seconds: std::ffi::c_long,
        nanoseconds: std::ffi::c_long,
    }
    unsafe extern "C" {
        fn clock_gettime(clock_id: std::ffi::c_int, time: *mut Time) -> std::ffi::c_int;
    }
    let mut time = Time {
        seconds: 0,
        nanoseconds: 0,
    };
    let clock_id = if cfg!(target_os = "macos") { 16 } else { 3 };
    // SAFETY: Time matches the platform's timespec ABI and points to initialized writable
    // storage; CLOCK_THREAD_CPUTIME_ID is a supported clock on these platforms.
    assert_eq!(unsafe { clock_gettime(clock_id, &mut time) }, 0);
    Duration::new(time.seconds as u64, time.nanoseconds as u32)
}

#[cfg(not(any(
    target_os = "macos",
    all(target_os = "linux", target_pointer_width = "64")
)))]
fn thread_cpu_time() -> Duration {
    Duration::ZERO
}

#[test]
#[ignore = "manual optimized-build callback budget measurement"]
fn eight_samples_and_two_key_locked_decks() {
    for source_rate in [48_000, 192_000, 384_000] {
        let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
        controller.set_sampler_volume(1.0).unwrap();
        let deck_track = sine_track(1_000.0, 0.05, 0.05, 4.0, SAMPLE_RATE);
        for deck in DeckId::ALL {
            controller.load_track(deck, deck_track.clone()).unwrap();
            controller.set_key_lock(deck, true).unwrap();
            controller.set_tempo(deck, 1.1).unwrap();
            controller.play(deck).unwrap();
        }
        for slot in 0..8 {
            // Separate decoded buffers and pitches model distinct samples, rather than
            // giving every voice the same decoded cache lines.
            let sample = sine_track(1_000.0 + slot as f64 * 17.0, 0.05, 0.05, 1.0, source_rate);
            controller
                .load_sample(
                    slot,
                    sample,
                    SampleSettings {
                        mode: SampleMode::Loop,
                        ..SampleSettings::default()
                    },
                )
                .unwrap();
            controller.trigger_sample(slot).unwrap();
        }
        let mut output = [0.0; 512 * 2];
        // Exclude command consumption, stretcher priming and cold caches.
        for _ in 0..16 {
            renderer.render(&mut output, 2);
        }
        let mut total = Duration::ZERO;
        let mut longest = Duration::ZERO;
        let mut samples = Vec::with_capacity(200);
        let cpu_started = thread_cpu_time();
        for _ in 0..200 {
            let started = Instant::now();
            renderer.render(&mut output, 2);
            let elapsed = started.elapsed();
            total += elapsed;
            longest = longest.max(elapsed);
            samples.push(elapsed);
            assert!(output.iter().all(|sample| sample.is_finite()));
        }
        let cpu_average = (thread_cpu_time() - cpu_started).as_secs_f64() / 200.0;
        samples.sort_unstable();
        let average = total.as_secs_f64() / 200.0;
        let deadline = 512.0 / f64::from(SAMPLE_RATE);
        eprintln!(
            "8 sample voices at {source_rate}Hz + 2 key-locked decks: CPU {:.3}ms ({:.1}% of callback), wall avg {:.3}ms, median {:.3}ms, p95 {:.3}ms, longest {:.3}ms, deadline {:.3}ms",
            cpu_average * 1e3,
            cpu_average / deadline * 100.0,
            average * 1e3,
            samples[100].as_secs_f64() * 1e3,
            samples[190].as_secs_f64() * 1e3,
            longest.as_secs_f64() * 1e3,
            deadline * 1e3
        );
    }
}
