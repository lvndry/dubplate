use crate::config::EffectConfig;
use crate::dsp::delay_line::{DelayLine, MIN_DELAY_SAMPLES};
use crate::unit::Frame;

/// A record of the last few seconds of music and a stylus that can play it at any speed.
///
/// The record keeps recording while the stylus slows, stops or spins backwards, so the stylus
/// falls behind the live music by the distance it has lost.
pub(crate) struct Turntable {
    lines: [DelayLine; 2],
    lag: f64,
}

impl Turntable {
    pub(crate) fn new(config: &EffectConfig) -> Self {
        let capacity = config.max_delay_samples();
        Self {
            lines: [DelayLine::new(capacity), DelayLine::new(capacity)],
            lag: f64::from(MIN_DELAY_SAMPLES),
        }
    }

    /// The most samples the stylus can fall behind.
    pub(crate) fn capacity(&self) -> f64 {
        f64::from(self.lines[0].max_delay()) - 4.0
    }

    pub(crate) fn drop_stylus(&mut self) {
        self.lag = f64::from(MIN_DELAY_SAMPLES);
    }

    /// Plays one frame at `speed` times normal, then records `input`.
    #[inline]
    pub(crate) fn play(&mut self, input: Frame, speed: f64) -> Frame {
        let lag = self.lag as f32;
        let output = [self.lines[0].read_cubic(lag), self.lines[1].read_cubic(lag)];
        self.record(input);
        self.lag = (self.lag + 1.0 - speed).clamp(f64::from(MIN_DELAY_SAMPLES), self.capacity());
        output
    }

    #[inline]
    pub(crate) fn record(&mut self, input: Frame) {
        self.lines[0].write(input[0]);
        self.lines[1].write(input[1]);
    }

    pub(crate) fn clear(&mut self) {
        for line in &mut self.lines {
            line.clear();
        }
        self.drop_stylus();
    }
}

/// Where a stop effect is in its motion.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Motion {
    /// Playing along with the live music, waiting to be triggered.
    Waiting,
    /// Triggered; starts on the next sample.
    Starting,
    /// Moving, `elapsed` samples into a motion `duration` samples long.
    Moving { elapsed: f64, duration: f64 },
    /// Come to rest; silent until switched off.
    Stopped,
}
