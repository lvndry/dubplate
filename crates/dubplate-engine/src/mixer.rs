use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use rtrb::{Consumer, Producer};

use crate::curves::{Ramp, channel_fader_gain, crossfader_gains};
use crate::deck::{Deck, DeckId};
use crate::effects::{EffectChange, EffectRack, EffectTarget, EffectUnitId};
use crate::grid::BeatGrid;
use crate::headphones::Headphones;
use crate::jog::JogInput;
use crate::processor::{AudioProcessor, ProcessContext, StereoBlock};
use crate::state::{GapDetector, SharedState, encode_loop_length, raise_peak};
use crate::sync::SyncMode;
use crate::tempo::{DeckLevel, MasterTempo, TempoMaster, TempoRange};
use crate::track::TrackBuffer;
use crate::{DECK_COUNT, INSERT_SLOTS, MAX_BLOCK_FRAMES};
use dubplate_fx::{BeatLength, EqBand, Limiter};

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
    SetBeatGrid {
        deck: DeckId,
        beat_grid: Option<BeatGrid>,
    },
    SetMasterTempo(MasterTempo),
    ChangeEffect {
        unit: EffectUnitId,
        change: EffectChange,
    },
    SetEq {
        deck: DeckId,
        band: EqBand,
        position: f32,
    },
    SetFilter {
        deck: DeckId,
        position: f32,
    },
    SetFilterResonance {
        deck: DeckId,
        resonance: f32,
    },
    Jog {
        deck: DeckId,
        input: JogInput,
    },
    SetVinylMode {
        deck: DeckId,
        enabled: bool,
    },
    SetJogTicksPerRevolution(u32),
    SetTempoFader {
        deck: DeckId,
        position: f64,
    },
    SetTempoRange {
        deck: DeckId,
        range: TempoRange,
    },
    SetPitchBend {
        deck: DeckId,
        bend: f64,
    },
    SetSync {
        deck: DeckId,
        mode: SyncMode,
    },
    SetSyncHalfAndDouble(bool),
    SetQuantize(bool),
    HotCue {
        deck: DeckId,
        seconds: f64,
    },
    BeatJump {
        deck: DeckId,
        beats: f64,
    },
    LoopIn(DeckId),
    LoopOut(DeckId),
    BeatLoop {
        deck: DeckId,
        length: BeatLength,
    },
    HalveLoop(DeckId),
    DoubleLoop(DeckId),
    ReloopOrExit(DeckId),
    SetHeadphoneCue {
        deck: DeckId,
        enabled: bool,
    },
    SetMasterCue(bool),
    SetHeadphoneMix(f32),
    SetHeadphoneLevel(f32),
    SetKeyLock {
        deck: DeckId,
        enabled: bool,
    },
}

/// Something the audio thread no longer needs. It goes back to the control side to be dropped,
/// because freeing memory on the audio thread can block.
pub(crate) enum Retired {
    Track(#[allow(dead_code)] Arc<TrackBuffer>),
    Processor(#[allow(dead_code)] Box<dyn AudioProcessor>),
    BeatGrid(#[allow(dead_code)] BeatGrid),
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

    pub(crate) fn beat_grid(&mut self, beat_grid: BeatGrid) {
        self.retire(Retired::BeatGrid(beat_grid));
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
    effects: EffectRack,
    tempo_master: TempoMaster,
    master_block: StereoBlock,
    master_gain: Ramp,
    headphones: Headphones,
    master_limiter: Limiter,
    headphone_limiter: Limiter,
    crossfader: f32,
    sync_half_and_double: bool,
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
            master_limiter: Limiter::new(sample_rate as f32),
            headphone_limiter: Limiter::new(sample_rate as f32),
            crossfader: 0.5,
            sync_half_and_double: true,
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

    fn render_block(&mut self, frame_count: usize) -> bool {
        self.master_block.set_frame_count(frame_count);
        self.master_block.silence();
        self.headphones.begin_block(frame_count);
        let tempo_master = self.choose_tempo_master();
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
        self.headphones.finish(&self.master_block);
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
        raise_peak(&self.shared.master_peak, self.master_block.peak());
        raise_peak(
            &self.shared.master_limiting,
            1.0 - self.master_limiter.take_gain_reduction(),
        );
        starved
    }

    fn choose_tempo_master(&mut self) -> Option<DeckId> {
        let (a_gain, b_gain) = crossfader_gains(self.crossfader);
        let levels = DeckId::ALL.map(|deck_id| {
            let deck = &self.decks[deck_id.index()];
            let crossfader_gain = if deck_id == DeckId::A { a_gain } else { b_gain };
            DeckLevel {
                deck: deck_id,
                has_beat_grid: deck.has_beat_grid(),
                level: deck.level(crossfader_gain),
                playing: deck.is_playing(),
            }
        });
        self.tempo_master.update(&levels)
    }

    /// Brings every synced deck other than the tempo master to the master's tempo, and with
    /// beat sync to its beats, for the next block.
    fn follow_tempo_master(&mut self, master: Option<DeckId>) {
        let master_beat = master.and_then(|deck| self.decks[deck.index()].master_beat(deck));
        for deck_id in DeckId::ALL {
            let deck = &mut self.decks[deck_id.index()];
            match master_beat {
                Some(beat) if beat.deck != deck_id && deck.sync_mode() != SyncMode::Off => {
                    deck.follow(beat, self.sync_half_and_double, &mut self.retire);
                }
                _ => deck.stop_following(),
            }
        }
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
            Command::SetBeatGrid { deck, beat_grid } => {
                self.decks[deck.index()].set_beat_grid(beat_grid, &mut self.retire);
            }
            Command::SetTempoFader { deck, position } => {
                self.decks[deck.index()].set_tempo_fader(position);
            }
            Command::SetTempoRange { deck, range } => {
                self.decks[deck.index()].set_tempo_range(range);
            }
            Command::SetPitchBend { deck, bend } => {
                self.decks[deck.index()].set_pitch_bend(bend);
            }
            Command::SetSync { deck, mode } => self.decks[deck.index()].set_sync(mode),
            Command::SetSyncHalfAndDouble(allowed) => self.sync_half_and_double = allowed,
            Command::SetQuantize(enabled) => {
                for deck in &mut self.decks {
                    deck.set_quantize(enabled);
                }
            }
            Command::HotCue { deck, seconds } => {
                self.decks[deck.index()].hot_cue(seconds, &mut self.retire);
            }
            Command::BeatJump { deck, beats } => {
                self.decks[deck.index()].beat_jump(beats, &mut self.retire);
            }
            Command::LoopIn(deck) => self.decks[deck.index()].loop_in(),
            Command::LoopOut(deck) => self.decks[deck.index()].loop_out(&mut self.retire),
            Command::BeatLoop { deck, length } => {
                self.decks[deck.index()].beat_loop(length, &mut self.retire);
            }
            Command::HalveLoop(deck) => self.decks[deck.index()].halve_loop(&mut self.retire),
            Command::DoubleLoop(deck) => self.decks[deck.index()].double_loop(&mut self.retire),
            Command::ReloopOrExit(deck) => {
                self.decks[deck.index()].reloop_or_exit(&mut self.retire);
            }
            Command::SetHeadphoneCue { deck, enabled } => self.headphones.set_cue(deck, enabled),
            Command::SetMasterCue(enabled) => self.headphones.set_master_cue(enabled),
            Command::SetHeadphoneMix(mix) => self.headphones.set_mix(mix),
            Command::SetHeadphoneLevel(level) => self.headphones.set_level(level),
            Command::SetKeyLock { deck, enabled } => {
                self.decks[deck.index()].set_key_lock(enabled);
            }
            Command::SetMasterTempo(source) => self.tempo_master.set_source(source),
            Command::ChangeEffect { unit, change } => self.effects.change(unit, change),
            Command::SetEq {
                deck,
                band,
                position,
            } => self.decks[deck.index()].set_eq(band, position),
            Command::SetFilter { deck, position } => {
                self.decks[deck.index()].set_filter(position);
            }
            Command::SetFilterResonance { deck, resonance } => {
                self.decks[deck.index()].set_filter_resonance(resonance);
            }
            Command::Jog { deck, input } => self.decks[deck.index()].jog(input),
            Command::SetVinylMode { deck, enabled } => {
                self.decks[deck.index()].set_vinyl_mode(enabled);
            }
            Command::SetJogTicksPerRevolution(ticks) => {
                for deck in &mut self.decks {
                    deck.set_jog_ticks_per_revolution(ticks);
                }
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
            shared.speed.store(deck.speed());
            shared.bpm.store(deck.bpm().unwrap_or(f64::NAN));
            shared
                .tempo_range
                .store(deck.tempo_range().index(), Ordering::Relaxed);
            shared
                .sync_mode
                .store(deck.sync_mode().index(), Ordering::Relaxed);
            shared
                .following
                .store(deck.is_following(), Ordering::Relaxed);
            shared
                .fader_waits_for_pickup
                .store(deck.fader_waits_for_pickup(), Ordering::Relaxed);
            let region = deck.loop_region();
            let seconds_or_none = |frames: Option<f64>| {
                frames.map_or(f64::NAN, |frames| deck.frames_to_seconds(frames))
            };
            shared
                .loop_start_seconds
                .store(seconds_or_none(region.map(|region| region.start)));
            shared
                .loop_end_seconds
                .store(seconds_or_none(region.map(|region| region.end)));
            shared.loop_active.store(
                region.is_some_and(|region| region.active),
                Ordering::Relaxed,
            );
            shared.loop_length.store(
                encode_loop_length(region.and_then(|region| region.length)),
                Ordering::Relaxed,
            );
            shared
                .pending_loop_start_seconds
                .store(seconds_or_none(deck.pending_loop_start()));
            shared.touched.store(deck.is_touched(), Ordering::Relaxed);
            shared
                .vinyl_mode
                .store(deck.vinyl_mode(), Ordering::Relaxed);
        }
        for (sounding, shared) in self
            .effects
            .sounding()
            .into_iter()
            .zip(&self.shared.effect_sounding)
        {
            shared.store(sounding, Ordering::Relaxed);
        }
        self.shared.tempo_master.store(self.tempo_master.current());
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
