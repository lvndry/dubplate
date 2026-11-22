use std::f32::consts::{FRAC_1_SQRT_2, TAU};

use crate::config::EffectConfig;
use crate::context::BlockTiming;
use crate::dsp::delay_line::{DelayLine, MIN_DELAY_SAMPLES};
use crate::dsp::delay_time::DelayTime;
use crate::dsp::filters::{OnePole, Svf, SvfCoefficients, one_pole_gain};
use crate::dsp::loudness::loop_loudness_gain;
use crate::dsp::shaping::flush_denormal;
use crate::dsp::smoothing::Smoothed;
use crate::effect::EffectKind;
use crate::effects::tape::read_taps;
use crate::unit::{Controls, Frame, Kernel, MixLaw, Unit, implement_effect, knob};

const LINE_COUNT: usize = 8;
const LINE_MILLISECONDS: [f32; LINE_COUNT] = [43.1, 51.7, 58.3, 66.1, 72.7, 81.3, 89.9, 97.3];
const MODULATION_RATES_HZ: [f32; LINE_COUNT] = [0.31, 0.43, 0.57, 0.67, 0.79, 0.89, 0.97, 1.07];
const MODULATION_MILLISECONDS: f32 = 0.45;
const LEFT_SIGNS: [f32; LINE_COUNT] = [1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0];
const RIGHT_SIGNS: [f32; LINE_COUNT] = [1.0, 1.0, -1.0, -1.0, 1.0, 1.0, -1.0, -1.0];
const DIFFUSER_MILLISECONDS: [[f32; 4]; 2] = [[4.77, 3.59, 12.73, 9.30], [4.93, 3.73, 12.29, 9.61]];
const DIFFUSER_GAINS: [f32; 4] = [0.7, 0.7, 0.6, 0.6];
const MIN_SIZE_SCALE: f32 = 0.5;
const MAX_SIZE_SCALE: f32 = 1.5;
const MIN_DECAY_SECONDS: f32 = 0.2;
const MAX_DECAY_SECONDS: f32 = 20.0;
const MAX_PRE_DELAY_SECONDS: f32 = 0.25;
const BRIGHTEST_DAMPING_HZ: f32 = 20_000.0;
const DARKEST_DAMPING_HZ: f32 = 1_200.0;
const MIN_LOW_CUT_HZ: f32 = 20.0;
const MAX_LOW_CUT_HZ: f32 = 1_000.0;
const OUTPUT_GAIN: f32 = 0.28;
const LEVEL_REFERENCE_DECAY_SECONDS: f32 = 2.5;
const MAX_LEVEL_BOOST: f32 = 1.4;
const CONTROL_INTERVAL: u32 = 32;
const SIZE_SMOOTHING_SECONDS: f32 = 0.25;
const TONE_SMOOTHING_SECONDS: f32 = 0.05;
const TRAIL_MARGIN_SECONDS: f32 = 0.3;

const DEFAULT_DECAY_SECONDS: f32 = 2.5;
const DEFAULT_SIZE: f32 = 0.6;
const DEFAULT_DAMPING: f32 = 0.45;
const DEFAULT_PRE_DELAY_SECONDS: f32 = 0.015;
const DEFAULT_LOW_CUT_HZ: f32 = 200.0;
const DEFAULT_MIX: f32 = 0.5;
const SHORTEST_DEPTH_DECAY_SECONDS: f32 = 2.0;
const LONGEST_DEPTH_DECAY_SECONDS: f32 = 5.0;

/// A stereo algorithmic reverb, from a small room to a long hall.
///
/// Built as an eight-line feedback delay network behind four stages of diffusion, with gently
/// moving line lengths so long tails stay smooth rather than metallic. The input is cut below
/// 200 Hz by default, so reverb on a kick drum adds space without mud. Switched off, the
/// reverb stops taking in new audio and its tail rings out.
///
/// The depth knob balances the music and the reverb, as the LEVEL/DEPTH knob of a DJ mixer
/// does: in the middle both play at full level and fully right only the reverb plays. It also
/// lengthens the decay from 2 to 5 s.
pub struct Reverb {
    unit: Unit<ReverbKernel>,
}

impl Reverb {
    /// Builds a reverb with a 2.5 s decay and mix 0.5, switched off. This allocates.
    pub fn new(config: &EffectConfig) -> Self {
        Self {
            unit: Unit::new(ReverbKernel::new(config), DEFAULT_MIX),
        }
    }

    /// Sets the time the tail takes to fall by 60 dB, from 0.2 to 20 seconds.
    pub fn set_decay_seconds(&mut self, seconds: f32) {
        let decay = &mut self.unit.kernel.decay_seconds;
        decay.set_target(knob(
            seconds,
            MIN_DECAY_SECONDS,
            MAX_DECAY_SECONDS,
            decay.target(),
        ));
    }

    /// Sets the size of the space, from 0 (small and dense) to 1 (large and open).
    pub fn set_size(&mut self, size: f32) {
        let scale = &mut self.unit.kernel.size_scale;
        let size = knob(size, 0.0, 1.0, DEFAULT_SIZE);
        scale.set_target(MIN_SIZE_SCALE + (MAX_SIZE_SCALE - MIN_SIZE_SCALE) * size);
    }

    /// Sets how quickly high frequencies die away, from 0 (bright) to 1 (dark).
    pub fn set_damping(&mut self, damping: f32) {
        let current = &mut self.unit.kernel.damping;
        current.set_target(knob(damping, 0.0, 1.0, current.target()));
    }

    /// Sets the gap between the sound and the start of its reverb, from 0 to 0.25 seconds.
    pub fn set_pre_delay_seconds(&mut self, seconds: f32) {
        let kernel = &mut self.unit.kernel;
        kernel.pre_delay_seconds = knob(
            seconds,
            0.0,
            MAX_PRE_DELAY_SECONDS,
            kernel.pre_delay_seconds,
        );
    }

    /// Sets the frequency below which sound is kept out of the reverb, from 20 to 1000 Hz.
    pub fn set_low_cut_hz(&mut self, hertz: f32) {
        let low_cut = &mut self.unit.kernel.low_cut_octaves;
        let hertz = knob(hertz, MIN_LOW_CUT_HZ, MAX_LOW_CUT_HZ, DEFAULT_LOW_CUT_HZ);
        low_cut.set_target(hertz.log2());
    }
}

implement_effect!(Reverb);

impl Controls for Reverb {
    fn effect_kind(&self) -> EffectKind {
        EffectKind::Reverb
    }

    fn apply_depth(&mut self, depth: f32) {
        self.unit.set_mix(depth);
        self.set_decay_seconds(
            SHORTEST_DEPTH_DECAY_SECONDS
                + (LONGEST_DEPTH_DECAY_SECONDS - SHORTEST_DEPTH_DECAY_SECONDS) * depth,
        );
    }
}

#[derive(Clone, Debug)]
struct Diffuser {
    line: DelayLine,
    delay_samples: f32,
}

impl Diffuser {
    #[inline]
    fn tick(&mut self, input: f32, gain: f32) -> f32 {
        let delayed = self.line.read_linear(self.delay_samples);
        let stored = flush_denormal(input + gain * delayed);
        self.line.write(stored);
        delayed - gain * stored
    }
}

pub(crate) struct ReverbKernel {
    pre_delay: [DelayLine; 2],
    pre_delay_time: DelayTime,
    pre_delay_seconds: f32,
    low_cut: [Svf; 2],
    low_cut_octaves: Smoothed,
    low_cut_coefficients: SvfCoefficients,
    diffusers: [[Diffuser; 4]; 2],
    lines: [DelayLine; LINE_COUNT],
    damping_filters: [OnePole; LINE_COUNT],
    modulation_phases: [f32; LINE_COUNT],
    line_base_samples: [f32; LINE_COUNT],
    line_gains: [f32; LINE_COUNT],
    damping_gain: f32,
    output_gain: f32,
    decay_seconds: Smoothed,
    damping: Smoothed,
    size_scale: Smoothed,
    control_countdown: u32,
    sample_rate: f32,
}

impl ReverbKernel {
    fn new(config: &EffectConfig) -> Self {
        let longest_line = LINE_MILLISECONDS[LINE_COUNT - 1] * MAX_SIZE_SCALE
            + MODULATION_MILLISECONDS * MAX_SIZE_SCALE;
        let line_capacity = config.samples_for(longest_line / 1000.0) + 4;
        let pre_delay_capacity = config.samples_for(MAX_PRE_DELAY_SECONDS) + 4;
        let diffuser = |milliseconds: f32| Diffuser {
            line: DelayLine::new(config.samples_for(milliseconds / 1000.0) + 4),
            delay_samples: 1.0,
        };
        let diffusers = DIFFUSER_MILLISECONDS.map(|channel| channel.map(diffuser));
        let low_cut_octaves = Smoothed::new(DEFAULT_LOW_CUT_HZ.log2(), TONE_SMOOTHING_SECONDS);
        Self {
            pre_delay: [
                DelayLine::new(pre_delay_capacity),
                DelayLine::new(pre_delay_capacity),
            ],
            pre_delay_time: DelayTime::new(),
            pre_delay_seconds: DEFAULT_PRE_DELAY_SECONDS,
            low_cut: [Svf::default(); 2],
            low_cut_octaves,
            low_cut_coefficients: SvfCoefficients::new(DEFAULT_LOW_CUT_HZ, FRAC_1_SQRT_2, 48_000.0),
            diffusers,
            lines: std::array::from_fn(|_| DelayLine::new(line_capacity)),
            damping_filters: [OnePole::default(); LINE_COUNT],
            modulation_phases: std::array::from_fn(|line_index| {
                line_index as f32 / LINE_COUNT as f32
            }),
            line_base_samples: [0.0; LINE_COUNT],
            line_gains: [0.0; LINE_COUNT],
            damping_gain: 1.0,
            output_gain: OUTPUT_GAIN,
            decay_seconds: Smoothed::new(DEFAULT_DECAY_SECONDS, TONE_SMOOTHING_SECONDS),
            damping: Smoothed::new(DEFAULT_DAMPING, TONE_SMOOTHING_SECONDS),
            size_scale: Smoothed::new(
                MIN_SIZE_SCALE + (MAX_SIZE_SCALE - MIN_SIZE_SCALE) * DEFAULT_SIZE,
                SIZE_SMOOTHING_SECONDS,
            ),
            control_countdown: 0,
            sample_rate: 0.0,
        }
    }

    fn update_controls(&mut self) {
        let decay_samples = self.decay_seconds.current() * self.sample_rate;
        let reference_samples = LEVEL_REFERENCE_DECAY_SECONDS * self.sample_rate;
        let size_scale = self.size_scale.current();
        let mut loudness = 0.0;
        let mut reference_loudness = 0.0;
        for (gain, base) in self.line_gains.iter_mut().zip(self.line_base_samples) {
            let line_samples = base * size_scale;
            *gain = decay_gain(line_samples, decay_samples);
            loudness += loop_loudness_gain(*gain);
            reference_loudness += loop_loudness_gain(decay_gain(line_samples, reference_samples));
        }
        let level = loudness / reference_loudness;
        self.output_gain = OUTPUT_GAIN * level.min(MAX_LEVEL_BOOST);
        let damping_hz = BRIGHTEST_DAMPING_HZ
            * (DARKEST_DAMPING_HZ / BRIGHTEST_DAMPING_HZ).powf(self.damping.current());
        self.damping_gain = if damping_hz >= self.sample_rate * 0.45 {
            1.0
        } else {
            one_pole_gain(damping_hz, self.sample_rate)
        };
        self.low_cut_coefficients = SvfCoefficients::new(
            self.low_cut_octaves.current().exp2(),
            FRAC_1_SQRT_2,
            self.sample_rate,
        );
    }
}

/// The gain per pass that makes a line of `line_samples` fall by 60 dB in `decay_samples`.
#[inline]
fn decay_gain(line_samples: f32, decay_samples: f32) -> f32 {
    10.0_f32.powf(-3.0 * line_samples / decay_samples)
}

impl Kernel for ReverbKernel {
    const TRAILS: bool = true;
    const MIX_LAW: MixLaw = MixLaw::Blend;

    fn begin_block(&mut self, timing: &BlockTiming) {
        if timing.sample_rate != self.sample_rate {
            self.sample_rate = timing.sample_rate;
            for smoothed in [
                &mut self.decay_seconds,
                &mut self.damping,
                &mut self.size_scale,
                &mut self.low_cut_octaves,
            ] {
                smoothed.prepare(timing.sample_rate);
            }
            for (base, milliseconds) in self.line_base_samples.iter_mut().zip(LINE_MILLISECONDS) {
                *base = milliseconds / 1000.0 * timing.sample_rate;
            }
            for (channel, milliseconds) in self.diffusers.iter_mut().zip(DIFFUSER_MILLISECONDS) {
                for (diffuser, delay) in channel.iter_mut().zip(milliseconds) {
                    let samples = (delay / 1000.0 * timing.sample_rate).round();
                    diffuser.delay_samples = samples.clamp(1.0, diffuser.line.max_delay());
                }
            }
            self.control_countdown = 0;
        }
        let pre_delay = (self.pre_delay_seconds * timing.sample_rate)
            .clamp(MIN_DELAY_SAMPLES, self.pre_delay[0].max_delay());
        self.pre_delay_time
            .set_target(pre_delay, timing.sample_rate);
    }

    #[inline]
    fn tick(&mut self, input: Frame, _beat_position: f64) -> Frame {
        self.decay_seconds.next();
        self.damping.next();
        self.low_cut_octaves.next();
        let size_scale = self.size_scale.next();
        if self.control_countdown == 0 {
            self.update_controls();
            self.control_countdown = CONTROL_INTERVAL;
        }
        self.control_countdown -= 1;

        let taps = self.pre_delay_time.next();
        let mut diffused = [0.0; 2];
        for (channel_index, channel_input) in input.into_iter().enumerate() {
            let line = &mut self.pre_delay[channel_index];
            let delayed = read_taps(line, &taps);
            line.write(channel_input);
            let mut signal = self.low_cut[channel_index]
                .tick(delayed, &self.low_cut_coefficients)
                .high;
            for (diffuser, gain) in self.diffusers[channel_index].iter_mut().zip(DIFFUSER_GAINS) {
                signal = diffuser.tick(signal, gain);
            }
            diffused[channel_index] = signal;
        }

        let modulation_depth = MODULATION_MILLISECONDS / 1000.0 * self.sample_rate * size_scale;
        let mut damped = [0.0; LINE_COUNT];
        for (line_index, damped_value) in damped.iter_mut().enumerate() {
            let phase = &mut self.modulation_phases[line_index];
            *phase += MODULATION_RATES_HZ[line_index] / self.sample_rate;
            if *phase >= 1.0 {
                *phase -= 1.0;
            }
            let length = self.line_base_samples[line_index] * size_scale
                + modulation_depth * (TAU * *phase).sin();
            let read = self.lines[line_index].read_linear(length);
            *damped_value = self.damping_filters[line_index].low_pass(read, self.damping_gain);
        }

        let mut output = [0.0; 2];
        let mut sum = 0.0;
        for (line_index, value) in damped.iter_mut().enumerate() {
            output[0] += *value * LEFT_SIGNS[line_index];
            output[1] += *value * RIGHT_SIGNS[line_index];
            *value *= self.line_gains[line_index];
            sum += *value;
        }
        let reflection = sum * (2.0 / LINE_COUNT as f32);
        for (line_index, (line, value)) in self.lines.iter_mut().zip(damped).enumerate() {
            let injected = diffused[line_index % 2];
            line.write(flush_denormal(value - reflection + injected));
        }

        [output[0] * self.output_gain, output[1] * self.output_gain]
    }

    fn reset(&mut self) {
        for line in self.pre_delay.iter_mut().chain(self.lines.iter_mut()) {
            line.clear();
        }
        for channel in &mut self.diffusers {
            for diffuser in channel {
                diffuser.line.clear();
            }
        }
        for filter in &mut self.low_cut {
            filter.reset();
        }
        for filter in &mut self.damping_filters {
            filter.reset();
        }
        for smoothed in [
            &mut self.decay_seconds,
            &mut self.damping,
            &mut self.size_scale,
            &mut self.low_cut_octaves,
        ] {
            smoothed.snap_to_target();
        }
        self.control_countdown = 0;
    }

    fn tail_seconds(&self) -> f32 {
        self.pre_delay_seconds + TRAIL_MARGIN_SECONDS
    }
}
