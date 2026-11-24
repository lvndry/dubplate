use std::sync::Arc;

use signalsmith_stretch::Stretch;

use crate::MAX_BLOCK_FRAMES;
use crate::curves::Ramp;
use crate::looping::LoopRegion;
use crate::mixer::RetireQueue;
use crate::stems::{PART_CHANNELS, mix_parts};
use crate::track::TrackBuffer;
use crate::voice::{StemGains, Voice};

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
    /// The deck plays the track's stems, so they and the track are stretched side by side and
    /// mixed after.
    pub(crate) stems: bool,
}

/// A time-stretcher with its buffers, for stereo or for every stem side by side.
struct Stretcher {
    stretch: Stretch,
    channels: usize,
    input: Vec<f32>,
    output: Vec<f32>,
    priming: Vec<f32>,
}

impl Stretcher {
    /// Builds and warms up a stretcher of `channels` channels. This allocates.
    fn new(channels: usize, output_rate: u32) -> Self {
        let mut stretch = Stretch::preset_default(channels as u32, output_rate);
        let priming_frames = stretch.input_latency() * 2 + stretch.output_latency();
        let priming = vec![0.0; priming_frames * channels];
        let input = vec![0.0; MAX_INPUT_FRAMES * channels];
        let mut output = vec![0.0; MAX_BLOCK_FRAMES * channels];
        stretch.seek(&priming, 1.0);
        stretch.process(&input, &mut output);
        stretch.reset();
        Self {
            stretch,
            channels,
            input,
            output,
            priming,
        }
    }

    fn priming_frames(&self) -> usize {
        self.priming.len() / self.channels
    }
}

/// Key lock, the master tempo of a CDJ: the deck changes tempo without changing pitch.
///
/// A Signalsmith Stretch time-stretcher plays the track at the deck's speed while keeping its
/// pitch. Its default preset overlaps its spectral frames four times, which keeps a key
/// shifted pitch true to a fraction of a cent. It needs to hear the music ahead of the time it plays, so a feeder reads the track
/// ahead of the playhead by the stretcher's latency, going round loops as the playhead will;
/// the stretched sound then lines up with the playhead frame for frame. After a jump the
/// stretcher is primed at the new place at once; until its sound is whole, the deck plays the
/// plain resampled sound there, so the first beat after a hot cue keeps its attack, then glides
/// into the stretched sound. Turning key lock on or off crossfades with the plain sound. Everything is allocated up front, so the audio thread never
/// allocates.
pub(crate) struct KeyLock {
    stereo: Stretcher,
    stems: Option<Stretcher>,
    stretching_stems: bool,
    feeder: Voice,
    input_latency: f64,
    output_latency: f64,
    engaged: bool,
    needs_priming: bool,
    loop_bounds: Option<(f64, f64)>,
    lead: f64,
    owed_frames: f64,
    mix: Ramp,
    frames_ready: usize,
    hold_frames: usize,
    transpose_semitones: f32,
}

impl KeyLock {
    /// Builds the stereo stretcher for `output_rate`. This allocates, so it runs before the
    /// renderer reaches the audio thread.
    pub(crate) fn new(output_rate: u32, declick_frames: u32) -> Self {
        Self::build(output_rate, declick_frames, None)
    }

    /// Builds the stereo stretcher and one for a track's stems side by side, for a deck. This
    /// allocates, so it runs before the renderer reaches the audio thread.
    pub(crate) fn with_stems(output_rate: u32, declick_frames: u32) -> Self {
        Self::build(
            output_rate,
            declick_frames,
            Some(Stretcher::new(PART_CHANNELS, output_rate)),
        )
    }

    fn build(output_rate: u32, declick_frames: u32, stems: Option<Stretcher>) -> Self {
        let stereo = Stretcher::new(CHANNELS, output_rate);
        let input_latency = stereo.stretch.input_latency();
        let output_latency = stereo.stretch.output_latency();
        let mix_frames = (f64::from(output_rate) * MIX_SECONDS).round().max(1.0) as u32;
        let mut feeder = Voice::new(declick_frames);
        feeder.transport.jump_to(1.0);
        Self {
            stereo,
            stems,
            stretching_stems: false,
            feeder,
            input_latency: input_latency as f64,
            output_latency: output_latency as f64,
            engaged: false,
            needs_priming: true,
            loop_bounds: None,
            lead: 0.0,
            owed_frames: 0.0,
            mix: Ramp::new(0.0, mix_frames),
            frames_ready: 0,
            hold_frames: 0,
            transpose_semitones: 0.0,
        }
    }

    /// The feeder and the stretcher in use: the stems' while the deck plays stems, the stereo
    /// one otherwise.
    fn feeder_and_stretcher(&mut self) -> (&mut Voice, &mut Stretcher) {
        let stretcher = match &mut self.stems {
            Some(stems) if self.stretching_stems => stems,
            _ => &mut self.stereo,
        };
        (&mut self.feeder, stretcher)
    }

    /// Shifts the stretched sound's pitch by `semitones`, for key shift, or to follow the tempo
    /// when key lock is off.
    pub(crate) fn set_transpose(&mut self, semitones: f32) {
        if semitones != self.transpose_semitones {
            self.transpose_semitones = semitones;
            self.stereo
                .stretch
                .set_transpose_factor_semitones(semitones, None);
            if let Some(stems) = &mut self.stems {
                stems
                    .stretch
                    .set_transpose_factor_semitones(semitones, None);
            }
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

    /// Whether the stretcher is running, or fading out.
    pub(crate) fn is_engaged(&self) -> bool {
        self.engaged
    }

    /// Whether the stretched sound is fully in, so the deck's own reads are not heard.
    pub(crate) fn is_fully_in(&self) -> bool {
        self.frames_ready > 0 && self.mix.current() >= 1.0 && self.mix.target() >= 1.0
    }

    /// Stretches the block's audio when key lock is wanted or still fading out. Returns whether
    /// the block has stretched frames to mix in.
    pub(crate) fn prepare(
        &mut self,
        mut block: KeyLockBlock<'_>,
        retire: &mut RetireQueue,
        starved: &mut bool,
    ) -> bool {
        self.frames_ready = 0;
        if block.wanted && !self.engaged {
            self.engaged = true;
            self.needs_priming = true;
        }
        let stretching_stems = block.stems && self.stems.is_some();
        if stretching_stems != self.stretching_stems {
            self.stretching_stems = stretching_stems;
            self.needs_priming = true;
        }
        // A playhead already beyond the loop plays forward without wrapping into it.
        block.active_loop = block
            .active_loop
            .filter(|region| region.start < region.end && block.playhead < region.end);
        let loop_bounds = block.active_loop.map(|region| (region.start, region.end));
        if self.loop_bounds != loop_bounds {
            // The stretcher may already hold audio past the new boundary.
            self.loop_bounds = loop_bounds;
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
        let stretching_stems = self.stretching_stems;
        let (feeder, stretcher) = self.feeder_and_stretcher();
        let channels = stretcher.channels;
        read_ahead(
            feeder,
            &mut stretcher.input[..count * channels],
            stretching_stems,
            unit_step,
            block.active_loop,
            retire,
            starved,
        );
        stretcher.stretch.process(
            &stretcher.input[..count * channels],
            &mut stretcher.output[..frames * channels],
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
        let stretching_stems = self.stretching_stems;
        let (feeder, stretcher) = self.feeder_and_stretcher();
        feeder.position =
            block.playhead + lead - stretcher.priming_frames() as f64 * block.unit_step;
        if let Some(region) = block.active_loop
            && feeder.position >= region.end
        {
            feeder.position = region.fold(feeder.position);
        }
        feeder.transport.jump_to(1.0);
        read_ahead(
            feeder,
            &mut stretcher.priming,
            stretching_stems,
            block.unit_step,
            block.active_loop,
            retire,
            starved,
        );
        stretcher
            .stretch
            .seek(&stretcher.priming, playback_rate.max(0.01));
        self.owed_frames = block.advance / block.unit_step;
        self.needs_priming = false;
        self.mix.jump_to(0.0);
        self.hold_frames = self.output_latency as usize;
    }

    /// The playhead's frame `index` of the block: the stretched sound over the plain resampled
    /// one from `voice`, in the share the key lock fade has reached. Stretched stems are mixed
    /// at `stem_gains` after stretching, so a stem's level changes as fast with key lock as
    /// without it.
    #[inline]
    pub(crate) fn playhead_frame(
        &mut self,
        index: usize,
        voice: &Voice,
        step: f64,
        stem_gains: &StemGains,
        starved: &mut bool,
    ) -> (f32, f32) {
        let mix = self.mix.next_value();
        let (stretched_left, stretched_right) = match &self.stems {
            Some(stems) if self.stretching_stems => {
                let frame = &stems.output[index * PART_CHANNELS..(index + 1) * PART_CHANNELS];
                let track = (frame[PART_CHANNELS - 2], frame[PART_CHANNELS - 1]);
                mix_parts(track, frame, stem_gains)
            }
            _ => (
                self.stereo.output[index * CHANNELS],
                self.stereo.output[index * CHANNELS + 1],
            ),
        };
        let (plain_left, plain_right) = if mix < 1.0 {
            voice.read_playhead(step, stem_gains, starved)
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
/// with a crossfade where the playhead will: stereo frames of the track, or every stem side by
/// side followed by the track when `stems`.
fn read_ahead(
    feeder: &mut Voice,
    input: &mut [f32],
    stems: bool,
    unit_step: f64,
    active_loop: Option<LoopRegion>,
    retire: &mut RetireQueue,
    starved: &mut bool,
) {
    let channels = if stems { PART_CHANNELS } else { CHANNELS };
    for frame in input.chunks_exact_mut(channels) {
        if stems {
            frame.copy_from_slice(&feeder.next_part_frame(unit_step, retire, starved));
        } else {
            (frame[0], frame[1]) = feeder.next_frame(1.0, unit_step, false, retire, starved);
        }
        let previous = feeder.position;
        feeder.position += unit_step;
        if let Some(wrapped) = active_loop.and_then(|region| region.wrap(previous, feeder.position))
        {
            feeder.jump(wrapped, unit_step, retire);
        }
    }
}
