use dubplate_fx::{EqBand, Isolator, SweepFilter};

use crate::curves::eq_gain;
use crate::processor::{ProcessContext, StereoBlock};
use crate::smart_cfx::{SmartCfx, SmartCfxRack};

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
    eq_positions: [f32; 3],
    automatic_low: Option<f32>,
    filter: SweepFilter,
    filter_knob: f32,
    smart_cfx: SmartCfxRack,
    filter_context: dubplate_fx::ProcessContext,
}

impl ChannelStrip {
    /// Builds a flat strip for `sample_rate`. This allocates, so it runs before the renderer
    /// reaches the audio thread.
    pub(crate) fn new(sample_rate: u32) -> Self {
        let mut filter = SweepFilter::new();
        filter.set_resonance(DEFAULT_RESONANCE);
        filter.reset();
        Self {
            isolator: Isolator::new(sample_rate as f32),
            eq_positions: [KNOB_CENTRE; 3],
            automatic_low: None,
            filter,
            filter_knob: KNOB_CENTRE,
            smart_cfx: SmartCfxRack::new(sample_rate),
            filter_context: dubplate_fx::ProcessContext::new(sample_rate as f32, 0.0, 0.0),
        }
    }

    pub(crate) fn latency_frames(&self) -> usize {
        self.isolator.latency_frames()
    }

    pub(crate) fn set_eq(&mut self, band: EqBand, position: f32) {
        self.eq_positions[band.index()] = position;
        self.apply_eq(band);
    }

    /// Lets Smart Fader turn the LOW knob, or gives it back with `None`. A LOW knob the DJ has
    /// turned away from its centre always wins.
    pub(crate) fn set_automatic_low(&mut self, position: Option<f32>) {
        self.automatic_low = position;
        self.apply_eq(EqBand::Low);
    }

    fn apply_eq(&mut self, band: EqBand) {
        let manual = self.eq_positions[band.index()];
        let position = match (band, self.automatic_low) {
            (EqBand::Low, Some(automatic)) if manual == KNOB_CENTRE => automatic,
            _ => manual,
        };
        self.isolator.set_gain(band, eq_gain(position));
    }

    pub(crate) fn set_filter(&mut self, position: f32) {
        self.filter_knob = position;
        self.smart_cfx.set_knob(position);
        self.apply_filter();
    }

    pub(crate) fn set_smart_cfx(&mut self, preset: Option<SmartCfx>) {
        self.smart_cfx.select(preset);
        self.apply_filter();
    }

    /// Turns the filter with the knob, or holds it open while a preset plays without it.
    fn apply_filter(&mut self) {
        let position = if self.smart_cfx.filters() {
            self.filter_knob * 2.0 - 1.0
        } else {
            0.0
        };
        self.filter.set_position(position);
    }

    pub(crate) fn set_filter_resonance(&mut self, resonance: f32) {
        self.filter.set_resonance(resonance);
    }

    /// Runs the EQ, then the filter with any Smart CFX effect around it.
    pub(crate) fn process(&mut self, block: &mut StereoBlock, context: &ProcessContext) {
        let (left, right) = block.channels_mut();
        self.isolator.process(left, right);
        self.smart_cfx.process(block, context, true);
        let (left, right) = block.channels_mut();
        self.filter.process(left, right, &self.filter_context);
        self.smart_cfx.process(block, context, false);
    }
}
