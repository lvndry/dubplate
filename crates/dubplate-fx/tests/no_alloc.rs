//! Proves that processing audio and turning every control never touches the heap.
//!
//! Counting is armed per thread, so allocations by the test harness or other threads are not
//! counted.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};

use dubplate_fx::{
    BeatLength, Echo, Effect, EffectConfig, EffectKind, Filter, FilterMode, FilterSlope,
    ProcessContext, Reverb, SweepFilter, create,
};

struct CountingAllocator;

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
}

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every call is forwarded unchanged to the system allocator; the counter only reads
// a thread local and bumps an atomic, which never allocate.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNTING.try_with(Cell::get).unwrap_or(false) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        // SAFETY: the caller upholds the contract of `GlobalAlloc::alloc`.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        if COUNTING.try_with(Cell::get).unwrap_or(false) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        // SAFETY: the caller upholds the contract of `GlobalAlloc::dealloc`.
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if COUNTING.try_with(Cell::get).unwrap_or(false) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        // SAFETY: the caller upholds the contract of `GlobalAlloc::alloc_zeroed`.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if COUNTING.try_with(Cell::get).unwrap_or(false) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        // SAFETY: the caller upholds the contract of `GlobalAlloc::realloc`.
        unsafe { System.realloc(pointer, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

const SAMPLE_RATE: f32 = 48_000.0;
const BLOCK_FRAMES: usize = 512;

#[test]
fn processing_and_turning_every_control_never_allocates() {
    let config = EffectConfig::default();
    let mut effects: Vec<Box<dyn Effect>> = EffectKind::ALL
        .into_iter()
        .map(|kind| create(kind, &config))
        .collect();
    let mut echo = Echo::new(&config);
    let mut reverb = Reverb::new(&config);
    let mut filter = Filter::new(FilterMode::LowPass);
    let mut sweep = SweepFilter::new();
    let mut left = vec![0.0_f32; BLOCK_FRAMES];
    let mut right = vec![0.0_f32; BLOCK_FRAMES];
    let blocks = 400;

    COUNTING.with(|flag| flag.set(true));
    let mut beat_position = 0.0;
    for block_index in 0..blocks {
        let tempo = if block_index < blocks / 2 {
            124.0
        } else {
            131.0
        };
        let sample_rate = if block_index % 100 < 90 {
            SAMPLE_RATE
        } else {
            44_100.0
        };
        let context = ProcessContext::new(sample_rate, tempo, beat_position);
        beat_position += BLOCK_FRAMES as f64 * tempo / 60.0 / f64::from(sample_rate);
        let turn = block_index as f32 / blocks as f32;

        echo.set_feedback(turn);
        reverb.set_decay_seconds(1.0 + 10.0 * turn);
        reverb.set_size(turn);
        reverb.set_damping(1.0 - turn);
        reverb.set_pre_delay_seconds(0.1 * turn);
        reverb.set_low_cut_hz(20.0 + 400.0 * turn);
        filter.set_cutoff_hz(50.0 + 10_000.0 * turn);
        filter.set_resonance(turn);
        sweep.set_position(turn * 2.0 - 1.0);
        if block_index == blocks / 3 {
            filter.set_mode(FilterMode::BandPass);
            filter.set_slope(FilterSlope::TwentyFourDecibels);
        }

        let extra: [&mut dyn Effect; 4] = [&mut echo, &mut reverb, &mut filter, &mut sweep];
        for effect in effects.iter_mut().map(|boxed| boxed.as_mut()).chain(extra) {
            if block_index % 50 == 0 {
                effect.set_enabled(!effect.is_enabled());
            }
            if block_index % 70 == 0 {
                effect
                    .set_beat(BeatLength::STANDARD[block_index / 70 % BeatLength::STANDARD.len()]);
            }
            effect.set_depth(turn);
            effect.set_mix(1.0 - 0.5 * turn);
            for (sample_index, (left_sample, right_sample)) in
                left.iter_mut().zip(right.iter_mut()).enumerate()
            {
                let phase = (block_index * BLOCK_FRAMES + sample_index) as f32 * 0.031;
                *left_sample = phase.sin() * 0.8;
                *right_sample = (phase * 1.3).cos() * 0.8;
            }
            if block_index == 123 {
                left[7] = f32::NAN;
            }
            effect.process(&mut left, &mut right, &context);
            if block_index == blocks - 1 {
                effect.reset();
            }
        }
    }
    COUNTING.with(|flag| flag.set(false));

    assert_eq!(
        ALLOCATIONS.load(Ordering::SeqCst),
        0,
        "the audio path allocated"
    );
    assert!(left.iter().chain(&right).all(|sample| sample.is_finite()));
}
