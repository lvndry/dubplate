//! What the audio thread spends on one 256-frame block, the work a sound card at 48 kHz asks
//! for every 5.3 ms, in CPU time of the rendering thread.
//!
//! Each scenario plays two synthetic tracks from memory, so the numbers never depend on the
//! disk or on a decoder. The decks are sent back near the start of their tracks, outside the
//! measured time, before they reach the end, so every measured block is a block of music.

use std::hint::black_box;
use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};

use dubplate_engine::{
    BeatGrid, Controller, DeckId, EffectKind, EffectTarget, EffectUnitId, EqBand, Renderer, Stem,
    StemLevel, StemSettings, StemTracks, Track, offline_engine,
};

#[path = "support/cpu_time.rs"]
mod cpu_time;

use cpu_time::ThreadCpuTime;

const OUTPUT_RATE: u32 = 48_000;
const BLOCK_FRAMES: usize = 256;
const TRACK_SECONDS: f64 = 40.0;
const REWIND_SECONDS: f64 = 1.0;
/// Blocks rendered after a rewind before timing resumes, so the jump's crossfade and the key
/// lock's priming are not measured as steady playback.
const SETTLE_BLOCKS: u64 = 64;

/// What the two decks are doing.
#[derive(Clone, Copy)]
enum Scenario {
    /// Nothing loaded: the fixed cost of a block.
    Idle,
    /// Two decks playing at their own tempo, flat EQ, no effects.
    TwoDecksPlain,
    /// Two decks off their own tempo with EQ, filter, key lock and three effect units on.
    TwoDecksFull,
    /// Two decks off their own tempo with key lock, playing their tracks.
    TwoDecksKeyLock,
    /// Two decks off their own tempo playing their stems, the vocals out on one and the bass
    /// down on the other.
    TwoDecksStems,
    /// The same with key lock, which stretches the four stems and the track side by side.
    TwoDecksStemsKeyLock,
}

impl Scenario {
    fn plays_stems(self) -> bool {
        matches!(
            self,
            Scenario::TwoDecksStems | Scenario::TwoDecksStemsKeyLock
        )
    }

    fn key_lock(self) -> bool {
        matches!(
            self,
            Scenario::TwoDecksKeyLock | Scenario::TwoDecksStemsKeyLock
        )
    }
}

/// Stereo noise shaped into something like music: a low thump, a mid tone and hiss, so every
/// band of the EQ and every effect has signal to work on.
fn synthetic_track(sample_rate: u32, seed: u32) -> Track {
    let (samples, _) = synthetic_samples(sample_rate, seed);
    Track::from_interleaved_stereo(&samples, sample_rate).expect("a valid synthetic track")
}

/// The same track with four stems, noise at the track's rate and length: what the stems hold
/// does not change what they cost.
fn synthetic_track_with_stems(sample_rate: u32, seed: u32) -> Track {
    let (samples, frame_count) = synthetic_samples(sample_rate, seed);
    let track =
        Track::from_interleaved_stereo(&samples, sample_rate).expect("a valid synthetic track");
    let stems: Vec<Vec<i16>> = (0..Stem::COUNT as u32)
        .map(|stem| {
            let mut state = seed.wrapping_mul(31).wrapping_add(stem + 1);
            (0..frame_count * 2)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 17;
                    state ^= state << 5;
                    (state >> 20) as i16
                })
                .collect()
        })
        .collect();
    let stems = StemTracks::from_interleaved_stereo(
        std::array::from_fn(|stem| stems[stem].as_slice()),
        sample_rate,
    )
    .expect("valid synthetic stems");
    track
        .attach_stems(stems)
        .expect("stems as long as the track");
    track
}

fn synthetic_samples(sample_rate: u32, seed: u32) -> (Vec<f32>, usize) {
    let frame_count = (TRACK_SECONDS * f64::from(sample_rate)) as usize;
    let mut state = seed.max(1);
    let mut samples = Vec::with_capacity(frame_count * 2);
    for index in 0..frame_count {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        let hiss = (state as f32 / u32::MAX as f32 - 0.5) * 0.1;
        let seconds = index as f64 / f64::from(sample_rate);
        let thump = (std::f64::consts::TAU * 55.0 * seconds).sin() as f32
            * (1.0 - (seconds * 2.0).fract() as f32);
        let tone = (std::f64::consts::TAU * 880.0 * seconds).sin() as f32 * 0.2;
        samples.push(thump * 0.5 + tone + hiss);
        samples.push(thump * 0.5 - tone + hiss);
    }
    (samples, frame_count)
}

struct Session {
    controller: Controller,
    renderer: Renderer,
    output: Vec<f32>,
    blocks_until_rewind: u64,
    playing: bool,
}

impl Session {
    fn new(scenario: Scenario) -> Self {
        let (mut controller, renderer) = offline_engine(OUTPUT_RATE);
        let playing = !matches!(scenario, Scenario::Idle);
        let track = |sample_rate, seed| {
            if scenario.plays_stems() {
                synthetic_track_with_stems(sample_rate, seed)
            } else {
                synthetic_track(sample_rate, seed)
            }
        };
        if playing {
            controller
                .load_track(DeckId::A, track(44_100, 7))
                .expect("load deck A");
            controller
                .load_track(DeckId::B, track(OUTPUT_RATE, 11))
                .expect("load deck B");
            controller
                .set_beat_grid(DeckId::A, Some(BeatGrid::new(124.0, 0.1).expect("grid A")))
                .expect("grid on deck A");
            controller
                .set_beat_grid(DeckId::B, Some(BeatGrid::new(128.0, 0.0).expect("grid B")))
                .expect("grid on deck B");
            controller.set_crossfader(0.5).expect("crossfader");
        }
        if matches!(scenario, Scenario::TwoDecksFull) {
            configure_full_mix(&mut controller).expect("configure the mix");
        }
        if matches!(
            scenario,
            Scenario::TwoDecksKeyLock | Scenario::TwoDecksStems | Scenario::TwoDecksStemsKeyLock
        ) {
            configure_stems_mix(&mut controller, scenario).expect("configure the stems");
        }
        if playing {
            for deck in DeckId::ALL {
                controller.play(deck).expect("play");
            }
        }
        let mut session = Self {
            controller,
            renderer,
            output: vec![0.0; BLOCK_FRAMES * 2],
            blocks_until_rewind: 0,
            playing,
        };
        session.rewind();
        session
    }

    /// Sends both decks back near the start and lets the jump settle, untimed.
    fn rewind(&mut self) {
        if self.playing {
            for deck in DeckId::ALL {
                self.controller
                    .seek(deck, REWIND_SECONDS)
                    .expect("seek near the start");
            }
        }
        for _ in 0..SETTLE_BLOCKS {
            self.renderer.render(&mut self.output, 2);
        }
        self.controller.collect_garbage();
        let fastest_tempo = 1.1;
        let playable_seconds = (TRACK_SECONDS - REWIND_SECONDS - 2.0) / fastest_tempo;
        let playable_blocks =
            (playable_seconds * f64::from(OUTPUT_RATE)) as u64 / BLOCK_FRAMES as u64;
        self.blocks_until_rewind = playable_blocks - SETTLE_BLOCKS;
    }

    fn time_blocks(&mut self, iterations: u64) -> Duration {
        let mut elapsed = Duration::ZERO;
        let mut remaining = iterations;
        while remaining > 0 {
            if self.blocks_until_rewind == 0 {
                self.rewind();
            }
            let chunk = remaining.min(self.blocks_until_rewind);
            let started = ThreadCpuTime::now();
            for _ in 0..chunk {
                self.renderer.render(black_box(&mut self.output), 2);
            }
            elapsed += ThreadCpuTime::now() - started;
            remaining -= chunk;
            self.blocks_until_rewind -= chunk;
        }
        elapsed
    }
}

fn configure_full_mix(controller: &mut Controller) -> Result<(), dubplate_engine::EngineError> {
    controller.set_tempo(DeckId::A, 1.04)?;
    controller.set_tempo(DeckId::B, 0.97)?;
    for deck in DeckId::ALL {
        controller.set_key_lock(deck, true)?;
        controller.set_eq(deck, EqBand::Low, 0.3)?;
        controller.set_eq(deck, EqBand::Mid, 0.55)?;
        controller.set_eq(deck, EqBand::High, 0.7)?;
    }
    controller.set_filter(DeckId::A, 0.3)?;
    controller.set_filter(DeckId::B, 0.7)?;
    let units = [
        (EffectKind::Echo, EffectTarget::Deck(DeckId::A)),
        (EffectKind::Reverb, EffectTarget::Deck(DeckId::B)),
        (EffectKind::Flanger, EffectTarget::Master),
    ];
    for (unit, (kind, target)) in EffectUnitId::ALL.into_iter().zip(units) {
        controller.set_effect_kind(unit, kind)?;
        controller.set_effect_target(unit, target)?;
        controller.set_effect_depth(unit, 0.6)?;
        controller.set_effect_enabled(unit, true)?;
    }
    Ok(())
}

fn configure_stems_mix(
    controller: &mut Controller,
    scenario: Scenario,
) -> Result<(), dubplate_engine::EngineError> {
    controller.set_tempo(DeckId::A, 1.04)?;
    controller.set_tempo(DeckId::B, 0.97)?;
    for deck in DeckId::ALL {
        controller.set_key_lock(deck, scenario.key_lock())?;
    }
    if scenario.plays_stems() {
        controller.set_stems(DeckId::A, StemSettings::UNTOUCHED.without(Stem::Vocals))?;
        controller.set_stems(
            DeckId::B,
            StemSettings::UNTOUCHED.with_level(Stem::Bass, StemLevel::new(0.5)),
        )?;
    }
    Ok(())
}

fn render_block(criterion: &mut Criterion<ThreadCpuTime>) {
    let mut group = criterion.benchmark_group("render_256_frames");
    let scenarios = [
        ("idle", Scenario::Idle),
        ("two_decks_plain", Scenario::TwoDecksPlain),
        ("two_decks_full", Scenario::TwoDecksFull),
        ("two_decks_key_lock", Scenario::TwoDecksKeyLock),
        ("two_decks_stems", Scenario::TwoDecksStems),
        ("two_decks_stems_key_lock", Scenario::TwoDecksStemsKeyLock),
    ];
    for (name, scenario) in scenarios {
        let mut session = Session::new(scenario);
        group.bench_function(name, |bencher| {
            bencher.iter_custom(|iterations| session.time_blocks(iterations));
        });
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = cpu_time::criterion();
    targets = render_block
}
criterion_main!(benches);
