use std::sync::Arc;

use signalsmith_stretch::Stretch;

use crate::MAX_BLOCK_FRAMES;
use crate::curves::Ramp;
use crate::looping::LoopRegion;
use crate::mixer::RetireQueue;
use crate::track::TrackBuffer;
use crate::voice::Voice;

const CHANNELS: usize = 2;
/// The most input one block can take: a deck at four times its tempo with a full nudge.
const MAX_INPUT_FRAMES: usize = MAX_BLOCK_FRAMES * 8;
/// How long key lock takes to fade in or out against the plain resampled sound.
const MIX_SECONDS: f64 = 0.02;

/// What a deck asks of its key lock for one block.
#[derive(Clone, Copy)]
pub(crate) struct KeyLockBlock<'track> {
    /// Whether the stretched sound should play: key lock is on and the record turns on its own.
    pub(crate) wanted: bool,
    pub(crate) track: Option<&'track Arc<TrackBuffer>>,
    /// The playhead at the start of the block, in track frames.
    pub(crate) playhead: f64,
    /// How far the playhead moves over the block, in track frames.
    pub(crate) advance: f64,
    pub(crate) frames: usize,
    /// Track frames per output frame at the track's own speed: the sample rate conversion.
    pub(crate) unit_step: f64,
    pub(crate) active_loop: Option<LoopRegion>,
}

/// Key lock, the master tempo of a CDJ: the deck changes tempo without changing pitch.
///
/// A Signalsmith Stretch time-stretcher plays the track at the deck's speed while keeping its
/// pitch. Its preset that spreads each spectral frame's work over the frame interval keeps
/// every block's cost close to the average, so no audio callback runs long. It needs to hear the music ahead of the time it plays, so a feeder reads the track
/// ahead of the playhead by the stretcher's latency, going round loops as the playhead will;
/// the stretched sound then lines up with the playhead frame for frame. After a jump the
/// stretcher is primed at the new place at once; until its sound is whole, the deck plays the
/// plain resampled sound there, so the first beat after a hot cue keeps its attack, then glides
/// into the stretched sound. Turning key lock on or off crossfades with the plain sound. Everything is allocated up front, so the audio thread never
/// allocates.
pub(crate) struct KeyLock {
    stretch: Stretch,
    feeder: Voice,
    input: Vec<f32>,
    output: Vec<f32>,
    priming: Vec<f32>,
    input_latency: f64,
    output_latency: f64,
    engaged: bool,
    needs_priming: bool,
    lead: f64,
    owed_frames: f64,
    mix: Ramp,
    frames_ready: usize,
    hold_frames: usize,
}

impl KeyLock {
    /// Builds the stretcher for `output_rate`. This allocates, so it runs before the renderer
    /// reaches the audio thread.
    pub(crate) fn new(output_rate: u32, declick_frames: u32) -> Self {
        let mut stretch = Stretch::preset_cheaper(CHANNELS as u32, output_rate);
        let input_latency = stretch.input_latency();
        let output_latency = stretch.output_latency();
        let priming_frames = input_latency * 2 + output_latency;
        let priming = vec![0.0; priming_frames * CHANNELS];
        let input = vec![0.0; MAX_INPUT_FRAMES * CHANNELS];
        let mut output = vec![0.0; MAX_BLOCK_FRAMES * CHANNELS];
        stretch.seek(&priming, 1.0);
        stretch.process(&input, &mut output);
        stretch.reset();
        let mix_frames = (f64::from(output_rate) * MIX_SECONDS).round().max(1.0) as u32;
        let mut feeder = Voice::new(declick_frames);
        feeder.transport.jump_to(1.0);
        Self {
            stretch,
            feeder,
            input,
            output,
            priming,
            input_latency: input_latency as f64,
            output_latency: output_latency as f64,
            engaged: false,
            needs_priming: true,
            lead: 0.0,
            owed_frames: 0.0,
            mix: Ramp::new(0.0, mix_frames),
            frames_ready: 0,
            hold_frames: 0,
        }
    }

    /// Asks for the stretcher to start again at the playhead, after it jumped.
    pub(crate) fn jumped(&mut self) {
        self.needs_priming = true;
    }

    /// Stops the stretched sound at once, for a new track.
    pub(crate) fn stop(&mut self, retire: &mut RetireQueue) {
        self.engaged = false;
        self.needs_priming = true;
        self.mix.jump_to(0.0);
        if let Some(track) = self.feeder.track.take() {
            retire.track(track);
        }
    }

    /// Whether the stretched sound is fully in, so the deck's own reads are not heard.
    pub(crate) fn is_fully_in(&self) -> bool {
        self.frames_ready > 0 && self.mix.current() >= 1.0 && self.mix.target() >= 1.0
    }

    /// Stretches the block's audio when key lock is wanted or still fading out. Returns whether
    /// the block has stretched frames to mix in.
    pub(crate) fn prepare(
        &mut self,
        block: KeyLockBlock<'_>,
        retire: &mut RetireQueue,
        starved: &mut bool,
    ) -> bool {
        self.frames_ready = 0;
        if block.wanted && !self.engaged {
            self.engaged = true;
            self.needs_priming = true;
        }
        let stretched_sound_ready = self.hold_frames == 0 && !self.needs_priming;
        let mix_target = if block.wanted && stretched_sound_ready {
            1.0
        } else {
            0.0
        };
        if self.mix.target() != mix_target {
            self.mix.set_target(mix_target);
        }
        if !block.wanted && self.mix.current() <= 0.0 {
            self.engaged = false;
        }
        let Some(track) = block.track.filter(|_| self.engaged) else {
            return false;
        };
        let frames = block.frames.min(MAX_BLOCK_FRAMES);
        let unit_step = block.unit_step.max(f64::MIN_POSITIVE);
        let rate = (block.advance / frames.max(1) as f64).max(0.0);
        let lead = self.output_latency * rate + self.input_latency * unit_step;
        if self.needs_priming {
            self.prime(track, block, lead, rate / unit_step, retire, starved);
        } else {
            self.owed_frames += (block.advance + lead - self.lead) / unit_step;
        }
        self.lead = lead;
        let count = (self.owed_frames.floor().max(0.0) as usize).min(MAX_INPUT_FRAMES);
        self.owed_frames -= count as f64;
        let input = &mut self.input[..count * CHANNELS];
        read_ahead(
            &mut self.feeder,
            input,
            unit_step,
            block.active_loop,
            retire,
            starved,
        );
        self.stretch.process(
            &self.input[..count * CHANNELS],
            &mut self.output[..frames * CHANNELS],
        );
        self.hold_frames = self.hold_frames.saturating_sub(frames);
        self.frames_ready = frames;
        true
    }

    /// Starts the stretcher so its sound lines up with the playhead from the next frame on:
    /// the feeder backs up to read the stretcher's look-back window, ending as far ahead of the
    /// playhead as the stretcher's latency.
    fn prime(
        &mut self,
        track: &Arc<TrackBuffer>,
        block: KeyLockBlock<'_>,
        lead: f64,
        playback_rate: f64,
        retire: &mut RetireQueue,
        starved: &mut bool,
    ) {
        let previous = self.feeder.track.replace(Arc::clone(track));
        if let Some(previous) = previous {
            retire.track(previous);
        }
        let priming_frames = self.priming.len() / CHANNELS;
        self.feeder.position = block.playhead + lead - priming_frames as f64 * block.unit_step;
        self.feeder.transport.jump_to(1.0);
        read_ahead(
            &mut self.feeder,
            &mut self.priming,
            block.unit_step,
            None,
            retire,
            starved,
        );
        self.stretch.seek(&self.priming, playback_rate.max(0.01));
        self.owed_frames = block.advance / block.unit_step;
        self.needs_priming = false;
        self.mix.jump_to(0.0);
        self.hold_frames = self.output_latency as usize;
    }

    /// The playhead's frame `index` of the block: the stretched sound over the plain resampled
    /// one from `voice`, in the share the key lock fade has reached.
    #[inline]
    pub(crate) fn playhead_frame(
        &mut self,
        index: usize,
        voice: &Voice,
        starved: &mut bool,
    ) -> (f32, f32) {
        let mix = self.mix.next_value();
        let stretched_left = self.output[index * CHANNELS];
        let stretched_right = self.output[index * CHANNELS + 1];
        let (plain_left, plain_right) = if mix < 1.0 {
            voice.read_playhead(starved)
        } else {
            (0.0, 0.0)
        };
        (
            plain_left + (stretched_left - plain_left) * mix,
            plain_right + (stretched_right - plain_right) * mix,
        )
    }
}

/// Reads frames at the track's own speed into interleaved `input`, going round `active_loop`
/// with a crossfade where the playhead will.
fn read_ahead(
    feeder: &mut Voice,
    input: &mut [f32],
    unit_step: f64,
    active_loop: Option<LoopRegion>,
    retire: &mut RetireQueue,
    starved: &mut bool,
) {
    for [left, right] in input.as_chunks_mut::<CHANNELS>().0 {
        (*left, *right) = feeder.next_frame(1.0, retire, starved);
        let previous = feeder.position;
        feeder.position += unit_step;
        if let Some(wrapped) = active_loop.and_then(|region| region.wrap(previous, feeder.position))
        {
            feeder.jump(wrapped, unit_step, retire);
        }
    }
}
