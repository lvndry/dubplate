//! The voicings of the echo family.

use crate::config::EffectConfig;
use crate::dsp::allpass::Allpass;
use crate::dsp::crush::SampleCrush;
use crate::dsp::filters::{Svf, SvfCoefficients, SvfOutput};
use crate::dsp::noise::WhiteNoise;
use crate::dsp::pitch_shift::PitchShifter;
use crate::dsp::smoothing::Smoothed;
use crate::effect::EffectKind;
use crate::effects::delays::tape::{RepeatTone, RepeatToneGains, feedback_for_depth};
use crate::unit::{Frame, MixLaw, engaged};

use super::Voicing;

/// Ages each repeat like tape, a little darker and thinner on every pass.
#[derive(Default)]
struct Tape {
    tones: [RepeatTone; 2],
    gains: RepeatToneGains,
}

impl Tape {
    fn prepare(&mut self, sample_rate: f32) {
        self.gains = RepeatToneGains::new(sample_rate);
    }

    #[inline]
    fn shape(&mut self, repeat: Frame) -> Frame {
        let [left, right] = &mut self.tones;
        [
            left.shape(repeat[0], &self.gains),
            right.shape(repeat[1], &self.gains),
        ]
    }

    fn reset(&mut self) {
        for tone in &mut self.tones {
            tone.reset();
        }
    }
}

/// One repeat, untouched: the Delay.
#[derive(Default)]
pub(crate) struct Single;

impl Voicing for Single {
    fn kind(&self) -> EffectKind {
        EffectKind::Delay
    }

    fn feedback_for_depth(&self, _depth: f32) -> f32 {
        0.0
    }

    fn prepare(&mut self, _sample_rate: f32) {}

    #[inline]
    fn shape(&mut self, repeat: Frame) -> Frame {
        repeat
    }

    fn reset(&mut self) {}
}

/// Tape-aged repeats with a trail that lengthens as the knob turns: the Echo.
#[derive(Default)]
pub(crate) struct Aged<const HISTORY: bool = false> {
    tape: Tape,
}

impl<const HISTORY: bool> Voicing for Aged<HISTORY> {
    const KEEPS_HISTORY: bool = HISTORY;

    fn kind(&self) -> EffectKind {
        EffectKind::Echo
    }

    fn feedback_for_depth(&self, depth: f32) -> f32 {
        feedback_for_depth(depth)
    }

    fn prepare(&mut self, sample_rate: f32) {
        self.tape.prepare(sample_rate);
    }

    #[inline]
    fn shape(&mut self, repeat: Frame) -> Frame {
        self.tape.shape(repeat)
    }

    fn reset(&mut self) {
        self.tape.reset();
    }
}

const SMALLEST_STEP_SEMITONES: f32 = 1.0;
const LARGEST_STEP_SEMITONES: f32 = 5.0;
const SHIFT_WINDOW_SECONDS: f32 = 0.04;

/// Shifts the music into the loop and every repeat on its way round by the same interval, so
/// each repeat sounds a step higher, or lower, than the one before: Up Echo and Down Echo. The
/// knob also widens the step, from 1 to 5 semitones.
pub(crate) struct Climbing {
    input_shifter: PitchShifter,
    shifter: PitchShifter,
    tape: Tape,
    rising: bool,
    ratio: f32,
}

impl Climbing {
    pub(crate) fn new(config: &EffectConfig, rising: bool) -> Self {
        Self {
            input_shifter: PitchShifter::new(config.samples_for(SHIFT_WINDOW_SECONDS)),
            shifter: PitchShifter::new(config.samples_for(SHIFT_WINDOW_SECONDS)),
            tape: Tape::default(),
            rising,
            ratio: 1.0,
        }
    }
}

impl Voicing for Climbing {
    fn kind(&self) -> EffectKind {
        if self.rising {
            EffectKind::UpEcho
        } else {
            EffectKind::DownEcho
        }
    }

    fn feedback_for_depth(&self, depth: f32) -> f32 {
        feedback_for_depth(depth)
    }

    fn prepare(&mut self, sample_rate: f32) {
        self.tape.prepare(sample_rate);
        for shifter in [&mut self.input_shifter, &mut self.shifter] {
            shifter.set_window_samples(SHIFT_WINDOW_SECONDS * sample_rate);
        }
    }

    fn set_depth(&mut self, depth: f32) {
        let semitones =
            SMALLEST_STEP_SEMITONES + (LARGEST_STEP_SEMITONES - SMALLEST_STEP_SEMITONES) * depth;
        let signed = if self.rising { semitones } else { -semitones };
        self.ratio = (signed / 12.0).exp2();
    }

    #[inline]
    fn send(&mut self, input: Frame) -> Frame {
        self.input_shifter.tick(input, self.ratio)
    }

    #[inline]
    fn shape(&mut self, repeat: Frame) -> Frame {
        self.shifter.tick(self.tape.shape(repeat), self.ratio)
    }

    fn reset(&mut self) {
        self.tape.reset();
        self.input_shifter.clear();
        self.shifter.clear();
    }
}

/// The filter of a filtered echo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EchoFilter {
    /// Lows cut at a fixed 300 Hz: the Low Cut Echo.
    LowCut,
    /// Lows cut, rising from 150 Hz to 4 kHz along the depth knob: the HPF Echo.
    HighPass,
    /// Highs cut, falling from 12 kHz to 400 Hz along the depth knob: the LPF Echo.
    LowPass,
    /// A band moving from 300 Hz to 5 kHz along the depth knob: the BPF Echo.
    BandPass,
}

impl EchoFilter {
    fn range_hz(self) -> (f32, f32) {
        match self {
            Self::LowCut => (300.0, 300.0),
            Self::HighPass => (150.0, 4_000.0),
            Self::LowPass => (12_000.0, 400.0),
            Self::BandPass => (300.0, 5_000.0),
        }
    }

    #[inline]
    fn pick(self, output: SvfOutput) -> f32 {
        match self {
            Self::LowCut | Self::HighPass => output.high,
            Self::LowPass => output.low,
            Self::BandPass => output.band,
        }
    }
}

const FILTER_QUALITY: f32 = 0.9;
const CUTOFF_SMOOTHING_SECONDS: f32 = 0.02;
const FILTER_FULL_MIX_DEPTH: f32 = 0.25;
/// The mix that plays the echo at full level on top of the music.
const ON_TOP: f32 = 0.5;

/// Filters the music into the loop and every repeat on its way round, so each repeat comes back
/// thinner, darker or narrower: Low Cut Echo, HPF Echo, LPF Echo and BPF Echo.
///
/// The Low Cut Echo's knob balances the music and the echo. The other three bring the echo in
/// on top of the music over the first quarter of the knob and sweep their filter across the
/// whole of it, a build-up's sweep.
pub(crate) struct Filtered {
    filter: EchoFilter,
    input_filters: [Svf; 2],
    feedback_filters: [Svf; 2],
    tape: Tape,
    cutoff_octaves: Smoothed,
    coefficients: SvfCoefficients,
    sample_rate: f32,
}

impl Filtered {
    pub(crate) fn new(filter: EchoFilter) -> Self {
        let (open_hz, _) = filter.range_hz();
        Self {
            filter,
            input_filters: [Svf::default(); 2],
            feedback_filters: [Svf::default(); 2],
            tape: Tape::default(),
            cutoff_octaves: Smoothed::new(open_hz.log2(), CUTOFF_SMOOTHING_SECONDS),
            coefficients: SvfCoefficients::new(open_hz, FILTER_QUALITY, 48_000.0),
            sample_rate: 48_000.0,
        }
    }

    #[inline]
    fn advance_cutoff(&mut self) {
        let before = self.cutoff_octaves.current();
        let octaves = self.cutoff_octaves.next();
        if octaves != before {
            self.coefficients =
                SvfCoefficients::new(octaves.exp2(), FILTER_QUALITY, self.sample_rate);
        }
    }
}

impl Voicing for Filtered {
    fn kind(&self) -> EffectKind {
        match self.filter {
            EchoFilter::LowCut => EffectKind::LowCutEcho,
            EchoFilter::HighPass => EffectKind::HpfEcho,
            EchoFilter::LowPass => EffectKind::LpfEcho,
            EchoFilter::BandPass => EffectKind::BpfEcho,
        }
    }

    fn feedback_for_depth(&self, depth: f32) -> f32 {
        feedback_for_depth(depth)
    }

    fn mix_for_depth(&self, depth: f32) -> f32 {
        match self.filter {
            EchoFilter::LowCut => depth,
            EchoFilter::HighPass | EchoFilter::LowPass | EchoFilter::BandPass => {
                engaged(depth, FILTER_FULL_MIX_DEPTH) * ON_TOP
            }
        }
    }

    fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.tape.prepare(sample_rate);
        self.coefficients = SvfCoefficients::new(
            self.cutoff_octaves.current().exp2(),
            FILTER_QUALITY,
            sample_rate,
        );
        self.cutoff_octaves.prepare(sample_rate);
    }

    fn set_depth(&mut self, depth: f32) {
        let (open_hz, closed_hz) = self.filter.range_hz();
        self.cutoff_octaves
            .set_target(open_hz.log2() + (closed_hz.log2() - open_hz.log2()) * depth);
    }

    #[inline]
    fn send(&mut self, input: Frame) -> Frame {
        self.advance_cutoff();
        let filter = self.filter;
        let [left, right] = &mut self.input_filters;
        [
            filter.pick(left.tick(input[0], &self.coefficients)),
            filter.pick(right.tick(input[1], &self.coefficients)),
        ]
    }

    #[inline]
    fn shape(&mut self, repeat: Frame) -> Frame {
        let aged = self.tape.shape(repeat);
        let filter = self.filter;
        let [left, right] = &mut self.feedback_filters;
        [
            filter.pick(left.tick(aged[0], &self.coefficients)),
            filter.pick(right.tick(aged[1], &self.coefficients)),
        ]
    }

    fn reset(&mut self) {
        for filter in self
            .input_filters
            .iter_mut()
            .chain(&mut self.feedback_filters)
        {
            filter.reset();
        }
        self.tape.reset();
        self.cutoff_octaves.snap_to_target();
    }
}

const CRUSHED_BITS: f32 = 4.0;
const CLEAN_BITS: f32 = 12.0;
const CRUSHED_RATE_HZ: f32 = 2_000.0;
const CLEAN_RATE_HZ: f32 = 16_000.0;

/// Crushes the music into the loop and every repeat again, so the trail grows grittier on each
/// pass: the Crush Echo. The knob balances the music and the echo and crushes harder, from 12
/// bits at 16 kHz to 4 bits at 2 kHz.
pub(crate) struct Crushed {
    input_crush: SampleCrush,
    feedback_crush: SampleCrush,
    tape: Tape,
    levels: f32,
    rate_hz: f32,
    sample_rate: f32,
}

impl Crushed {
    pub(crate) fn new() -> Self {
        Self {
            input_crush: SampleCrush::default(),
            feedback_crush: SampleCrush::default(),
            tape: Tape::default(),
            levels: CLEAN_BITS.exp2(),
            rate_hz: CLEAN_RATE_HZ,
            sample_rate: 48_000.0,
        }
    }
}

impl Voicing for Crushed {
    fn kind(&self) -> EffectKind {
        EffectKind::CrushEcho
    }

    fn feedback_for_depth(&self, depth: f32) -> f32 {
        feedback_for_depth(depth)
    }

    fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.tape.prepare(sample_rate);
    }

    fn set_depth(&mut self, depth: f32) {
        let bits = CLEAN_BITS + (CRUSHED_BITS - CLEAN_BITS) * depth;
        self.levels = (bits - 1.0).exp2();
        self.rate_hz =
            (CLEAN_RATE_HZ.log2() + (CRUSHED_RATE_HZ.log2() - CLEAN_RATE_HZ.log2()) * depth).exp2();
    }

    #[inline]
    fn send(&mut self, input: Frame) -> Frame {
        let step = self.rate_hz / self.sample_rate;
        self.input_crush.tick(input, self.levels, step)
    }

    #[inline]
    fn shape(&mut self, repeat: Frame) -> Frame {
        let step = self.rate_hz / self.sample_rate;
        let aged = self.tape.shape(repeat);
        self.feedback_crush.tick(aged, self.levels, step)
    }

    fn reset(&mut self) {
        self.input_crush = SampleCrush::default();
        self.feedback_crush = SampleCrush::default();
        self.tape.reset();
    }
}

const NOISE_LOWEST_HZ: f32 = 300.0;
const NOISE_HIGHEST_HZ: f32 = 9_000.0;
const NOISE_QUALITY: f32 = 1.2;
const NOISE_LEVEL: f32 = 0.25;
const NOISE_FEEDBACK: f32 = 0.6;

/// White noise through a band-pass filter, echoed on the beat and laid over the music, which
/// keeps its own level: the Noise. The knob raises the noise and sweeps its band up from
/// 300 Hz to 9 kHz, a build-up's riser.
pub(crate) struct Noise {
    source: WhiteNoise,
    band: [Svf; 2],
    tape: Tape,
    enabled: bool,
    enabled_level: f32,
    level: Smoothed,
    centre_octaves: Smoothed,
    coefficients: SvfCoefficients,
    sample_rate: f32,
}

impl Noise {
    pub(crate) fn new() -> Self {
        Self {
            source: WhiteNoise::default(),
            band: [Svf::default(); 2],
            tape: Tape::default(),
            enabled: false,
            enabled_level: 0.0,
            level: Smoothed::new(0.0, CUTOFF_SMOOTHING_SECONDS),
            centre_octaves: Smoothed::new(NOISE_LOWEST_HZ.log2(), CUTOFF_SMOOTHING_SECONDS),
            coefficients: SvfCoefficients::new(NOISE_LOWEST_HZ, NOISE_QUALITY, 48_000.0),
            sample_rate: 48_000.0,
        }
    }

    fn update_band_coefficients(&mut self) {
        self.coefficients = SvfCoefficients::new(
            self.centre_octaves.current().exp2(),
            NOISE_QUALITY,
            self.sample_rate,
        );
    }
}

impl Voicing for Noise {
    const MIX_LAW: MixLaw = MixLaw::Layer;

    fn kind(&self) -> EffectKind {
        EffectKind::Noise
    }

    fn feedback_for_depth(&self, _depth: f32) -> f32 {
        NOISE_FEEDBACK
    }

    fn mix_for_depth(&self, _depth: f32) -> f32 {
        1.0
    }

    fn prepare(&mut self, sample_rate: f32) {
        if self.sample_rate != sample_rate {
            self.sample_rate = sample_rate;
            self.update_band_coefficients();
        }
        self.tape.prepare(sample_rate);
        self.level.prepare(sample_rate);
        self.centre_octaves.prepare(sample_rate);
    }

    fn set_depth(&mut self, depth: f32) {
        self.enabled_level = NOISE_LEVEL * depth.sqrt();
        if self.enabled {
            self.level.set_target(self.enabled_level);
        }
        self.centre_octaves.set_target(
            NOISE_LOWEST_HZ.log2() + (NOISE_HIGHEST_HZ.log2() - NOISE_LOWEST_HZ.log2()) * depth,
        );
    }

    fn switched_on(&mut self) {
        self.enabled = true;
        self.level.set_target(self.enabled_level);
    }

    fn switched_off(&mut self) {
        // This send generates its own noise, so fading the deck input does not stop it.
        self.enabled = false;
        self.level.set_target(0.0);
    }

    #[inline]
    fn send(&mut self, _input: Frame) -> Frame {
        let before = self.centre_octaves.current();
        let octaves = self.centre_octaves.next();
        if octaves != before {
            self.update_band_coefficients();
        }
        let level = self.level.next();
        let [left, right] = &mut self.band;
        [
            left.tick(self.source.next(), &self.coefficients).band * level,
            right.tick(self.source.next(), &self.coefficients).band * level,
        ]
    }

    #[inline]
    fn shape(&mut self, repeat: Frame) -> Frame {
        self.tape.shape(repeat)
    }

    fn reset(&mut self) {
        for filter in &mut self.band {
            filter.reset();
        }
        self.tape.reset();
        self.level.snap_to_target();
        self.centre_octaves.snap_to_target();
        self.update_band_coefficients();
    }
}

const DIFFUSER_MILLISECONDS: [[f32; 2]; 2] = [[5.3, 8.9], [5.9, 9.7]];
const DIFFUSER_GAIN: f32 = 0.6;
const SPIRAL_LOWEST_FEEDBACK: f32 = 0.55;
const SPIRAL_HIGHEST_FEEDBACK: f32 = 1.0;

/// Smears every repeat through two short all-pass diffusers, so the trail blooms into a reverb,
/// and lets the delay time glide when the beat changes, so the trail bends in pitch: the
/// Spiral. The knob balances the music and the spiral and raises the feedback until, fully
/// right, the trail no longer fades.
pub(crate) struct Diffused {
    diffusers: [[Allpass; 2]; 2],
    tape: Tape,
    shift: Option<Shift>,
}

/// The pitch step of a Spiral Up or Spiral Down.
struct Shift {
    input_shifter: PitchShifter,
    shifter: PitchShifter,
    rising: bool,
    ratio: f32,
}

const SPIRAL_SMALLEST_STEP_SEMITONES: f32 = 1.0;
const SPIRAL_LARGEST_STEP_SEMITONES: f32 = 3.0;

impl Diffused {
    /// A plain Spiral.
    pub(crate) fn spiral(config: &EffectConfig) -> Self {
        Self {
            diffusers: DIFFUSER_MILLISECONDS
                .map(|channel| channel.map(|milliseconds| Allpass::new(config, milliseconds))),
            tape: Tape::default(),
            shift: None,
        }
    }

    /// A Spiral whose repeats also climb, or fall, a step each pass: Spiral Up and Spiral Down.
    pub(crate) fn climbing(config: &EffectConfig, rising: bool) -> Self {
        Self {
            shift: Some(Shift {
                input_shifter: PitchShifter::new(config.samples_for(SHIFT_WINDOW_SECONDS)),
                shifter: PitchShifter::new(config.samples_for(SHIFT_WINDOW_SECONDS)),
                rising,
                ratio: 1.0,
            }),
            ..Self::spiral(config)
        }
    }
}

impl Voicing for Diffused {
    const GLIDES: bool = true;

    fn kind(&self) -> EffectKind {
        match &self.shift {
            None => EffectKind::Spiral,
            Some(Shift { rising: true, .. }) => EffectKind::SpiralUp,
            Some(Shift { rising: false, .. }) => EffectKind::SpiralDown,
        }
    }

    fn feedback_for_depth(&self, depth: f32) -> f32 {
        SPIRAL_LOWEST_FEEDBACK + (SPIRAL_HIGHEST_FEEDBACK - SPIRAL_LOWEST_FEEDBACK) * depth
    }

    fn prepare(&mut self, sample_rate: f32) {
        self.tape.prepare(sample_rate);
        for (channel, milliseconds) in self.diffusers.iter_mut().zip(DIFFUSER_MILLISECONDS) {
            for (diffuser, delay) in channel.iter_mut().zip(milliseconds) {
                diffuser.prepare(delay, sample_rate);
            }
        }
        if let Some(shift) = &mut self.shift {
            for shifter in [&mut shift.input_shifter, &mut shift.shifter] {
                shifter.set_window_samples(SHIFT_WINDOW_SECONDS * sample_rate);
            }
        }
    }

    fn set_depth(&mut self, depth: f32) {
        if let Some(shift) = &mut self.shift {
            let semitones = SPIRAL_SMALLEST_STEP_SEMITONES
                + (SPIRAL_LARGEST_STEP_SEMITONES - SPIRAL_SMALLEST_STEP_SEMITONES) * depth;
            let signed = if shift.rising { semitones } else { -semitones };
            shift.ratio = (signed / 12.0).exp2();
        }
    }

    #[inline]
    fn send(&mut self, input: Frame) -> Frame {
        match &mut self.shift {
            Some(shift) => shift.input_shifter.tick(input, shift.ratio),
            None => input,
        }
    }

    #[inline]
    fn shape(&mut self, repeat: Frame) -> Frame {
        let mut diffused = self.tape.shape(repeat);
        for (channel, sample) in self.diffusers.iter_mut().zip(&mut diffused) {
            for diffuser in channel {
                *sample = diffuser.tick(*sample, DIFFUSER_GAIN);
            }
        }
        match &mut self.shift {
            Some(shift) => shift.shifter.tick(diffused, shift.ratio),
            None => diffused,
        }
    }

    fn reset(&mut self) {
        for diffuser in self.diffusers.iter_mut().flatten() {
            diffuser.clear();
        }
        self.tape.reset();
        if let Some(shift) = &mut self.shift {
            shift.input_shifter.clear();
            shift.shifter.clear();
        }
    }
}

const HELIX_LOWEST_FEEDBACK: f32 = 0.6;
const HELIX_FREEZE_DEPTH: f32 = 0.98;
const HELIX_FREEZE_SECONDS: f32 = 0.004;

/// Records the music into the loop and repeats it every beat length without ageing it, laying
/// each new pass over the last; changing the beat length bends the recording's pitch: the
/// Helix. The knob sets how much of each pass stays, and fully right freezes the loop: it takes
/// no new music and repeats what it holds for as long as it plays.
pub(crate) struct Layered {
    frozen: Smoothed,
    freeze_requested: bool,
    enabled: bool,
}

impl Default for Layered {
    fn default() -> Self {
        Self {
            frozen: Smoothed::new(0.0, HELIX_FREEZE_SECONDS),
            freeze_requested: false,
            enabled: false,
        }
    }
}

impl Layered {
    fn update_freeze(&mut self) {
        self.frozen
            .set_target(if self.enabled && self.freeze_requested {
                1.0
            } else {
                0.0
            });
    }
}

impl Voicing for Layered {
    const GLIDES: bool = true;

    fn kind(&self) -> EffectKind {
        EffectKind::Helix
    }

    fn feedback_for_depth(&self, depth: f32) -> f32 {
        HELIX_LOWEST_FEEDBACK + (1.0 - HELIX_LOWEST_FEEDBACK) * depth
    }

    fn prepare(&mut self, sample_rate: f32) {
        self.frozen.prepare(sample_rate);
    }

    fn set_depth(&mut self, depth: f32) {
        self.freeze_requested = depth >= HELIX_FREEZE_DEPTH;
        self.update_freeze();
    }

    fn switched_on(&mut self) {
        self.enabled = true;
        self.update_freeze();
    }

    fn switched_off(&mut self) {
        self.enabled = false;
        self.update_freeze();
    }

    fn frozen(&self) -> f32 {
        self.frozen.current()
    }

    #[inline]
    fn send(&mut self, input: Frame) -> Frame {
        let open = 1.0 - self.frozen.next();
        input.map(|sample| sample * open)
    }

    #[inline]
    fn shape(&mut self, repeat: Frame) -> Frame {
        repeat
    }

    fn reset(&mut self) {
        self.frozen.snap_to_target();
    }
}

const OUT_SHORTEST_FEEDBACK: f32 = 0.55;
const OUT_LONGEST_FEEDBACK: f32 = 0.85;
const OUT_EDGE_SECONDS: f32 = 0.003;

/// Takes in one beat length of the music from the moment it is switched on, then cuts the music
/// and lets the echoes of that last beat length fade away: the Echo Out, the afro house exit,
/// and rekordbox's Echo Release FX. The knob sets how long the echoes last.
#[derive(Default)]
pub(crate) struct Outgoing {
    tape: Tape,
    taken: f32,
    window: f32,
    edge_samples: f32,
    external_capture: bool,
}

impl Voicing for Outgoing {
    fn kind(&self) -> EffectKind {
        EffectKind::EchoOut
    }

    fn feedback_for_depth(&self, depth: f32) -> f32 {
        OUT_SHORTEST_FEEDBACK + (OUT_LONGEST_FEEDBACK - OUT_SHORTEST_FEEDBACK) * depth
    }

    fn mix_for_depth(&self, _depth: f32) -> f32 {
        1.0
    }

    fn prepare(&mut self, sample_rate: f32) {
        self.tape.prepare(sample_rate);
        self.edge_samples = (OUT_EDGE_SECONDS * sample_rate).max(1.0);
    }

    fn set_loop_samples(&mut self, samples: f32) {
        if self.taken == 0.0 {
            self.window = samples;
        }
    }

    fn switched_on(&mut self) {
        self.taken = 0.0;
    }

    fn set_external_capture(&mut self, capture: bool) {
        self.external_capture = capture;
    }

    #[inline]
    fn dry_passthrough(&self) -> f32 {
        // Keep the live music audible until the first repeat begins to fade in.
        let past_capture = (self.taken - self.window).max(0.0);
        (1.0 - past_capture / self.edge_samples.max(1.0)).max(0.0)
    }

    #[inline]
    fn send(&mut self, input: Frame) -> Frame {
        if self.external_capture {
            return input;
        }
        let captured = self.taken;
        self.taken = (captured + 1.0).min(self.window + self.edge_samples);
        if captured >= self.window {
            return [0.0; 2];
        }
        let edge = (captured.min(self.window - captured) / self.edge_samples.max(1.0)).min(1.0);
        input.map(|sample| sample * edge)
    }

    #[inline]
    fn shape(&mut self, repeat: Frame) -> Frame {
        self.tape.shape(repeat)
    }

    fn reset(&mut self) {
        self.tape.reset();
        self.taken = 0.0;
    }
}
