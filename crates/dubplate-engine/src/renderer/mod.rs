//! The audio-thread side of the engine: [`Renderer`] applies the control side's commands and
//! mixes the decks, sampler and preview into the master and headphone outputs.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use rtrb::Consumer;

use crate::command::Command;
use crate::curves::Ramp;
use crate::deck::{Deck, DeckId};
use crate::effects::{EffectRack, EffectTarget};
use crate::headphones::Headphones;
use crate::preview::PreviewBus;
use crate::processor::{AudioProcessor, ProcessContext, StereoBlock};
use crate::recording::RecordingSink;
use crate::retire::RetireQueue;
use crate::sampler::Sampler;
use crate::state::{GapDetector, SharedState, raise_peak};
use crate::tempo::TempoMaster;
use crate::{DECK_COUNT, INSERT_SLOTS, MAX_BLOCK_FRAMES};
use dubplate_fx::Limiter;

mod commands;
mod publish;
mod tempo;

const DECLICK_SECONDS: f64 = 0.005;
const MAX_COMMANDS_PER_CALLBACK: usize = 256;

/// The audio-thread side of the engine: it mixes the decks into the output.
///
/// An audio output calls [`Renderer::render`] from its real-time callback. Tests and offline
/// tools can call it directly to render without a sound card. Rendering never allocates,
/// locks, blocks or does I/O.
pub struct Renderer {
    decks: [Deck; DECK_COUNT],
    master_inserts: [Option<Box<dyn AudioProcessor>>; INSERT_SLOTS],
    effects: EffectRack,
    tempo_master: TempoMaster,
    master_block: StereoBlock,
    master_gain: Ramp,
    headphones: Headphones,
    sampler: Sampler,
    preview: PreviewBus,
    master_limiter: Limiter,
    recording: Option<RecordingSink>,
    headphone_limiter: Limiter,
    crossfader: f32,
    sync_half_and_double: bool,
    smart_fader: bool,
    smart_bpm: Option<f64>,
    commands: Consumer<Command>,
    retire: RetireQueue,
    shared: Arc<SharedState>,
    sample_rate: u32,
    gap_detector: GapDetector,
}

impl Renderer {
    pub(crate) fn new(
        sample_rate: u32,
        commands: Consumer<Command>,
        retire: RetireQueue,
        shared: Arc<SharedState>,
    ) -> Self {
        let declick_frames = (f64::from(sample_rate) * DECLICK_SECONDS).round().max(1.0) as u32;
        let mut renderer = Self {
            decks: std::array::from_fn(|_| {
                Deck::new(sample_rate, MAX_BLOCK_FRAMES, declick_frames)
            }),
            master_inserts: std::array::from_fn(|_| None),
            effects: EffectRack::new(sample_rate),
            tempo_master: TempoMaster::default(),
            master_block: StereoBlock::with_capacity(MAX_BLOCK_FRAMES),
            master_gain: Ramp::new(1.0, declick_frames),
            headphones: Headphones::new(MAX_BLOCK_FRAMES, declick_frames),
            sampler: Sampler::new(sample_rate, MAX_BLOCK_FRAMES, declick_frames),
            preview: PreviewBus::new(sample_rate, MAX_BLOCK_FRAMES, declick_frames),
            master_limiter: Limiter::new(sample_rate as f32),
            recording: None,
            headphone_limiter: Limiter::new(sample_rate as f32),
            crossfader: 0.5,
            sync_half_and_double: true,
            smart_fader: false,
            smart_bpm: None,
            commands,
            retire,
            shared,
            sample_rate,
            gap_detector: GapDetector::default(),
        };
        for deck in DeckId::ALL {
            renderer.update_channel_gain(deck);
        }
        for deck in &mut renderer.decks {
            deck.settle_gains();
        }
        let mixer_latency =
            renderer.decks[0].mixer_latency_frames() + renderer.master_limiter.latency_frames();
        renderer.shared.stats.mixer_latency_frames.store(
            u32::try_from(mixer_latency).unwrap_or(u32::MAX),
            Ordering::Relaxed,
        );
        renderer.publish_state();
        renderer
    }

    /// Output sample rate in Hz.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Fills `output`, interleaved with `channel_count` channels, with the next frames of the
    /// mix. The master goes to the first two channels and, with four or more channels, the
    /// headphones to the third and fourth; any further channels are silent and a mono output
    /// gets the sum of both sides at half level.
    pub fn render(&mut self, output: &mut [f32], channel_count: usize) {
        let callback = self.begin_callback();
        let starved = self.render_frames(output, channel_count);
        self.end_callback(callback, output.len() / channel_count.max(1), starved);
    }

    /// Starts serving a callback: applies the commands that arrived since the previous one.
    pub(crate) fn begin_callback(&mut self) -> Instant {
        let started = Instant::now();
        self.apply_commands();
        started
    }

    /// Renders into `output` and reports whether a playing deck ran out of decoded audio.
    pub(crate) fn render_frames(&mut self, output: &mut [f32], channel_count: usize) -> bool {
        let channel_count = channel_count.max(1);
        let frame_count = output.len() / channel_count;
        let mut starved = false;
        for chunk in output.chunks_mut(MAX_BLOCK_FRAMES * channel_count) {
            let block_frames = chunk.len() / channel_count;
            starved |= self.render_block(block_frames);
            write_interleaved(
                &self.master_block,
                &self.headphones.block,
                chunk,
                channel_count,
            );
        }
        output[frame_count * channel_count..].fill(0.0);
        starved
    }

    /// Finishes serving a callback: publishes deck state and records how long it took.
    pub(crate) fn end_callback(&mut self, started: Instant, frame_count: usize, starved: bool) {
        self.publish_state();
        self.shared
            .stats
            .record_callback(frame_count, started.elapsed(), starved);
    }

    /// Records the device's timing for the callback being served: when it was called and when
    /// its first sample will be heard, both on the device's clock.
    pub(crate) fn observe_device_timing(
        &mut self,
        callback_time: Duration,
        playback_time: Duration,
        frame_count: usize,
    ) {
        let stats = &self.shared.stats;
        stats.record_output_latency(playback_time.saturating_sub(callback_time));
        if self
            .gap_detector
            .observe(callback_time, frame_count, self.sample_rate)
        {
            stats.device_gaps.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(super) fn render_block(&mut self, frame_count: usize) -> bool {
        self.master_block.set_frame_count(frame_count);
        self.master_block.silence();
        self.headphones.begin_block(frame_count);
        let tempo_master = self.choose_tempo_master();
        self.drive_smart_fader(tempo_master, frame_count);
        self.follow_tempo_master(tempo_master);
        let deck_contexts = self.decks.each_ref().map(|deck| ProcessContext {
            sample_rate: self.sample_rate,
            tempo: deck.tempo(),
            beat_clock: deck.beat_clock(),
        });
        let mut starved = false;
        for (deck_id, (deck, context)) in DeckId::ALL
            .into_iter()
            .zip(self.decks.iter_mut().zip(&deck_contexts))
        {
            starved |= deck.render(frame_count, context, &mut self.retire).starved;
            let shared = &self.shared.decks[deck_id.index()];
            raise_peak(&shared.pre_fader_peak, deck.block.peak());
            self.headphones.listen(deck_id, &deck.block);
            deck.apply_channel_gain();
            self.effects
                .process(EffectTarget::Deck(deck_id), &mut deck.block, context);
            let peak = deck.mix_into(&mut self.master_block);
            raise_peak(&self.shared.decks[deck_id.index()].peak, peak);
        }
        let sampler_bpm = tempo_master.and_then(|deck| self.decks[deck.index()].bpm());
        self.sampler
            .render(frame_count, sampler_bpm, &mut self.retire, &mut starved);
        if self.sampler.is_cued() {
            self.headphones.listen_to(&self.sampler.block);
        }
        self.master_block.add(&self.sampler.block);
        raise_peak(&self.shared.sampler_peak, self.sampler.block.peak());
        let master_context = ProcessContext {
            sample_rate: self.sample_rate,
            tempo: 1.0,
            beat_clock: tempo_master.and_then(|deck| deck_contexts[deck.index()].beat_clock),
        };
        for processor in self.master_inserts.iter_mut().flatten() {
            processor.process(&mut self.master_block, &master_context);
        }
        self.effects.process(
            EffectTarget::Master,
            &mut self.master_block,
            &master_context,
        );
        starved |= self.render_preview(frame_count);
        self.headphones
            .finish(&self.master_block, &self.preview.block);
        let (headphones_left, headphones_right) = self.headphones.block.channels_mut();
        self.headphone_limiter
            .process(headphones_left, headphones_right);
        raise_peak(&self.shared.headphone_peak, self.headphones.block.peak());
        let (left_channel, right_channel) = self.master_block.channels_mut();
        for (left, right) in left_channel.iter_mut().zip(right_channel.iter_mut()) {
            let gain = self.master_gain.next_value();
            *left *= gain;
            *right *= gain;
        }
        self.master_limiter.process(left_channel, right_channel);
        if let Some(recording) = &mut self.recording {
            recording.write(&self.master_block);
        }
        raise_peak(&self.shared.master_peak, self.master_block.peak());
        raise_peak(
            &self.shared.master_limiting,
            1.0 - self.master_limiter.take_gain_reduction(),
        );
        starved
    }

    pub(super) fn render_preview(&mut self, frame_count: usize) -> bool {
        let mut starved = false;
        self.preview
            .render(frame_count, &mut self.retire, &mut starved);
        starved
    }
}

/// Writes the master to the first two channels and, on a device with four or more, the
/// headphones to the third and fourth, as a DJ controller's sound card wires them. A mono device
/// gets the master summed at half level; any further channels are silent.
fn write_interleaved(
    master: &StereoBlock,
    headphones: &StereoBlock,
    output: &mut [f32],
    channel_count: usize,
) {
    let frames = output.chunks_exact_mut(channel_count);
    let master_frames = master.left().iter().zip(master.right());
    let headphone_frames = headphones.left().iter().zip(headphones.right());
    for (frame, ((left, right), (phones_left, phones_right))) in
        frames.zip(master_frames.zip(headphone_frames))
    {
        match frame {
            [mono] => *mono = (left + right) * 0.5,
            [first, second] => {
                *first = *left;
                *second = *right;
            }
            [first, second, third] => {
                *first = *left;
                *second = *right;
                *third = 0.0;
            }
            [first, second, third, fourth, rest @ ..] => {
                *first = *left;
                *second = *right;
                *third = *phones_left;
                *fourth = *phones_right;
                rest.fill(0.0);
            }
            [] => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::deck::DeckId;
    use crate::offline_engine;
    use crate::track::TrackWriter;

    #[test]
    fn a_deck_that_reaches_undecoded_audio_counts_as_starved() {
        let (mut controller, mut renderer) = offline_engine(48_000);
        let mut writer = TrackWriter::new(48_000, 2);
        for _ in 0..600 {
            writer.push_frame(0.5, 0.5);
        }
        writer.publish();
        controller.load_track(DeckId::A, writer.track()).unwrap();
        controller.play(DeckId::A).unwrap();
        let mut output = vec![0.0; 960];
        renderer.render(&mut output, 2);
        assert_eq!(controller.stats().starved_callbacks, 0);
        renderer.render(&mut output, 2);
        renderer.render(&mut output, 2);
        assert_eq!(controller.stats().starved_callbacks, 2);
        assert_eq!(output[output.len() - 1], 0.0);

        for _ in 0..48_000 {
            writer.push_frame(0.5, 0.5);
        }
        writer.finish();
        renderer.render(&mut output, 2);
        let stats = controller.stats();
        assert_eq!(stats.starved_callbacks, 2);
        assert_eq!(stats.callbacks, 4);
        assert_eq!(stats.buffer_frames, 480);
        assert!((stats.buffer_latency_ms - 10.0).abs() < 1e-9);
    }
}
