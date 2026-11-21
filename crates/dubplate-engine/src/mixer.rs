use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use rtrb::{Consumer, Producer};

use crate::curves::{Ramp, channel_fader_gain, crossfader_gains};
use crate::deck::{Deck, DeckId};
use crate::processor::{AudioProcessor, ProcessContext, StereoBlock};
use crate::state::{GapDetector, SharedState, raise_peak};
use crate::track::TrackBuffer;
use crate::{DECK_COUNT, INSERT_SLOTS, MAX_BLOCK_FRAMES};

const DECLICK_SECONDS: f64 = 0.005;
const MAX_COMMANDS_PER_CALLBACK: usize = 256;

/// A change the control side asks the audio thread to make.
pub(crate) enum Command {
    Load {
        deck: DeckId,
        track: Option<Arc<TrackBuffer>>,
    },
    Play(DeckId),
    Pause(DeckId),
    Cue(DeckId),
    SetCuePoint {
        deck: DeckId,
        seconds: f64,
    },
    Seek {
        deck: DeckId,
        seconds: f64,
    },
    SetTempo {
        deck: DeckId,
        tempo: f64,
    },
    SetTrim {
        deck: DeckId,
        gain: f32,
    },
    SetChannelFader {
        deck: DeckId,
        position: f32,
    },
    SetCrossfader(f32),
    SetMasterGain(f32),
    SetDeckInsert {
        deck: DeckId,
        slot: usize,
        processor: Option<Box<dyn AudioProcessor>>,
    },
    SetMasterInsert {
        slot: usize,
        processor: Option<Box<dyn AudioProcessor>>,
    },
}

/// Something the audio thread no longer needs. It goes back to the control side to be dropped,
/// because freeing memory on the audio thread can block.
pub(crate) enum Retired {
    Track(#[allow(dead_code)] Arc<TrackBuffer>),
    Processor(#[allow(dead_code)] Box<dyn AudioProcessor>),
}

/// The audio thread's end of the queue of retired objects.
pub(crate) struct RetireQueue {
    producer: Producer<Retired>,
}

impl RetireQueue {
    pub(crate) fn new(producer: Producer<Retired>) -> Self {
        Self { producer }
    }

    pub(crate) fn track(&mut self, track: Arc<TrackBuffer>) {
        self.retire(Retired::Track(track));
    }

    fn processor(&mut self, processor: Box<dyn AudioProcessor>) {
        self.retire(Retired::Processor(processor));
    }

    fn retire(&mut self, item: Retired) {
        if let Err(rtrb::PushError::Full(item)) = self.producer.push(item) {
            // Leaking is the lesser harm: freeing here could block the audio thread.
            std::mem::forget(item);
        }
    }
}

/// The audio-thread side of the engine: it mixes the decks into the output.
///
/// An audio output calls [`Renderer::render`] from its real-time callback. Tests and offline
/// tools can call it directly to render without a sound card. Rendering never allocates,
/// locks, blocks or does I/O.
pub struct Renderer {
    decks: [Deck; DECK_COUNT],
    master_inserts: [Option<Box<dyn AudioProcessor>>; INSERT_SLOTS],
    master_block: StereoBlock,
    master_gain: Ramp,
    crossfader: f32,
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
            master_block: StereoBlock::with_capacity(MAX_BLOCK_FRAMES),
            master_gain: Ramp::new(1.0, declick_frames),
            crossfader: 0.5,
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
        renderer.publish_state();
        renderer
    }

    /// Output sample rate in Hz.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Fills `output`, interleaved with `channel_count` channels, with the next frames of the
    /// mix. Deck A and B's mix goes to the first two channels; any further channels are silent
    /// and a mono output gets the sum of both sides at half level.
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
            write_interleaved(&self.master_block, chunk, channel_count);
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

    fn render_block(&mut self, frame_count: usize) -> bool {
        self.master_block.set_frame_count(frame_count);
        self.master_block.silence();
        let mut starved = false;
        for deck in &mut self.decks {
            let context = ProcessContext {
                sample_rate: self.sample_rate,
                tempo: deck.tempo(),
            };
            starved |= deck.render(frame_count, &context, &mut self.retire).starved;
        }
        for (index, deck) in self.decks.iter_mut().enumerate() {
            let peak = deck.mix_into(&mut self.master_block);
            raise_peak(&self.shared.decks[index].peak, peak);
        }
        let context = ProcessContext {
            sample_rate: self.sample_rate,
            tempo: 1.0,
        };
        for processor in self.master_inserts.iter_mut().flatten() {
            processor.process(&mut self.master_block, &context);
        }
        let (left_channel, right_channel) = self.master_block.channels_mut();
        let mut peak = 0.0_f32;
        for (left, right) in left_channel.iter_mut().zip(right_channel.iter_mut()) {
            let gain = self.master_gain.next_value();
            *left *= gain;
            *right *= gain;
            peak = peak.max(left.abs()).max(right.abs());
        }
        raise_peak(&self.shared.master_peak, peak);
        starved
    }

    fn apply_commands(&mut self) {
        for _ in 0..MAX_COMMANDS_PER_CALLBACK {
            let Ok(command) = self.commands.pop() else {
                return;
            };
            self.apply(command);
        }
    }

    fn apply(&mut self, command: Command) {
        match command {
            Command::Load { deck, track } => {
                self.decks[deck.index()].load(track, &mut self.retire);
            }
            Command::Play(deck) => self.decks[deck.index()].play(),
            Command::Pause(deck) => self.decks[deck.index()].pause(&mut self.retire),
            Command::Cue(deck) => self.decks[deck.index()].cue(&mut self.retire),
            Command::SetCuePoint { deck, seconds } => {
                self.decks[deck.index()].set_cue_point(seconds);
            }
            Command::Seek { deck, seconds } => {
                self.decks[deck.index()].seek(seconds, &mut self.retire);
            }
            Command::SetTempo { deck, tempo } => self.decks[deck.index()].set_tempo(tempo),
            Command::SetTrim { deck, gain } => self.decks[deck.index()].set_trim(gain),
            Command::SetChannelFader { deck, position } => {
                self.decks[deck.index()].set_fader_position(position);
                self.update_channel_gain(deck);
            }
            Command::SetCrossfader(position) => {
                self.crossfader = position;
                for deck in DeckId::ALL {
                    self.update_channel_gain(deck);
                }
            }
            Command::SetMasterGain(gain) => self.master_gain.set_target(gain),
            Command::SetDeckInsert {
                deck,
                slot,
                processor,
            } => {
                let slot = self.decks[deck.index()].inserts.get_mut(slot);
                replace_insert(slot, processor, &mut self.retire);
            }
            Command::SetMasterInsert { slot, processor } => {
                replace_insert(
                    self.master_inserts.get_mut(slot),
                    processor,
                    &mut self.retire,
                );
            }
        }
    }

    fn update_channel_gain(&mut self, deck: DeckId) {
        let (left_gain, right_gain) = crossfader_gains(self.crossfader);
        let side_gain = match deck {
            DeckId::A => left_gain,
            DeckId::B => right_gain,
        };
        let deck = &mut self.decks[deck.index()];
        deck.set_channel_gain(channel_fader_gain(deck.fader_position()) * side_gain);
    }

    fn publish_state(&self) {
        for (deck, shared) in self.decks.iter().zip(&self.shared.decks) {
            shared.loaded.store(deck.is_loaded(), Ordering::Relaxed);
            shared.playing.store(deck.is_playing(), Ordering::Relaxed);
            shared.position_seconds.store(deck.position_seconds());
            shared.cue_point_seconds.store(deck.cue_point_seconds());
            shared.tempo.store(deck.tempo());
        }
    }
}

/// Puts a processor in a slot and sends the one it replaces off the audio thread. A slot that
/// does not exist sends the new processor back too.
fn replace_insert(
    slot: Option<&mut Option<Box<dyn AudioProcessor>>>,
    processor: Option<Box<dyn AudioProcessor>>,
    retire: &mut RetireQueue,
) {
    let unused = match slot {
        Some(slot) => std::mem::replace(slot, processor),
        None => processor,
    };
    if let Some(unused) = unused {
        retire.processor(unused);
    }
}

fn write_interleaved(block: &StereoBlock, output: &mut [f32], channel_count: usize) {
    let frames = output.chunks_exact_mut(channel_count);
    for ((frame, left), right) in frames.zip(block.left()).zip(block.right()) {
        match frame {
            [mono] => *mono = (left + right) * 0.5,
            [first, second, rest @ ..] => {
                *first = *left;
                *second = *right;
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
