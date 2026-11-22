//! What each effect, the channel EQ and the limiter cost on one 256-frame block, switched on at
//! a performance depth and following a 124 BPM beat.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use dubplate_fx::{
    BeatLength, EffectConfig, EffectKind, EqBand, Isolator, Limiter, ProcessContext, create,
};

const SAMPLE_RATE: f32 = 48_000.0;
const BLOCK_FRAMES: usize = 256;
const TEMPO_BPM: f64 = 124.0;
/// Blocks run before measuring, so delay lines hold a trail and every fade has finished.
const WARM_UP_BLOCKS: usize = 400;

/// A mix of a low tone and noise at `level`, so filters, delays and the limiter all have work.
fn source_block(seed: u32, level: f32) -> (Vec<f32>, Vec<f32>) {
    let mut state = seed.max(1);
    let mut left = Vec::with_capacity(BLOCK_FRAMES);
    let mut right = Vec::with_capacity(BLOCK_FRAMES);
    for index in 0..BLOCK_FRAMES {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        let noise = state as f32 / u32::MAX as f32 - 0.5;
        let tone = (std::f32::consts::TAU * 110.0 * index as f32 / SAMPLE_RATE).sin();
        left.push((tone * 0.8 + noise * 0.4) * level);
        right.push((tone * 0.8 - noise * 0.4) * level);
    }
    (left, right)
}

/// The beat position after `blocks` blocks at the bench tempo.
fn beat_after(blocks: usize) -> f64 {
    blocks as f64 * BLOCK_FRAMES as f64 / f64::from(SAMPLE_RATE) * TEMPO_BPM / 60.0
}

/// Runs `process` on a fresh copy of the source block each time, moving the beat on as music
/// plays, so the input never decays into silence and beat-synced effects keep sweeping.
struct BlockRunner {
    source_left: Vec<f32>,
    source_right: Vec<f32>,
    left: Vec<f32>,
    right: Vec<f32>,
    blocks: usize,
}

impl BlockRunner {
    fn new(level: f32) -> Self {
        let (source_left, source_right) = source_block(17, level);
        Self {
            left: source_left.clone(),
            right: source_right.clone(),
            source_left,
            source_right,
            blocks: 0,
        }
    }

    fn run(&mut self, process: &mut impl FnMut(&mut [f32], &mut [f32], &ProcessContext)) {
        self.left.copy_from_slice(&self.source_left);
        self.right.copy_from_slice(&self.source_right);
        let context = ProcessContext::new(SAMPLE_RATE, TEMPO_BPM, beat_after(self.blocks));
        process(
            black_box(&mut self.left),
            black_box(&mut self.right),
            &context,
        );
        self.blocks = (self.blocks + 1) % 100_000;
    }
}

fn effects(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("effect_256_frames");
    let config = EffectConfig {
        max_sample_rate: SAMPLE_RATE,
        ..EffectConfig::default()
    };
    for kind in EffectKind::ALL {
        let mut effect = create(kind, &config);
        effect.set_beat(BeatLength::HALF);
        effect.set_depth(0.7);
        effect.set_enabled(true);
        let mut runner = BlockRunner::new(1.0);
        let mut process = |left: &mut [f32], right: &mut [f32], context: &ProcessContext| {
            effect.process(left, right, context);
        };
        for _ in 0..WARM_UP_BLOCKS {
            runner.run(&mut process);
        }
        group.bench_function(BenchmarkId::from_parameter(kind.name()), |bencher| {
            bencher.iter(|| runner.run(&mut process));
        });
    }
    group.finish();
}

fn channel_and_master(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("mixer_256_frames");
    let mut isolator = Isolator::new(SAMPLE_RATE);
    isolator.set_gain(EqBand::Low, 0.3);
    isolator.set_gain(EqBand::High, 1.5);
    let mut runner = BlockRunner::new(1.0);
    let mut process_isolator = |left: &mut [f32], right: &mut [f32], _context: &ProcessContext| {
        isolator.process(left, right);
    };
    group.bench_function("isolator_eq", |bencher| {
        bencher.iter(|| runner.run(&mut process_isolator));
    });

    let mut limiter = Limiter::new(SAMPLE_RATE);
    let mut runner = BlockRunner::new(2.0);
    let mut process_limiter = |left: &mut [f32], right: &mut [f32], _context: &ProcessContext| {
        limiter.process(left, right);
    };
    group.bench_function("limiter_limiting", |bencher| {
        bencher.iter(|| runner.run(&mut process_limiter));
    });
    group.finish();
}

criterion_group!(benches, effects, channel_and_master);
criterion_main!(benches);
