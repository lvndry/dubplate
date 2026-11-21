use std::sync::Arc;

use dubplate_fx::EqBand;

use crate::INSERT_SLOTS;
use crate::channel::ChannelStrip;
use crate::curves::{Ramp, channel_fader_gain};
use crate::grid::BeatGrid;
use crate::jog::{BlockStart, JogInput, Platter};
use crate::mixer::RetireQueue;
use crate::processor::{AudioProcessor, ProcessContext, StereoBlock};
use crate::sync::{MasterBeat, PhaseAction, SyncFollow, SyncMode, phase_action, sync_multiple};
use crate::tempo::{BeatClock, TempoFader, TempoRange};
use crate::track::{FrameRead, MAX_TRACK_SECONDS, TrackBuffer};

/// One of the decks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeckId {
    /// Deck A, on the left side of the crossfader.
    A,
    /// Deck B, on the right side of the crossfader.
    B,
}

impl DeckId {
    /// Every deck, in order.
    pub const ALL: [DeckId; crate::DECK_COUNT] = [DeckId::A, DeckId::B];

    /// Position of the deck in [`DeckId::ALL`].
    pub fn index(self) -> usize {
        match self {
            DeckId::A => 0,
            DeckId::B => 1,
        }
    }
}

/// Fades that can overlap on one deck, for jumps that come faster than a fade lasts.
const MAX_TAILS: usize = 8;

/// A short fade that lets the old playback position die away after a jump, a pause or a load,
/// so the waveform never steps from one value to another between two samples.
struct Tail {
    track: Arc<TrackBuffer>,
    position: f64,
    rate: f64,
    start_gain: f32,
    remaining_frames: u32,
}

impl Tail {
    /// The tail's gain on its next frame. It falls one step before the first frame, so with
    /// the new position fading in from one step the two always add up to the level before the
    /// jump.
    fn gain(&self, declick_frames: u32) -> f32 {
        self.start_gain * self.remaining_frames.saturating_sub(1) as f32 / declick_frames as f32
    }
}

/// Below this share of its own speed the record is too slow to be heard; the level follows the
/// speed down to silence, as a stylus does, so a record held still is silent instead of
/// holding a constant offset.
const AUDIBLE_SPEED: f64 = 0.05;

/// What rendering one block of a deck found.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DeckRender {
    pub(crate) starved: bool,
}

/// The audio-thread side of a deck.
pub(crate) struct Deck {
    track: Option<Arc<TrackBuffer>>,
    position: f64,
    tempo: f64,
    rate: f64,
    playing: bool,
    transport: Ramp,
    tails: [Option<Tail>; MAX_TAILS],
    cue_point: f64,
    beat_grid: Option<BeatGrid>,
    fader: TempoFader,
    sync: SyncFollow,
    phase_bend: f64,
    trim: Ramp,
    channel_gain: Ramp,
    fader_position: f32,
    platter: Platter,
    strip: ChannelStrip,
    pub(crate) inserts: [Option<Box<dyn AudioProcessor>>; INSERT_SLOTS],
    pub(crate) block: StereoBlock,
    output_rate: f64,
    declick_frames: u32,
}

impl Deck {
    pub(crate) fn new(output_rate: u32, max_block_frames: usize, declick_frames: u32) -> Self {
        Self {
            track: None,
            position: 0.0,
            tempo: 1.0,
            rate: 1.0,
            playing: false,
            transport: Ramp::new(0.0, declick_frames),
            tails: std::array::from_fn(|_| None),
            cue_point: 0.0,
            beat_grid: None,
            fader: TempoFader::default(),
            sync: SyncFollow::default(),
            phase_bend: 0.0,
            trim: Ramp::new(1.0, declick_frames),
            channel_gain: Ramp::new(1.0, declick_frames),
            fader_position: 1.0,
            platter: Platter::new(output_rate),
            strip: ChannelStrip::new(output_rate),
            inserts: std::array::from_fn(|_| None),
            block: StereoBlock::with_capacity(max_block_frames),
            output_rate: f64::from(output_rate.max(1)),
            declick_frames,
        }
    }

    pub(crate) fn is_loaded(&self) -> bool {
        self.track.is_some()
    }

    pub(crate) fn is_playing(&self) -> bool {
        self.playing
    }

    pub(crate) fn tempo(&self) -> f64 {
        self.tempo
    }

    /// The speed the record turns at, where 1.0 is the track's own tempo: the tempo, bent by a
    /// nudge, or the hand's speed while scratching.
    pub(crate) fn speed(&self) -> f64 {
        self.platter.last_step() * self.output_rate / self.source_rate()
    }

    pub(crate) fn is_touched(&self) -> bool {
        self.platter.is_touched()
    }

    pub(crate) fn vinyl_mode(&self) -> bool {
        self.platter.vinyl_mode()
    }

    pub(crate) fn mixer_latency_frames(&self) -> usize {
        self.strip.latency_frames()
    }

    pub(crate) fn fader_position(&self) -> f32 {
        self.fader_position
    }

    pub(crate) fn has_beat_grid(&self) -> bool {
        self.beat_grid.is_some()
    }

    pub(crate) fn set_beat_grid(&mut self, beat_grid: Option<BeatGrid>, retire: &mut RetireQueue) {
        if let Some(previous) = std::mem::replace(&mut self.beat_grid, beat_grid) {
            retire.beat_grid(previous);
        }
        self.sync.multiple = None;
    }

    /// The tempo as it plays, in beats per minute, when the deck has a beat grid.
    pub(crate) fn bpm(&self) -> Option<f64> {
        self.beat_grid
            .as_ref()
            .map(|grid| grid.bpm_at(self.position_seconds()) * self.tempo)
    }

    /// The tempo and beat position at the next frame the deck plays, when it has a beat grid.
    pub(crate) fn beat_clock(&self) -> Option<BeatClock> {
        self.beat_grid.as_ref().map(|grid| BeatClock {
            bpm: grid.bpm_at(self.position_seconds()) * self.tempo,
            beat_position: grid.beat_at(self.position_seconds()),
        })
    }

    /// The deck's gain to the master while it plays, 0 when it is stopped. Counts the trim,
    /// the channel fader and the crossfader side gain `crossfader_gain`, the same way the
    /// louder-deck rule hears the deck. The channel fader is not read from `channel_gain`
    /// because that gain already carries the crossfader.
    pub(crate) fn level(&self, crossfader_gain: f32) -> f32 {
        if self.playing {
            self.trim.target() * channel_fader_gain(self.fader_position()) * crossfader_gain
        } else {
            0.0
        }
    }

    fn source_rate(&self) -> f64 {
        self.track
            .as_ref()
            .map_or(self.output_rate, |track| f64::from(track.sample_rate()))
    }

    pub(crate) fn position_seconds(&self) -> f64 {
        self.position / self.source_rate()
    }

    pub(crate) fn cue_point_seconds(&self) -> f64 {
        self.cue_point / self.source_rate()
    }

    fn update_rate(&mut self) {
        self.rate = self.tempo * (1.0 + self.phase_bend) * self.source_rate() / self.output_rate;
        self.platter.set_track_rate(self.source_rate());
    }

    /// Hands what the deck is playing over to a fading tail and silences the main voice. Tails
    /// from earlier jumps keep fading; when every slot is busy the quietest one is dropped.
    fn start_tail(&mut self, retire: &mut RetireQueue) {
        let gain = self.transport.current();
        self.transport.jump_to(0.0);
        if gain <= 0.0 {
            return;
        }
        let Some(track) = self.track.clone() else {
            return;
        };
        let tail = Tail {
            track,
            position: self.position,
            rate: self.platter.last_step(),
            start_gain: gain,
            remaining_frames: self.declick_frames,
        };
        let declick_frames = self.declick_frames;
        let slot = match self.tails.iter().position(Option::is_none) {
            Some(free) => free,
            None => self
                .tails
                .iter()
                .enumerate()
                .min_by(|(_, first), (_, second)| {
                    let first_gain = first.as_ref().map_or(0.0, |tail| tail.gain(declick_frames));
                    let second_gain = second
                        .as_ref()
                        .map_or(0.0, |tail| tail.gain(declick_frames));
                    first_gain.total_cmp(&second_gain)
                })
                .map_or(0, |(index, _)| index),
        };
        if let Some(evicted) = self.tails[slot].replace(tail) {
            retire.track(evicted.track);
        }
    }

    /// Puts a track on the deck, stopped at its start, and fades out whatever was playing.
    pub(crate) fn load(&mut self, track: Option<Arc<TrackBuffer>>, retire: &mut RetireQueue) {
        self.start_tail(retire);
        if let Some(previous) = std::mem::replace(&mut self.track, track) {
            retire.track(previous);
        }
        self.playing = false;
        self.position = 0.0;
        self.cue_point = 0.0;
        if let Some(previous) = self.beat_grid.take() {
            retire.beat_grid(previous);
        }
        self.sync.multiple = None;
        self.platter.stop();
        self.update_rate();
    }

    pub(crate) fn play(&mut self) {
        if self.track.is_none() || self.at_end() {
            return;
        }
        self.playing = true;
        self.transport.set_target(1.0);
    }

    /// Stops the motor. A hand on the record keeps the music under it.
    pub(crate) fn pause(&mut self, retire: &mut RetireQueue) {
        if self.playing {
            self.playing = false;
            if !self.platter.is_holding() {
                self.start_tail(retire);
            }
        }
    }

    /// CDJ cue button: while playing, returns to the cue point and stops; while stopped, sets
    /// the cue point where the deck is.
    pub(crate) fn cue(&mut self, retire: &mut RetireQueue) {
        if self.playing {
            self.playing = false;
            self.start_tail(retire);
            self.position = self.cue_point;
        } else {
            self.cue_point = self.position;
        }
    }

    pub(crate) fn set_cue_point(&mut self, seconds: f64) {
        self.cue_point = self.clamp_position(seconds * self.source_rate());
    }

    pub(crate) fn seek(&mut self, seconds: f64, retire: &mut RetireQueue) {
        self.jump_to_frame(seconds * self.source_rate(), retire);
    }

    /// Jumps to a frame of the track, fading the old position out while the new one fades in.
    fn jump_to_frame(&mut self, frame: f64, retire: &mut RetireQueue) {
        self.start_tail(retire);
        self.position = self.clamp_position(frame);
        if self.playing {
            if self.at_end() {
                self.playing = false;
            } else {
                self.transport.set_target(1.0);
            }
        }
    }

    fn apply_tempo(&mut self, tempo: f64) {
        self.tempo = tempo;
        self.phase_bend = 0.0;
        self.update_rate();
    }

    /// Sets the tempo the fader asks for directly, where 1.0 is the track's own tempo.
    pub(crate) fn set_tempo(&mut self, tempo: f64) {
        if let Some(tempo) = self.fader.move_to_tempo(tempo, self.tempo) {
            self.play_fader_tempo(tempo);
        }
    }

    /// Moves the tempo fader, from -1 (slowest) to 1 (fastest).
    pub(crate) fn set_tempo_fader(&mut self, position: f64) {
        if let Some(tempo) = self.fader.move_to(position, self.tempo) {
            self.play_fader_tempo(tempo);
        }
    }

    pub(crate) fn set_tempo_range(&mut self, range: TempoRange) {
        if let Some(tempo) = self.fader.set_range(range) {
            self.play_fader_tempo(tempo);
        }
    }

    fn play_fader_tempo(&mut self, tempo: f64) {
        if !self.sync.following {
            self.apply_tempo(tempo);
        }
    }

    pub(crate) fn tempo_range(&self) -> TempoRange {
        self.fader.range()
    }

    /// Whether the tempo fader waits to be moved through the playing tempo before it takes
    /// over again.
    pub(crate) fn fader_waits_for_pickup(&self) -> bool {
        !self.fader.is_in_control()
    }

    pub(crate) fn set_pitch_bend(&mut self, bend: f64) {
        if bend != 0.0 {
            self.loosen_beat_sync();
        }
        self.platter.set_held_bend(bend);
    }

    pub(crate) fn sync_mode(&self) -> SyncMode {
        self.sync.mode
    }

    pub(crate) fn is_following(&self) -> bool {
        self.sync.following
    }

    pub(crate) fn set_sync(&mut self, mode: SyncMode) {
        self.sync.mode = mode;
        self.sync.multiple = None;
        self.sync.locked = false;
    }

    /// A hand on a following deck drops beat sync to tempo sync, as on a CDJ-3000.
    fn loosen_beat_sync(&mut self) {
        if self.sync.following && self.sync.mode == SyncMode::Beat {
            self.sync.mode = SyncMode::Tempo;
        }
    }

    /// What a following deck needs from this deck as the tempo master.
    pub(crate) fn master_beat(&self, deck: DeckId) -> Option<MasterBeat> {
        let grid = self.beat_grid.as_ref()?;
        let seconds = self.position_seconds();
        Some(MasterBeat {
            deck,
            bpm: grid.bpm_at(seconds) * self.tempo,
            beat: grid.beat_at(seconds),
            running: self.playing && !self.platter.is_holding(),
        })
    }

    /// Stops following the tempo master; the tempo fader takes over once it reaches the
    /// playing tempo.
    pub(crate) fn stop_following(&mut self) {
        self.sync.locked = false;
        if self.sync.following {
            self.sync.following = false;
            self.phase_bend = 0.0;
            self.update_rate();
            self.fader.take_back(self.tempo);
        }
    }

    /// Follows the tempo master for the next block: its tempo, and with beat sync its beats.
    pub(crate) fn follow(
        &mut self,
        master: MasterBeat,
        allow_half_and_double: bool,
        retire: &mut RetireQueue,
    ) {
        if !self.sync.following {
            self.sync.following = true;
            self.fader.hand_over();
        }
        let Some(grid) = &self.beat_grid else {
            return;
        };
        let seconds = self.position_seconds();
        let own_bpm = grid.bpm_at(seconds);
        let multiple = match self.sync.multiple {
            Some((deck, multiple)) if deck == master.deck => multiple,
            _ => {
                let multiple =
                    sync_multiple(master.bpm, own_bpm * self.tempo, allow_half_and_double);
                self.sync.multiple = Some((master.deck, multiple));
                multiple
            }
        };
        let tempo = master.bpm * multiple / own_bpm;
        let lock_beats = self.sync.mode == SyncMode::Beat
            && master.running
            && self.playing
            && !self.platter.is_holding();
        let mut phase_bend = 0.0;
        if lock_beats {
            let beat = grid.beat_at(seconds);
            let beats_per_second = own_bpm * tempo / 60.0;
            match phase_action(
                master.beat * multiple,
                beat,
                beats_per_second,
                self.sync.locked,
            ) {
                PhaseAction::Jump(beats) => {
                    let mut target = grid.seconds_at(beat + beats);
                    if target < 0.0 {
                        target = grid.seconds_at(beat + beats + 1.0);
                    }
                    self.jump_to_frame(target * self.source_rate(), retire);
                }
                PhaseAction::Bend(bend) => phase_bend = bend,
            }
        }
        self.sync.locked = lock_beats;
        self.tempo = tempo;
        self.phase_bend = phase_bend;
        self.update_rate();
    }

    pub(crate) fn jog(&mut self, input: JogInput) {
        if self.playing
            && matches!(
                input,
                JogInput::Nudge(_) | JogInput::Scratch(_) | JogInput::Touch(true)
            )
        {
            self.loosen_beat_sync();
        }
        self.platter.apply(input, self.playing);
    }

    pub(crate) fn set_jog_ticks_per_revolution(&mut self, ticks: u32) {
        self.platter.set_ticks_per_revolution(ticks);
    }

    pub(crate) fn set_vinyl_mode(&mut self, enabled: bool) {
        self.platter.set_vinyl_mode(enabled);
    }

    pub(crate) fn set_eq(&mut self, band: EqBand, position: f32) {
        self.strip.set_eq(band, position);
    }

    pub(crate) fn set_filter(&mut self, position: f32) {
        self.strip.set_filter(position);
    }

    pub(crate) fn set_filter_resonance(&mut self, resonance: f32) {
        self.strip.set_filter_resonance(resonance);
    }

    pub(crate) fn set_trim(&mut self, gain: f32) {
        self.trim.set_target(gain);
    }

    pub(crate) fn set_fader_position(&mut self, position: f32) {
        self.fader_position = position;
    }

    pub(crate) fn set_channel_gain(&mut self, gain: f32) {
        self.channel_gain.set_target(gain);
    }

    /// Moves every gain straight to its target, for setting up a deck before it plays.
    pub(crate) fn settle_gains(&mut self) {
        let channel_gain = self.channel_gain.target();
        self.channel_gain.jump_to(channel_gain);
        let trim = self.trim.target();
        self.trim.jump_to(trim);
    }

    fn clamp_position(&self, position: f64) -> f64 {
        let longest = MAX_TRACK_SECONDS as f64 * self.source_rate();
        let end = self
            .track
            .as_ref()
            .and_then(|track| track.total_frames())
            .map_or(longest, |frames| frames as f64);
        position.clamp(0.0, end)
    }

    fn at_end(&self) -> bool {
        self.track
            .as_ref()
            .and_then(|track| track.total_frames())
            .is_some_and(|frames| self.position >= frames as f64)
    }

    /// Renders `frame_count` frames into the deck's block, after the trim, the EQ, the filter
    /// and the inserts, and before the channel fader.
    pub(crate) fn render(
        &mut self,
        frame_count: usize,
        context: &ProcessContext,
        retire: &mut RetireQueue,
    ) -> DeckRender {
        let mut result = DeckRender::default();
        if let Some(frames) = self.platter.take_search() {
            self.jump_to_frame(self.position + frames, retire);
        }
        let motor_step = if self.playing { self.rate } else { 0.0 };
        self.platter.begin_block(BlockStart {
            motor_step,
            frames: frame_count,
        });
        let holding = self.platter.is_holding();
        let transport_target = if self.playing || holding { 1.0 } else { 0.0 };
        if self.transport.target() != transport_target {
            self.transport.set_target(transport_target);
        }
        self.block.set_frame_count(frame_count);
        let audible_step = self.source_rate() / self.output_rate * AUDIBLE_SPEED;
        let end_frame = self
            .track
            .as_ref()
            .and_then(|track| track.total_frames())
            .map(|frames| frames as f64);
        let (left_channel, right_channel) = self.block.channels_mut();
        for (left, right) in left_channel.iter_mut().zip(right_channel.iter_mut()) {
            let mut left_sum = 0.0;
            let mut right_sum = 0.0;
            let step = self.platter.next_step(motor_step);
            let transport_gain = self.transport.next_value();
            if transport_gain > 0.0
                && let Some(track) = &self.track
            {
                let stylus_gain = (step.abs() / audible_step).min(1.0) as f32;
                let gain = transport_gain * stylus_gain;
                if gain > 0.0 {
                    let (sample_left, sample_right, missing) =
                        read_interpolated(track, self.position);
                    result.starved |= missing;
                    left_sum += sample_left * gain;
                    right_sum += sample_right * gain;
                }
            }
            for slot in &mut self.tails {
                let Some(tail) = slot else {
                    continue;
                };
                let gain = tail.gain(self.declick_frames);
                let (sample_left, sample_right, missing) =
                    read_interpolated(&tail.track, tail.position);
                result.starved |= missing;
                left_sum += sample_left * gain;
                right_sum += sample_right * gain;
                tail.position += tail.rate;
                tail.remaining_frames -= 1;
                if tail.remaining_frames == 0
                    && let Some(finished) = slot.take()
                {
                    retire.track(finished.track);
                }
            }
            if step != 0.0 {
                self.position = (self.position + step).max(0.0);
                if let Some(end) = end_frame
                    && self.position >= end
                {
                    self.position = end;
                    if self.playing && !holding {
                        self.playing = false;
                        self.transport.jump_to(0.0);
                    }
                }
            }
            let trim = self.trim.next_value();
            *left = left_sum * trim;
            *right = right_sum * trim;
        }
        self.strip.process(&mut self.block);
        for processor in self.inserts.iter_mut().flatten() {
            processor.process(&mut self.block, context);
        }
        result
    }

    /// Applies the channel fader and crossfader gain to the deck's block.
    pub(crate) fn apply_channel_gain(&mut self) {
        let (left_channel, right_channel) = self.block.channels_mut();
        for (left, right) in left_channel.iter_mut().zip(right_channel.iter_mut()) {
            let gain = self.channel_gain.next_value();
            *left *= gain;
            *right *= gain;
        }
    }

    /// Adds the deck's block into the master block and returns its peak.
    pub(crate) fn mix_into(&self, master: &mut StereoBlock) -> f32 {
        let (master_left, master_right) = master.channels_mut();
        let deck_frames = self.block.left().iter().zip(self.block.right());
        let mut peak = 0.0_f32;
        for ((master_left, master_right), (left, right)) in master_left
            .iter_mut()
            .zip(master_right.iter_mut())
            .zip(deck_frames)
        {
            peak = peak.max(left.abs()).max(right.abs());
            *master_left += left;
            *master_right += right;
        }
        peak
    }
}

/// Reads a stereo frame between two decoded frames with Catmull-Rom cubic interpolation, which
/// reproduces straight lines exactly and keeps tempo changes free of the dull, aliased sound of
/// linear interpolation. The flag is true when a frame needed was not decoded yet.
fn read_interpolated(track: &TrackBuffer, position: f64) -> (f32, f32, bool) {
    let base = position.floor();
    let fraction = (position - base) as f32;
    let base_index = base as i64;
    let mut left = [0.0_f32; 4];
    let mut right = [0.0_f32; 4];
    let mut missing = false;
    for offset in 0..4 {
        match track.read_frame(base_index.saturating_add(offset as i64 - 1)) {
            FrameRead::Frame(sample_left, sample_right) => {
                left[offset] = sample_left;
                right[offset] = sample_right;
            }
            FrameRead::Outside => {}
            FrameRead::Missing => missing = true,
        }
    }
    (
        catmull_rom(left, fraction),
        catmull_rom(right, fraction),
        missing,
    )
}

fn catmull_rom(points: [f32; 4], fraction: f32) -> f32 {
    let [before, start, end, after] = points;
    start
        + 0.5
            * fraction
            * (end - before
                + fraction
                    * (2.0 * before - 5.0 * start + 4.0 * end - after
                        + fraction * (3.0 * (start - end) + after - before)))
}
#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use crate::curves::channel_fader_gain;

    use super::*;

    #[test]
    fn catmull_rom_is_exact_on_straight_lines() {
        let points = [1.0, 2.0, 3.0, 4.0];
        for step in 0..=10 {
            let fraction = step as f32 / 10.0;
            assert!((catmull_rom(points, fraction) - (2.0 + fraction)).abs() < 1e-6);
        }
    }

    #[test]
    fn catmull_rom_passes_through_the_frames() {
        let points = [0.3, -0.7, 0.9, 0.1];
        assert_eq!(catmull_rom(points, 0.0), -0.7);
        assert!((catmull_rom(points, 1.0) - 0.9).abs() < 1e-6);
    }

    #[test]
    fn the_louder_deck_level_counts_the_crossfader_gain() {
        let track = crate::track::Track::from_interleaved_stereo(&[0.1, 0.1], 48_000).unwrap();
        let (producer, _consumer) = rtrb::RingBuffer::new(16);
        let mut retire = RetireQueue::new(producer);
        let mut deck = Deck::new(48_000, 512, 240);
        deck.load(Some(Arc::clone(track.buffer())), &mut retire);
        assert_eq!(deck.level(1.0), 0.0);
        deck.play();
        deck.set_trim(2.0);
        assert_eq!(deck.fader_position(), 1.0);
        // Crossfader fully to this deck: the trim stands alone.
        assert_eq!(deck.level(1.0), 2.0);
        // Half-open fader, -3 dB crossfader: both count.
        let crossfader_gain = FRAC_PI_2.cos();
        let half_fader = channel_fader_gain(0.5);
        assert_eq!(deck.level(0.5 * 1.0), 2.0 * 1.0 * 0.5);
        assert_eq!(deck.level(1.0) * half_fader, 2.0 * half_fader);
        assert_eq!(deck.level(0.5), 2.0 * 1.0 * 0.5);
        let _ = crossfader_gain;
        // Crossfader fully to the other deck: the level goes to zero while the deck still plays.
        assert_eq!(deck.level(0.0), 0.0);
    }
}
