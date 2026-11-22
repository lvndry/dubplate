use std::sync::Arc;

use crate::keylock::{KeyLock, KeyLockBlock};
use crate::mixer::RetireQueue;
use crate::processor::StereoBlock;
use crate::track::{Track, TrackBuffer};
use crate::voice::{UNITY_STEM_GAINS, Voice};

/// The longest a preview plays before it stops itself, in seconds.
pub const MAX_PREVIEW_SECONDS: f64 = 30.0;

/// One of the two tracks of a [`Preview`].
#[derive(Clone)]
pub struct PreviewTrack {
    /// The track.
    pub track: Track,
    /// Where it starts playing, in seconds from the start of the track.
    pub start_seconds: f64,
    /// Its tempo, where 1.0 is the track's own: the tempo it will play at in the mix.
    pub tempo: f64,
}

impl std::fmt::Debug for PreviewTrack {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreviewTrack")
            .field("start_seconds", &self.start_seconds)
            .field("tempo", &self.tempo)
            .finish_non_exhaustive()
    }
}

/// A track or a seam to hear in the headphones before playing it: one track, or two
/// overlapping as they will in the mix.
#[derive(Debug, Clone)]
pub struct Preview {
    /// The track, or the outgoing track of a seam, from its own start position.
    pub first: PreviewTrack,
    /// The incoming track of a seam, which comes in `second_offset_seconds` after the preview
    /// starts; `None` to hear the first track alone.
    pub second: Option<PreviewTrack>,
    /// When the second track comes in, in seconds after the preview starts.
    pub second_offset_seconds: f64,
    /// Plays both tracks at their tempos without changing their pitch, as key lock does.
    pub key_lock: bool,
}

/// Where a preview is, as the audio thread last published it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreviewState {
    /// A preview plays.
    pub playing: bool,
    /// Seconds since the preview started.
    pub position_seconds: f64,
}

/// A preview as the audio thread receives it: tracks and positions in frames.
pub(crate) struct PreviewPlan {
    pub(crate) tracks: [Option<PlannedTrack>; 2],
    pub(crate) second_offset_seconds: f64,
    pub(crate) key_lock: bool,
}

pub(crate) struct PlannedTrack {
    pub(crate) track: Arc<TrackBuffer>,
    pub(crate) start_seconds: f64,
    pub(crate) tempo: f64,
}

impl PreviewPlan {
    pub(crate) fn new(preview: &Preview) -> Self {
        let planned = |preview_track: &PreviewTrack| PlannedTrack {
            track: Arc::clone(preview_track.track.buffer()),
            start_seconds: preview_track.start_seconds,
            tempo: preview_track.tempo,
        };
        Self {
            tracks: [
                Some(planned(&preview.first)),
                preview.second.as_ref().map(planned),
            ],
            second_offset_seconds: preview.second_offset_seconds,
            key_lock: preview.key_lock,
        }
    }
}

/// One track of the preview on the audio thread: the deck's own reader and key lock.
struct PreviewVoice {
    voice: Voice,
    key_lock: KeyLock,
    tempo: f64,
    starts_at_frame: u64,
    finished: bool,
}

/// The audio-thread side of the preview: it plays into a block the headphones mix in, after
/// the cue mix and before the headphone level, and never reaches the decks or the master.
pub(crate) struct PreviewBus {
    voices: [PreviewVoice; 2],
    key_lock: bool,
    playing: bool,
    elapsed_frames: u64,
    output_rate: f64,
    max_frames: u64,
    pub(crate) block: StereoBlock,
}

impl PreviewBus {
    /// Builds the bus for `output_rate`. This allocates its time-stretchers, so it runs before
    /// the renderer reaches the audio thread.
    pub(crate) fn new(output_rate: u32, max_block_frames: usize, declick_frames: u32) -> Self {
        let voice = || PreviewVoice {
            voice: Voice::new(declick_frames),
            key_lock: KeyLock::new(output_rate, declick_frames),
            tempo: 1.0,
            starts_at_frame: 0,
            finished: true,
        };
        let output_rate = f64::from(output_rate.max(1));
        Self {
            voices: [voice(), voice()],
            key_lock: false,
            playing: false,
            elapsed_frames: 0,
            output_rate,
            max_frames: (MAX_PREVIEW_SECONDS * output_rate) as u64,
            block: StereoBlock::with_capacity(max_block_frames),
        }
    }

    pub(crate) fn is_playing(&self) -> bool {
        self.playing
    }

    pub(crate) fn position_seconds(&self) -> f64 {
        self.elapsed_frames as f64 / self.output_rate
    }

    /// Starts a preview, or stops the one playing with `None`.
    pub(crate) fn set(&mut self, plan: Option<PreviewPlan>, retire: &mut RetireQueue) {
        for preview_voice in &mut self.voices {
            preview_voice.voice.start_tail(0.0, retire);
            preview_voice.key_lock.stop(retire);
        }
        let Some(plan) = plan else {
            self.playing = false;
            return;
        };
        let output_rate = self.output_rate;
        let [first, second] = plan.tracks;
        let starts = [
            0,
            (plan.second_offset_seconds.max(0.0) * output_rate) as u64,
        ];
        for ((preview_voice, planned), starts_at_frame) in
            self.voices.iter_mut().zip([first, second]).zip(starts)
        {
            let Some(planned) = planned else {
                if let Some(previous) = preview_voice.voice.track.take() {
                    retire.track(previous);
                }
                preview_voice.finished = true;
                continue;
            };
            let source_rate = f64::from(planned.track.sample_rate());
            let previous = preview_voice.voice.track.replace(planned.track);
            if let Some(previous) = previous {
                retire.track(previous);
            }
            preview_voice.voice.position = planned.start_seconds.max(0.0) * source_rate;
            preview_voice.voice.transport.jump_to(0.0);
            preview_voice.voice.transport.set_target(1.0);
            preview_voice.tempo = planned.tempo;
            preview_voice.starts_at_frame = starts_at_frame;
            preview_voice.finished = false;
        }
        self.key_lock = plan.key_lock;
        self.elapsed_frames = 0;
        self.playing = true;
    }

    /// Renders the next frames of the preview into its block.
    pub(crate) fn render(
        &mut self,
        frame_count: usize,
        retire: &mut RetireQueue,
        starved: &mut bool,
    ) {
        self.block.set_frame_count(frame_count);
        self.block.silence();
        let output_rate = self.output_rate;
        let block_start = self.elapsed_frames;
        let key_lock = self.key_lock && self.playing;
        let (left_out, right_out) = self.block.channels_mut();
        for preview_voice in &mut self.voices {
            if !self.playing && !preview_voice.voice.has_tails() {
                continue;
            }
            let Some(track) = preview_voice.voice.track.clone() else {
                continue;
            };
            let unit_step = f64::from(track.sample_rate()) / output_rate;
            let step = preview_voice.tempo * unit_step;
            let skip = preview_voice
                .starts_at_frame
                .saturating_sub(block_start)
                .min(frame_count as u64) as usize;
            let active = frame_count - skip;
            let running = !preview_voice.finished && self.playing;
            let stretched = preview_voice.key_lock.prepare(
                KeyLockBlock {
                    wanted: key_lock && running && (preview_voice.tempo - 1.0).abs() > 1e-6,
                    track: Some(&track),
                    playhead: preview_voice.voice.position,
                    advance: if running { step * active as f64 } else { 0.0 },
                    frames: active,
                    unit_step,
                    active_loop: None,
                    stems: false,
                },
                retire,
                starved,
            );
            let end = track.total_frames().map(|frames| frames as f64);
            for (index, (left, right)) in left_out[skip..]
                .iter_mut()
                .zip(right_out[skip..].iter_mut())
                .enumerate()
            {
                let gain = preview_voice.voice.transport.next_value();
                let (playhead_left, playhead_right) = if stretched {
                    preview_voice.key_lock.playhead_frame(
                        index,
                        &preview_voice.voice,
                        step,
                        &UNITY_STEM_GAINS,
                        starved,
                    )
                } else if gain > 0.0 {
                    preview_voice
                        .voice
                        .read_playhead(step, &UNITY_STEM_GAINS, starved)
                } else {
                    (0.0, 0.0)
                };
                let (tail_left, tail_right) =
                    preview_voice
                        .voice
                        .next_tails(&UNITY_STEM_GAINS, retire, starved);
                *left += playhead_left * gain + tail_left;
                *right += playhead_right * gain + tail_right;
                if running && !preview_voice.finished {
                    preview_voice.voice.position += step;
                    if end.is_some_and(|end| preview_voice.voice.position >= end) {
                        preview_voice.finished = true;
                        preview_voice.voice.transport.jump_to(0.0);
                    }
                }
            }
        }
        if self.playing {
            self.elapsed_frames += frame_count as u64;
            let all_finished = self
                .voices
                .iter()
                .all(|preview_voice| preview_voice.finished);
            if all_finished || self.elapsed_frames >= self.max_frames {
                self.playing = false;
                for preview_voice in &mut self.voices {
                    preview_voice.voice.start_tail(0.0, retire);
                    preview_voice.finished = true;
                }
            }
        }
    }
}
