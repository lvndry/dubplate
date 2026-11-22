use std::f32::consts::PI;

use super::delay_line::{DelayLine, MIN_DELAY_SAMPLES};

/// A low-latency pitch shifter: two read heads sweep through a short delay line, half a window
/// apart, each faded in and out with a raised-cosine window so their sum stays at unity.
///
/// Its latency is half a window on average, a few tens of milliseconds, which keeps the
/// shifted sound tight enough to blend with the dry music and to run inside an echo's feedback
/// loop. Its sound is the classic one of DJ mixers' pitch effects: clean on sustained sounds,
/// lightly smeared on sharp transients.
pub(crate) struct PitchShifter {
    lines: [DelayLine; 2],
    phase: f32,
    window_samples: f32,
}

impl PitchShifter {
    /// Builds a shifter whose window can be up to `max_window_samples` long. This allocates.
    pub(crate) fn new(max_window_samples: usize) -> Self {
        let capacity = max_window_samples + MIN_DELAY_SAMPLES as usize + 4;
        Self {
            lines: [DelayLine::new(capacity), DelayLine::new(capacity)],
            phase: 0.0,
            window_samples: max_window_samples as f32,
        }
    }

    /// Sets the window, clamped to what the lines hold.
    pub(crate) fn set_window_samples(&mut self, samples: f32) {
        let longest = self.lines[0].max_delay() - MIN_DELAY_SAMPLES - 1.0;
        self.window_samples = samples.clamp(MIN_DELAY_SAMPLES, longest);
    }

    pub(crate) fn clear(&mut self) {
        for line in &mut self.lines {
            line.clear();
        }
        self.phase = 0.0;
    }

    /// Shifts one stereo frame by `ratio` (2 is an octave up, 0.5 an octave down).
    #[inline]
    pub(crate) fn tick(&mut self, input: [f32; 2], ratio: f32) -> [f32; 2] {
        let window = self.window_samples;
        self.phase = (self.phase + (1.0 - ratio) / window).rem_euclid(1.0);
        let heads = [self.phase, (self.phase + 0.5).rem_euclid(1.0)];
        let mut output = [0.0; 2];
        for (line, (channel_input, channel_output)) in
            self.lines.iter_mut().zip(input.iter().zip(&mut output))
        {
            for head in heads {
                let gain = (PI * head).sin();
                let delay = MIN_DELAY_SAMPLES + head * window;
                *channel_output += line.read_linear(delay) * gain * gain;
            }
            line.write(*channel_input);
        }
        output
    }
}
