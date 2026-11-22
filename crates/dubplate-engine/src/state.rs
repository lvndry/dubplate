use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

use dubplate_fx::BeatLength;

use crate::deck::DeckId;
use crate::sync::SyncMode;
use crate::tempo::TempoRange;
use crate::{DECK_COUNT, EFFECT_UNIT_COUNT};

const WARMUP_CALLBACKS: u64 = 20;
const NO_LATENCY: u64 = u64::MAX;

/// State the audio thread publishes and the control side reads, all through atomics so neither
/// side ever waits for the other.
pub(crate) struct SharedState {
    pub(crate) decks: [DeckShared; DECK_COUNT],
    pub(crate) master_peak: AtomicU32,
    pub(crate) headphone_peak: AtomicU32,
    pub(crate) master_limiting: AtomicU32,
    pub(crate) sampler_peak: AtomicU32,
    pub(crate) sampler_playing: AtomicU32,
    pub(crate) sampler_loaded: AtomicU32,
    pub(crate) preview_playing: AtomicBool,
    pub(crate) preview_position_seconds: AtomicF64,
    pub(crate) effect_sounding: [AtomicBool; EFFECT_UNIT_COUNT],
    pub(crate) tempo_master: AtomicDeck,
    pub(crate) stats: SharedStats,
}

#[derive(Default)]
pub(crate) struct DeckShared {
    pub(crate) loaded: AtomicBool,
    pub(crate) playing: AtomicBool,
    pub(crate) position_seconds: AtomicF64,
    pub(crate) cue_point_seconds: AtomicF64,
    pub(crate) tempo: AtomicF64,
    pub(crate) speed: AtomicF64,
    pub(crate) bpm: AtomicF64,
    pub(crate) tempo_range: AtomicU8,
    pub(crate) sync_mode: AtomicU8,
    pub(crate) following: AtomicBool,
    pub(crate) fader_waits_for_pickup: AtomicBool,
    pub(crate) loop_start_seconds: AtomicF64,
    pub(crate) loop_end_seconds: AtomicF64,
    pub(crate) loop_active: AtomicBool,
    pub(crate) loop_length: AtomicU32,
    pub(crate) pending_loop_start_seconds: AtomicF64,
    pub(crate) slip_enabled: AtomicBool,
    pub(crate) slip_shadow_seconds: AtomicF64,
    pub(crate) touched: AtomicBool,
    pub(crate) vinyl_mode: AtomicBool,
    pub(crate) peak: AtomicU32,
    pub(crate) pre_fader_peak: AtomicU32,
}

pub(crate) struct SharedStats {
    pub(crate) sample_rate: u32,
    pub(crate) callbacks: AtomicU64,
    pub(crate) late_callbacks: AtomicU64,
    pub(crate) starved_callbacks: AtomicU64,
    pub(crate) device_gaps: AtomicU64,
    pub(crate) device_errors: AtomicU64,
    pub(crate) last_callback_frames: AtomicU32,
    pub(crate) max_callback_frames: AtomicU32,
    pub(crate) max_load_micro: AtomicU64,
    pub(crate) output_latency_nanos: AtomicU64,
    pub(crate) mixer_latency_frames: AtomicU32,
}

/// An `f64` stored as bits in an `AtomicU64`.
#[derive(Default)]
pub(crate) struct AtomicF64(AtomicU64);

impl AtomicF64 {
    pub(crate) fn store(&self, value: f64) {
        self.0.store(value.to_bits(), Ordering::Relaxed);
    }

    pub(crate) fn load(&self) -> f64 {
        f64::from_bits(self.0.load(Ordering::Relaxed))
    }
}

/// An `Option<DeckId>` stored as 0 for none and the deck's index plus one.
#[derive(Default)]
pub(crate) struct AtomicDeck(AtomicU8);

impl AtomicDeck {
    pub(crate) fn store(&self, deck: Option<DeckId>) {
        let encoded = deck.map_or(0, |deck| deck.index() + 1);
        self.0
            .store(u8::try_from(encoded).unwrap_or(0), Ordering::Relaxed);
    }

    pub(crate) fn load(&self) -> Option<DeckId> {
        let encoded = usize::from(self.0.load(Ordering::Relaxed));
        encoded
            .checked_sub(1)
            .and_then(|index| DeckId::ALL.get(index).copied())
    }
}

/// A loop length packed in one integer, numerator above denominator, 0 for none.
pub(crate) fn encode_loop_length(length: Option<BeatLength>) -> u32 {
    length.map_or(0, |length| {
        let (numerator, denominator) = length.fraction_parts();
        u32::from(numerator) << 16 | u32::from(denominator)
    })
}

pub(crate) fn decode_loop_length(encoded: u32) -> Option<BeatLength> {
    let numerator = u16::try_from(encoded >> 16).ok()?;
    let denominator = u16::try_from(encoded & 0xFFFF).ok()?;
    BeatLength::new(numerator, denominator)
}

/// Raises a peak meter. Non-negative `f32` values order the same way as their bits, so an
/// integer maximum on the bits is a float maximum.
pub(crate) fn raise_peak(meter: &AtomicU32, peak: f32) {
    meter.fetch_max(peak.abs().to_bits(), Ordering::Relaxed);
}

pub(crate) fn take_peak(meter: &AtomicU32) -> f32 {
    f32::from_bits(meter.swap(0, Ordering::Relaxed))
}

impl SharedState {
    pub(crate) fn new(sample_rate: u32) -> Self {
        Self {
            decks: std::array::from_fn(|_| {
                let deck = DeckShared::default();
                deck.tempo.store(1.0);
                deck.bpm.store(f64::NAN);
                deck.loop_start_seconds.store(f64::NAN);
                deck.loop_end_seconds.store(f64::NAN);
                deck.pending_loop_start_seconds.store(f64::NAN);
                deck.slip_shadow_seconds.store(f64::NAN);
                deck.tempo_range
                    .store(TempoRange::default().index(), Ordering::Relaxed);
                deck.vinyl_mode.store(true, Ordering::Relaxed);
                deck
            }),
            master_peak: AtomicU32::new(0),
            headphone_peak: AtomicU32::new(0),
            master_limiting: AtomicU32::new(0),
            sampler_peak: AtomicU32::new(0),
            sampler_playing: AtomicU32::new(0),
            sampler_loaded: AtomicU32::new(0),
            preview_playing: AtomicBool::new(false),
            preview_position_seconds: AtomicF64::default(),
            effect_sounding: std::array::from_fn(|_| AtomicBool::new(false)),
            tempo_master: AtomicDeck::default(),
            stats: SharedStats {
                sample_rate,
                callbacks: AtomicU64::new(0),
                late_callbacks: AtomicU64::new(0),
                starved_callbacks: AtomicU64::new(0),
                device_gaps: AtomicU64::new(0),
                device_errors: AtomicU64::new(0),
                last_callback_frames: AtomicU32::new(0),
                max_callback_frames: AtomicU32::new(0),
                max_load_micro: AtomicU64::new(0),
                output_latency_nanos: AtomicU64::new(NO_LATENCY),
                mixer_latency_frames: AtomicU32::new(0),
            },
        }
    }
}

impl SharedStats {
    /// Records one callback. A callback is late when computing it took longer than the audio it
    /// produced lasts, which means the device ran out of samples.
    pub(crate) fn record_callback(&self, frames: usize, elapsed: Duration, starved: bool) {
        let frames_u32 = u32::try_from(frames).unwrap_or(u32::MAX);
        self.callbacks.fetch_add(1, Ordering::Relaxed);
        self.last_callback_frames
            .store(frames_u32, Ordering::Relaxed);
        self.max_callback_frames
            .fetch_max(frames_u32, Ordering::Relaxed);
        if frames == 0 || self.sample_rate == 0 {
            return;
        }
        let budget = frames as f64 / f64::from(self.sample_rate);
        let load = elapsed.as_secs_f64() / budget;
        self.max_load_micro
            .fetch_max((load * 1_000_000.0) as u64, Ordering::Relaxed);
        if load > 1.0 {
            self.late_callbacks.fetch_add(1, Ordering::Relaxed);
        }
        if starved {
            self.starved_callbacks.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(crate) fn record_output_latency(&self, latency: Duration) {
        let nanos = u64::try_from(latency.as_nanos()).unwrap_or(u64::MAX - 1);
        self.output_latency_nanos.store(nanos, Ordering::Relaxed);
    }

    pub(crate) fn snapshot(&self) -> EngineStats {
        let buffer_frames = self.last_callback_frames.load(Ordering::Relaxed);
        let latency_nanos = self.output_latency_nanos.load(Ordering::Relaxed);
        let milliseconds_per_frame = if self.sample_rate == 0 {
            0.0
        } else {
            1000.0 / f64::from(self.sample_rate)
        };
        EngineStats {
            sample_rate: self.sample_rate,
            buffer_frames,
            max_buffer_frames: self.max_callback_frames.load(Ordering::Relaxed),
            buffer_latency_ms: f64::from(buffer_frames) * milliseconds_per_frame,
            mixer_latency_ms: f64::from(self.mixer_latency_frames.load(Ordering::Relaxed))
                * milliseconds_per_frame,
            output_latency_ms: (latency_nanos != NO_LATENCY)
                .then(|| latency_nanos as f64 / 1_000_000.0),
            callbacks: self.callbacks.load(Ordering::Relaxed),
            late_callbacks: self.late_callbacks.load(Ordering::Relaxed),
            starved_callbacks: self.starved_callbacks.load(Ordering::Relaxed),
            device_gaps: self.device_gaps.load(Ordering::Relaxed),
            device_errors: self.device_errors.load(Ordering::Relaxed),
            max_callback_load: self.max_load_micro.load(Ordering::Relaxed) as f64 / 1_000_000.0,
        }
    }
}

/// Watches the device's callback timestamps for missed cycles: when the time between two
/// callbacks is much longer than the audio the previous one produced, the device played
/// something other than Dubplate's audio in between.
#[derive(Debug, Default)]
pub(crate) struct GapDetector {
    previous_callback: Option<Duration>,
    previous_frames: usize,
    observed_callbacks: u64,
}

impl GapDetector {
    pub(crate) fn observe(
        &mut self,
        callback_time: Duration,
        frames: usize,
        sample_rate: u32,
    ) -> bool {
        self.observed_callbacks += 1;
        let previous = self.previous_callback.replace(callback_time);
        let previous_frames = std::mem::replace(&mut self.previous_frames, frames);
        let Some(previous) = previous else {
            return false;
        };
        if self.observed_callbacks <= WARMUP_CALLBACKS || sample_rate == 0 {
            return false;
        }
        let expected = previous_frames as f64 / f64::from(sample_rate);
        let interval = callback_time.saturating_sub(previous).as_secs_f64();
        interval > expected * 1.5 + 0.001
    }
}

/// Audio thread health since the engine started.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EngineStats {
    /// Output sample rate in Hz.
    pub sample_rate: u32,
    /// Frames in the most recent device callback.
    pub buffer_frames: u32,
    /// Largest number of frames the device asked for in one callback.
    pub max_buffer_frames: u32,
    /// Duration of one buffer in milliseconds: the latency the buffer adds.
    pub buffer_latency_ms: f64,
    /// Time the mixer delays every deck by: the channel EQ, which keeps its bands in phase, and
    /// the master limiter's look-ahead. Add it to the output latency to line a waveform up with
    /// what is heard.
    pub mixer_latency_ms: f64,
    /// Time from the callback to the moment its first sample reaches the speakers, as the
    /// device reports it. Includes the buffer and the device's own latency.
    pub output_latency_ms: Option<f64>,
    /// Callbacks served.
    pub callbacks: u64,
    /// Callbacks that took longer to compute than the audio they produced lasts.
    pub late_callbacks: u64,
    /// Callbacks where a playing deck reached audio that was not decoded yet.
    pub starved_callbacks: u64,
    /// Device cycles that came much later than the previous buffer's length, meaning the
    /// device skipped a cycle.
    pub device_gaps: u64,
    /// Errors the audio device reported, such as a disconnect or an underrun it detected.
    pub device_errors: u64,
    /// Highest share of a buffer's duration spent computing it, where 1.0 is the whole budget.
    pub max_callback_load: f64,
}

impl EngineStats {
    /// Every callback that could have been heard as a dropout.
    pub fn overruns(&self) -> u64 {
        self.late_callbacks + self.starved_callbacks + self.device_gaps + self.device_errors
    }
}

/// What a deck is doing, as the audio thread last published it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeckState {
    /// A track is on the deck.
    pub loaded: bool,
    /// The deck is playing.
    pub playing: bool,
    /// Playback position in seconds from the start of the track.
    pub position_seconds: f64,
    /// Cue point in seconds from the start of the track.
    pub cue_point_seconds: f64,
    /// Playback speed, where 1.0 is the track's own tempo.
    pub tempo: f64,
}

/// A deck's tempo and sync, as the audio thread last published them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TempoState {
    /// The tempo as it plays, in beats per minute, without nudges or pitch bends. `None`
    /// without a beat grid.
    pub bpm: Option<f64>,
    /// How far the tempo fader reaches.
    pub tempo_range: TempoRange,
    /// The SYNC setting. Nudging or scratching a following deck turns beat sync into tempo
    /// sync.
    pub sync: SyncMode,
    /// The deck plays at the tempo master's tempo this moment: it is synced and another deck
    /// leads.
    pub following: bool,
    /// The deck is the tempo master that synced decks follow.
    pub is_master: bool,
    /// The tempo fader no longer matches the tempo, after sync or a range change, and waits
    /// to be moved through it before it takes over.
    pub fader_waits_for_pickup: bool,
}

/// A deck's loop, as the audio thread last published it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoopState {
    /// Where the loop starts, in seconds from the start of the track. `None` without a loop.
    pub start_seconds: Option<f64>,
    /// Where the loop ends, in seconds from the start of the track. `None` without a loop.
    pub end_seconds: Option<f64>,
    /// The playhead goes round the loop. An exited loop stays for RELOOP.
    pub active: bool,
    /// The length of a beat loop. `None` for a loop set with loop in and loop out.
    pub length: Option<BeatLength>,
    /// A loop in point waiting for its loop out, in seconds.
    pub pending_start_seconds: Option<f64>,
}

/// A deck's slip mode, as the audio thread last published it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlipState {
    /// The SLIP button: scratches and loops slip, not only rolls.
    pub enabled: bool,
    /// Where the track would be now, in seconds, while a roll, scratch or loop slips: the
    /// shadow playhead a waveform draws, and where the deck lands when it lets go.
    pub shadow_seconds: Option<f64>,
}

/// What a deck's jog wheel is doing, as the audio thread last published it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlatterState {
    /// The speed the record actually turns at, where 1.0 is the track's own tempo: the tempo,
    /// bent while the jog nudges, or the hand's speed while scratching. Negative backwards, 0
    /// when stopped. A platter display turns at this speed.
    pub speed: f64,
    /// A hand is on the platter top, holding the record.
    pub touched: bool,
    /// Touching the platter top holds the record, as in a CDJ's vinyl mode.
    pub vinyl_mode: bool,
}

/// Peak levels since the previous reading, as linear sample values where 1.0 is full scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Meters {
    /// Peak of each deck after its fader, the crossfader and the effect units on it.
    pub deck_peaks: [f32; DECK_COUNT],
    /// Peak of each channel before its fader, after the trim, the EQ and the filter: what a
    /// DJ mixer's channel level meters show.
    pub channel_peaks: [f32; DECK_COUNT],
    /// Peak of the master output.
    pub master_peak: f32,
    /// Peak of the headphone output.
    pub headphone_peak: f32,
    /// The lowest gain the master limiter applied, where 1.0 means it did nothing. Below 1.0
    /// the master was pushed into the limiter: a LIMIT light.
    pub master_limiter_gain: f32,
    /// Peak of the sampler bus, after the sampler volume.
    pub sampler_peak: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn late_callbacks_are_those_slower_than_their_audio() {
        let stats = SharedState::new(48_000).stats;
        stats.record_callback(480, Duration::from_millis(5), false);
        stats.record_callback(480, Duration::from_millis(11), false);
        stats.record_callback(480, Duration::from_millis(1), true);
        let snapshot = stats.snapshot();
        assert_eq!(snapshot.callbacks, 3);
        assert_eq!(snapshot.late_callbacks, 1);
        assert_eq!(snapshot.starved_callbacks, 1);
        assert_eq!(snapshot.overruns(), 2);
        assert!((snapshot.buffer_latency_ms - 10.0).abs() < 1e-9);
        assert!((snapshot.max_callback_load - 1.1).abs() < 1e-3);
    }

    #[test]
    fn gap_detector_flags_a_missed_cycle_after_warmup() {
        let mut detector = GapDetector::default();
        let period = Duration::from_micros(10_000);
        let mut time = Duration::ZERO;
        for _ in 0..=WARMUP_CALLBACKS {
            assert!(!detector.observe(time, 480, 48_000));
            time += period;
        }
        assert!(!detector.observe(time, 480, 48_000));
        time += period * 2;
        assert!(detector.observe(time, 480, 48_000));
    }

    #[test]
    fn peak_meter_keeps_the_highest_value_until_read() {
        let meter = AtomicU32::new(0);
        raise_peak(&meter, 0.5);
        raise_peak(&meter, -0.75);
        raise_peak(&meter, 0.25);
        assert_eq!(take_peak(&meter), 0.75);
        assert_eq!(take_peak(&meter), 0.0);
    }
}
