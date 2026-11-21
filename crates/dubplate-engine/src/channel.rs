use dubplate_fx::{Effect, EqBand, Isolator, SweepFilter};

use crate::curves::eq_gain;
use crate::processor::StereoBlock;

/// The centre of a mixer knob, where an EQ band is at unity and the filter is flat.
pub(crate) const KNOB_CENTRE: f32 = 0.5;
const DEFAULT_RESONANCE: f32 = 0.35;

/// The knobs of one mixer channel, as the control side last set them. Every knob runs from 0
/// to 1 with its centre at 0.5, like the controller's own knobs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChannelSettings {
    /// HI, MID and LOW EQ knobs, in the order of [`EqBand::ALL`]: 0 kills the band, 0.5 leaves
    /// it untouched and 1 boosts it by 6 dB.
    pub eq: [f32; 3],
    /// The filter knob (CFX on the DDJ-FLX4): low-pass to the left, high-pass to the right,
    /// flat at 0.5.
    pub filter: f32,
    /// How much the filter sings at its cutoff as the knob leaves the centre, from 0 to 1.
    pub filter_resonance: f32,
}

impl ChannelSettings {
    /// The position of one EQ knob.
    pub fn eq(&self, band: EqBand) -> f32 {
        self.eq[band.index()]
    }
}

impl Default for ChannelSettings {
    fn default() -> Self {
        Self {
            eq: [KNOB_CENTRE; 3],
            filter: KNOB_CENTRE,
            filter_resonance: DEFAULT_RESONANCE,
        }
    }
}

/// The audio-thread side of a mixer channel's EQ and filter, in the order of a DJM: the
/// isolator EQ, then the filter.
pub(crate) struct ChannelStrip {
    isolator: Isolator,
    filter: SweepFilter,
    filter_context: dubplate_fx::ProcessContext,
}

impl ChannelStrip {
    /// Builds a flat strip for `sample_rate`. This allocates, so it runs before the renderer
    /// reaches the audio thread.
    pub(crate) fn new(sample_rate: u32) -> Self {
        let mut filter = SweepFilter::new();
        filter.set_resonance(DEFAULT_RESONANCE);
        filter.set_enabled(true);
        filter.reset();
        Self {
            isolator: Isolator::new(sample_rate as f32),
            filter,
            filter_context: dubplate_fx::ProcessContext::new(sample_rate as f32, 0.0, 0.0),
        }
    }

    pub(crate) fn latency_frames(&self) -> usize {
        self.isolator.latency_frames()
    }

    pub(crate) fn set_eq(&mut self, band: EqBand, position: f32) {
        self.isolator.set_gain(band, eq_gain(position));
    }

    pub(crate) fn set_filter(&mut self, position: f32) {
        self.filter.set_position(position * 2.0 - 1.0);
    }

    pub(crate) fn set_filter_resonance(&mut self, resonance: f32) {
        self.filter.set_resonance(resonance);
    }

    pub(crate) fn process(&mut self, block: &mut StereoBlock) {
        let (left, right) = block.channels_mut();
        self.isolator.process(left, right);
        self.filter.process(left, right, &self.filter_context);
    }
}
