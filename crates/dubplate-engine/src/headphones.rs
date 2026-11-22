use crate::DECK_COUNT;
use crate::curves::{Ramp, channel_fader_gain, crossfader_gains};
use crate::deck::DeckId;
use crate::processor::StereoBlock;

const DEFAULT_MIX: f32 = 0.5;
const DEFAULT_LEVEL: f32 = 0.5;

/// The headphone section of a DJ mixer, as the control side last set it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeadphoneSettings {
    /// Each channel's CUE button: the channel's sound, before its fader, goes to the
    /// headphones.
    pub cue: [bool; DECK_COUNT],
    /// The MASTER CUE button: the master mix goes to the headphones' cue side too.
    pub master_cue: bool,
    /// The MIXING knob, from 0.0 (cue only) to 1.0 (master only).
    pub mix: f32,
    /// The LEVEL knob, from 0.0 (silent) to 1.0 (full).
    pub level: f32,
}

impl HeadphoneSettings {
    /// Whether a channel's CUE button is on.
    pub fn is_cued(&self, deck: DeckId) -> bool {
        self.cue[deck.index()]
    }
}

impl Default for HeadphoneSettings {
    fn default() -> Self {
        Self {
            cue: [false; DECK_COUNT],
            master_cue: false,
            mix: DEFAULT_MIX,
            level: DEFAULT_LEVEL,
        }
    }
}

/// The audio-thread side of the headphone section: the cue bus and its mix with the master.
pub(crate) struct Headphones {
    settings: HeadphoneSettings,
    cue_gain: Ramp,
    master_gain: Ramp,
    level: Ramp,
    cue_block: StereoBlock,
    pub(crate) block: StereoBlock,
}

impl Headphones {
    pub(crate) fn new(max_block_frames: usize, declick_frames: u32) -> Self {
        let settings = HeadphoneSettings::default();
        let (cue_gain, master_gain) = crossfader_gains(settings.mix);
        Self {
            settings,
            cue_gain: Ramp::new(cue_gain, declick_frames),
            master_gain: Ramp::new(master_gain, declick_frames),
            level: Ramp::new(channel_fader_gain(settings.level), declick_frames),
            cue_block: StereoBlock::with_capacity(max_block_frames),
            block: StereoBlock::with_capacity(max_block_frames),
        }
    }

    pub(crate) fn set_cue(&mut self, deck: DeckId, enabled: bool) {
        self.settings.cue[deck.index()] = enabled;
    }

    pub(crate) fn set_master_cue(&mut self, enabled: bool) {
        self.settings.master_cue = enabled;
    }

    pub(crate) fn set_mix(&mut self, mix: f32) {
        self.settings.mix = mix;
        let (cue_gain, master_gain) = crossfader_gains(mix);
        self.cue_gain.set_target(cue_gain);
        self.master_gain.set_target(master_gain);
    }

    pub(crate) fn set_level(&mut self, level: f32) {
        self.settings.level = level;
        self.level.set_target(channel_fader_gain(level));
    }

    /// Starts a block with an empty cue bus.
    pub(crate) fn begin_block(&mut self, frame_count: usize) {
        self.cue_block.set_frame_count(frame_count);
        self.cue_block.silence();
        self.block.set_frame_count(frame_count);
    }

    /// Adds a channel's sound before its fader to the cue bus when its CUE button is on.
    pub(crate) fn listen(&mut self, deck: DeckId, pre_fader: &StereoBlock) {
        if self.settings.is_cued(deck) {
            self.cue_block.add(pre_fader);
        }
    }

    /// Adds a source whose own CUE button is on, such as the sampler, to the cue bus.
    pub(crate) fn listen_to(&mut self, block: &StereoBlock) {
        self.cue_block.add(block);
    }

    /// Mixes the cue bus with `master` into the headphone block, with `preview` added after
    /// the cue mix and before the level.
    pub(crate) fn finish(&mut self, master: &StereoBlock, preview: &StereoBlock) {
        if self.settings.master_cue {
            self.cue_block.add(master);
        }
        let (left_out, right_out) = self.block.channels_mut();
        let frames = left_out
            .iter_mut()
            .zip(right_out.iter_mut())
            .zip(self.cue_block.left().iter().zip(self.cue_block.right()))
            .zip(master.left().iter().zip(master.right()))
            .zip(preview.left().iter().zip(preview.right()));
        for (
            (((left, right), (cue_left, cue_right)), (master_left, master_right)),
            preview_frame,
        ) in frames
        {
            let (preview_left, preview_right) = preview_frame;
            let cue_gain = self.cue_gain.next_value();
            let master_gain = self.master_gain.next_value();
            let level = self.level.next_value();
            *left = (cue_left * cue_gain + master_left * master_gain + preview_left) * level;
            *right = (cue_right * cue_gain + master_right * master_gain + preview_right) * level;
        }
    }
}
