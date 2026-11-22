//! Proves the audio callback never allocates or frees memory, with a counting allocator that
//! is armed on the thread running the measured code, so allocations by the test harness or
//! other threads are not counted.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::path::{Path, PathBuf};

use common::{SAMPLE_RATE, constant_track, stems_like, wait_until_decoded, write_wav};
use dubplate_engine::cpal::{OutputCallbackInfo, OutputStreamTimestamp, StreamInstant};
use dubplate_engine::{
    AudioProcessor, BeatGrid, BeatLength, Controller, DeckId, EffectKind, EffectTarget,
    EffectUnitId, EngineError, EqBand, GridBeat, MasterTempo, PadEffect, Preview, PreviewTrack,
    ProcessContext, Renderer, SampleMode, SampleSettings, SmartCfx, Stem, StemLevel, StemSettings,
    StereoBlock, SyncMode, TempoRange, offline_engine,
};

struct CountingAllocator;

thread_local! {
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}
thread_local! {
    static DEALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
}

fn counting() -> bool {
    COUNTING.try_with(Cell::get).unwrap_or(false)
}

// SAFETY: every method forwards to the system allocator unchanged; counting has no effect on
// the memory returned.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if counting() {
            ALLOCATIONS.with(|count| count.set(count.get() + 1));
        }
        // SAFETY: the caller upholds `GlobalAlloc::alloc`'s contract, which `System` shares.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if counting() {
            ALLOCATIONS.with(|count| count.set(count.get() + 1));
        }
        // SAFETY: the caller upholds `GlobalAlloc::alloc_zeroed`'s contract.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if counting() {
            ALLOCATIONS.with(|count| count.set(count.get() + 1));
        }
        // SAFETY: the caller upholds `GlobalAlloc::realloc`'s contract.
        unsafe { System.realloc(pointer, layout, new_size) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        if counting() {
            DEALLOCATIONS.with(|count| count.set(count.get() + 1));
        }
        // SAFETY: the caller upholds `GlobalAlloc::dealloc`'s contract.
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

/// Runs `body` with counting armed on this thread and returns the allocations and frees it made.
fn count_allocations(body: impl FnOnce()) -> (usize, usize) {
    let allocations_before = ALLOCATIONS.with(Cell::get);
    let deallocations_before = DEALLOCATIONS.with(Cell::get);
    let was_counting = COUNTING.with(|flag| flag.replace(true));
    struct ResetCounting(bool);
    impl Drop for ResetCounting {
        fn drop(&mut self) {
            COUNTING.with(|flag| flag.set(self.0));
        }
    }
    let guard = ResetCounting(was_counting);
    body();
    drop(guard);
    (
        ALLOCATIONS.with(Cell::get) - allocations_before,
        DEALLOCATIONS.with(Cell::get) - deallocations_before,
    )
}

#[test]
fn the_counter_sees_allocations_on_the_armed_thread() {
    let (allocations, deallocations) = count_allocations(|| {
        let boxed = std::hint::black_box(Box::new(7_u64));
        drop(boxed);
    });
    assert_eq!((allocations, deallocations), (1, 1));
}

#[test]
fn concurrent_measurements_are_independent() {
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let workers: Vec<_> = (0..2)
        .map(|_| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                // Warm the barrier's thread-local bookkeeping outside the measurement.
                barrier.wait();
                count_allocations(|| {
                    let boxed = std::hint::black_box(Box::new(7_u64));
                    barrier.wait();
                    drop(boxed);
                    barrier.wait();
                })
            })
        })
        .collect();
    for worker in workers {
        assert_eq!(worker.join().unwrap(), (1, 1));
    }
}

#[test]
fn unwinding_disarms_the_counter() {
    let _ = std::panic::catch_unwind(|| count_allocations(|| panic!("test panic")));
    assert!(!counting());
}

struct Echo {
    delay_line: Vec<f32>,
    write_index: usize,
}

impl AudioProcessor for Echo {
    fn process(&mut self, block: &mut StereoBlock, _context: &ProcessContext) {
        let (left, right) = block.channels_mut();
        for (left_sample, right_sample) in left.iter_mut().zip(right.iter_mut()) {
            let delayed = self.delay_line[self.write_index];
            self.delay_line[self.write_index] = (*left_sample + *right_sample) * 0.5;
            self.write_index = (self.write_index + 1) % self.delay_line.len();
            *left_sample += delayed * 0.3;
            *right_sample += delayed * 0.3;
        }
    }
}

fn echo() -> Box<dyn AudioProcessor> {
    Box::new(Echo {
        delay_line: vec![0.0; 12_000],
        write_index: 0,
    })
}

fn render_armed(renderer: &mut Renderer, output: &mut [f32], channel_count: usize) {
    let (allocations, deallocations) = count_allocations(|| {
        for block in output.chunks_mut(512 * channel_count) {
            renderer.render(block, channel_count);
        }
    });
    assert_eq!(allocations, 0, "the audio thread allocated");
    assert_eq!(deallocations, 0, "the audio thread freed memory");
}

type Step = fn(&mut Controller) -> Result<(), EngineError>;

fn run_session(
    controller: &mut Controller,
    renderer: &mut Renderer,
    files: &[PathBuf],
) -> Result<(), EngineError> {
    let mut output = vec![0.0_f32; SAMPLE_RATE as usize * 2];
    let track_a = controller.load_file(DeckId::A, &files[0])?;
    let track_b = controller.load_file(DeckId::B, &files[1])?;
    wait_until_decoded(&track_a);
    wait_until_decoded(&track_b);
    controller.set_deck_insert(DeckId::A, 0, Some(echo()))?;
    controller.set_master_insert(1, Some(echo()))?;
    controller.set_beat_grid(DeckId::A, Some(BeatGrid::new(124.0, 0.05)?))?;
    controller.set_beat_grid(DeckId::B, Some(BeatGrid::new(128.0, 0.0)?))?;
    let targets = [
        EffectTarget::Deck(DeckId::A),
        EffectTarget::BothDecks,
        EffectTarget::Master,
    ];
    for (unit, target) in EffectUnitId::ALL.into_iter().zip(targets) {
        controller.set_effect_target(unit, target)?;
        controller.set_effect_enabled(unit, true)?;
    }
    let recording = controller.start_recording(0.5)?;
    controller.set_key_lock(DeckId::A, true)?;
    controller.set_key_lock(DeckId::B, true)?;
    controller.set_tempo(DeckId::A, 1.07)?;
    controller.set_tempo(DeckId::B, 0.9)?;
    controller.play(DeckId::A)?;
    controller.play(DeckId::B)?;
    render_armed(renderer, &mut output, 2);

    for (step, kind) in EffectKind::ALL.into_iter().enumerate() {
        let unit = EffectUnitId::ALL[step % EffectUnitId::ALL.len()];
        let target = targets[step % targets.len()];
        let beat = BeatLength::STANDARD[step % BeatLength::STANDARD.len()];
        controller.set_effect_kind(unit, kind)?;
        controller.set_effect_target(unit, target)?;
        controller.set_effect_beat(unit, beat)?;
        controller.set_effect_depth(unit, step as f32 / EffectKind::ALL.len() as f32)?;
        if step % 4 == 3 {
            controller.toggle_effect(unit)?;
        }
        render_armed(renderer, &mut output[..4800], 2);
    }
    controller.set_master_tempo(MasterTempo::Deck(DeckId::B))?;
    controller.set_beat_grid(DeckId::A, None)?;
    render_armed(renderer, &mut output[..4800], 2);
    let beats: Vec<GridBeat> = (0..64)
        .map(|index| GridBeat {
            seconds: 0.1 + f64::from(index) * 0.49,
            bpm: 120.0 + f64::from(index % 4),
        })
        .collect();
    controller.set_beat_grid(DeckId::A, Some(BeatGrid::from_beats(&beats)?))?;
    controller.set_sync(DeckId::A, SyncMode::Beat)?;
    render_armed(renderer, &mut output[..9600], 2);

    let steps: &[Step] = &[
        |controller| controller.set_smart_fader(true),
        |controller| controller.set_fader_echo(true),
        |controller| controller.set_channel_fader(DeckId::A, 0.3),
        |controller| controller.set_channel_fader(DeckId::A, 0.0),
        |controller| controller.set_channel_fader(DeckId::A, 1.0),
        |controller| controller.set_crossfader(0.6),
        |controller| controller.set_crossfader(0.9),
        |controller| controller.set_smart_fader(false),
        |controller| controller.set_smart_cfx(DeckId::A, Some(SmartCfx::DubEcho)),
        |controller| controller.set_filter(DeckId::A, 0.9),
        |controller| controller.set_smart_cfx(DeckId::A, Some(SmartCfx::Space)),
        |controller| controller.set_smart_cfx(DeckId::A, None),
        |controller| {
            let track = controller
                .track(DeckId::B)
                .cloned()
                .ok_or(EngineError::NoOutputDevice)?;
            controller.load_sample(
                2,
                track,
                SampleSettings {
                    mode: SampleMode::Loop,
                    sync: true,
                    bpm: Some(120.0),
                    ..SampleSettings::default()
                },
            )
        },
        |controller| controller.trigger_sample(2),
        |controller| controller.trigger_sample(2),
        |controller| controller.set_sampler_cue(true),
        |controller| controller.stop_sample(2),
        |controller| controller.eject_sample(2),
        |controller| {
            let track = controller
                .track(DeckId::B)
                .cloned()
                .ok_or(EngineError::NoOutputDevice)?;
            controller.preview(Some(Preview {
                first: PreviewTrack {
                    track: track.clone(),
                    start_seconds: 0.2,
                    tempo: 1.05,
                },
                second: PreviewTrack {
                    track,
                    start_seconds: 0.0,
                    tempo: 0.95,
                },
                second_offset_seconds: 0.05,
                key_lock: true,
            }))
        },
        |controller| controller.preview(None),
        |controller| controller.set_slip(DeckId::A, true),
        |controller| controller.beat_roll(DeckId::A, BeatLength::QUARTER),
        |controller| controller.beat_roll(DeckId::A, BeatLength::EIGHTH),
        |controller| controller.release_roll(DeckId::A),
        |controller| {
            controller.hold_pad_effect(
                DeckId::B,
                PadEffect {
                    kind: EffectKind::Brake,
                    beat: BeatLength::HALF,
                    depth: 1.0,
                },
            )
        },
        |controller| controller.release_pad_effect(DeckId::B),
        |controller| controller.set_key_shift(DeckId::A, 3.0),
        |controller| controller.set_key_shift(DeckId::B, -2.0),
        |controller| controller.set_key_shift(DeckId::A, 0.0),
        |controller| controller.set_key_lock(DeckId::B, false),
        |controller| controller.set_key_lock(DeckId::B, true),
        |controller| controller.set_headphone_cue(DeckId::A, true),
        |controller| controller.set_master_cue(true),
        |controller| controller.set_headphone_mix(0.2),
        |controller| controller.set_headphone_level(0.8),
        |controller| controller.beat_loop(DeckId::A, BeatLength::FOUR),
        |controller| controller.halve_loop(DeckId::A),
        |controller| controller.double_loop(DeckId::A),
        |controller| controller.beat_jump(DeckId::A, 4.0),
        |controller| controller.reloop_or_exit(DeckId::A),
        |controller| controller.reloop_or_exit(DeckId::A),
        |controller| controller.loop_in(DeckId::B),
        |controller| controller.loop_out(DeckId::B),
        |controller| controller.hot_cue(DeckId::B, 0.5),
        |controller| controller.set_quantize(false),
        |controller| controller.beat_jump(DeckId::B, -2.0),
        |controller| controller.set_tempo_fader(DeckId::B, 0.4),
        |controller| controller.set_tempo_range(DeckId::B, TempoRange::Wide),
        |controller| controller.set_pitch_bend(DeckId::A, 0.04),
        |controller| controller.set_pitch_bend(DeckId::A, 0.0),
        |controller| controller.set_sync(DeckId::A, SyncMode::Tempo),
        |controller| controller.set_sync_half_and_double(false),
        |controller| controller.set_beat_grid(DeckId::B, Some(BeatGrid::new(70.0, 0.2)?)),
        |controller| controller.set_sync(DeckId::A, SyncMode::Off),
        |controller| controller.set_eq(DeckId::A, EqBand::Low, 0.0),
        |controller| controller.set_eq(DeckId::B, EqBand::High, 1.0),
        |controller| controller.set_filter(DeckId::A, 0.1),
        |controller| controller.set_filter(DeckId::B, 0.85),
        |controller| controller.set_filter_resonance(DeckId::B, 1.0),
        |controller| controller.jog_nudge(DeckId::A, 12),
        |controller| controller.set_jog_touch(DeckId::B, true),
        |controller| controller.jog_scratch(DeckId::B, -30),
        |controller| controller.jog_scratch(DeckId::B, 45),
        |controller| controller.set_jog_touch(DeckId::B, false),
        |controller| controller.jog_search(DeckId::A, 200),
        |controller| controller.set_vinyl_mode(DeckId::A, false),
        |controller| controller.set_jog_ticks_per_revolution(1_024),
        |controller| controller.set_tempo(DeckId::A, 1.06),
        |controller| controller.set_tempo(DeckId::B, 0.92),
        |controller| controller.set_crossfader(0.1),
        |controller| controller.set_channel_fader(DeckId::B, 0.4),
        |controller| controller.set_gain(DeckId::A, 1.4),
        |controller| controller.seek(DeckId::A, 0.75),
        |controller| controller.cue(DeckId::B),
        |controller| controller.set_deck_insert(DeckId::A, 0, Some(echo())),
        |controller| controller.set_master_insert(1, None),
        |controller| attach_stems(controller, DeckId::A),
        |controller| attach_stems(controller, DeckId::B),
        |controller| controller.set_stems(DeckId::A, StemSettings::UNTOUCHED.without(Stem::Vocals)),
        |controller| controller.set_stems(DeckId::A, StemSettings::UNTOUCHED.solo(Stem::Vocals)),
        |controller| {
            controller.set_stems(
                DeckId::B,
                StemSettings::UNTOUCHED.with_level(Stem::Bass, StemLevel::new(0.5)),
            )
        },
        |controller| controller.set_stems(DeckId::A, StemSettings::UNTOUCHED),
        |controller| controller.load_track(DeckId::B, constant_track(0.3, 0.3, 2.0, SAMPLE_RATE)),
        |controller| {
            controller.load_prepared_track(
                DeckId::B,
                constant_track(0.2, 0.2, 2.0, SAMPLE_RATE),
                Some(BeatGrid::new(120.0, 0.0)?),
            )
        },
        |controller| controller.duplicate_deck(DeckId::B, DeckId::A).map(|_| ()),
        |controller| {
            let snapshot = controller.snapshot_deck(DeckId::B)?;
            controller.restore_deck(DeckId::A, &snapshot)
        },
        |controller| controller.play(DeckId::B),
        |controller| controller.eject(DeckId::A),
    ];
    for step in steps {
        step(controller)?;
        render_armed(renderer, &mut output[..4800], 2);
    }
    render_armed(renderer, &mut output, 2);
    assert!(
        recording.dropped_frames() > 0,
        "the full tap keeps dropping without allocating"
    );
    controller.stop_recording()?;
    render_armed(renderer, &mut output[..4800], 2);
    let mut odd_sized = vec![0.0_f32; 1999 * 3];
    render_armed(renderer, &mut odd_sized, 3);
    let mut four_channels = vec![0.0_f32; 2048 * 4];
    render_armed(renderer, &mut four_channels, 4);
    Ok(())
}

/// Gives the track on `deck` stems as long as it, at its rate.
fn attach_stems(controller: &mut Controller, deck: DeckId) -> Result<(), EngineError> {
    let track = controller
        .track(deck)
        .cloned()
        .ok_or(EngineError::NoOutputDevice)?;
    let seconds = track.duration_seconds().unwrap_or_default();
    track.attach_stems(stems_like(seconds, track.sample_rate()))
}

fn generated_files(directory: &Path) -> Vec<PathBuf> {
    [("a.wav", 44_100, 0.4_f32), ("b.wav", 48_000, -0.3)]
        .into_iter()
        .map(|(name, sample_rate, value)| {
            let path = directory.join(name);
            let samples = vec![value; sample_rate as usize * 2 * 4];
            write_wav(&path, &samples, 2, sample_rate);
            path
        })
        .collect()
}

#[test]
fn rendering_never_allocates_or_frees() {
    let directory = tempfile::tempdir().unwrap();
    let files = generated_files(directory.path());
    let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
    run_session(&mut controller, &mut renderer, &files).unwrap();
    assert_eq!(controller.stats().starved_callbacks, 0);
}

fn callback_info(callback_nanos: u64) -> OutputCallbackInfo {
    OutputCallbackInfo::new(OutputStreamTimestamp {
        callback: StreamInstant::from_nanos(callback_nanos),
        playback: StreamInstant::from_nanos(callback_nanos + 15_000_000),
    })
}

#[test]
fn the_device_callback_never_allocates_or_frees() {
    let directory = tempfile::tempdir().unwrap();
    let files = generated_files(directory.path());
    let (mut controller, renderer) = offline_engine(SAMPLE_RATE);
    let track = controller.load_file(DeckId::A, &files[0]).unwrap();
    wait_until_decoded(&track);
    controller.set_master_insert(0, Some(echo())).unwrap();
    controller.play(DeckId::A).unwrap();
    let mut callback = renderer.into_device_callback::<i16>(2);
    let mut output = vec![0_i16; 1024 * 2];
    let buffer_nanos = 1024 * 1_000_000_000 / u64::from(SAMPLE_RATE);
    let (allocations, deallocations) = count_allocations(|| {
        for callback_index in 0..150 {
            callback(&mut output, &callback_info(callback_index * buffer_nanos));
        }
    });
    assert_eq!(allocations, 0, "the device callback allocated");
    assert_eq!(deallocations, 0, "the device callback freed memory");
    assert!(output.iter().any(|sample| *sample != 0));

    let stats = controller.stats();
    assert_eq!(stats.callbacks, 150);
    assert_eq!(stats.buffer_frames, 1024);
    assert_eq!(stats.device_gaps, 0);
    assert_eq!(stats.output_latency_ms, Some(15.0));
}
