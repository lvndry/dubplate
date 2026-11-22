use std::f64::consts::TAU;

/// Jog ticks per platter revolution of the Pioneer DDJ-FLX4, and the default.
pub const DEFAULT_JOG_TICKS_PER_REVOLUTION: u32 = 720;

/// Seconds of the track one platter revolution covers when scratching: a record at 33⅓ RPM.
const SCRATCH_SECONDS_PER_REVOLUTION: f64 = 1.8;
/// Seconds of the track one revolution of the jog's edge moves the music by when nudging.
const NUDGE_SECONDS_PER_REVOLUTION: f64 = 0.3;
/// Seconds of the track one revolution moves with SHIFT held.
const SEARCH_SECONDS_PER_REVOLUTION: f64 = 15.0;
/// The largest tempo bend a nudge reaches, as a share of the tempo.
const MAX_NUDGE_BEND: f64 = 0.9;
/// How quickly a nudge's bend follows the wheel and returns once it stops.
const NUDGE_SMOOTHING_SECONDS: f64 = 0.05;
/// How closely the playhead follows the hand: a cutoff of the speed's smoothing.
const HAND_RESPONSE_HZ: f64 = 30.0;
/// How quickly the music returns to the motor's speed once the hand lets go.
const RELEASE_SECONDS: f64 = 0.02;
/// How long a scrub on a stopped deck waits for the wheel to move again before letting go.
const SCRUB_RELEASE_SECONDS: f64 = 0.06;
/// Speeds, in track frames per output frame, below which the platter counts as still.
const STILL_STEP: f64 = 1e-4;

/// Something the DJ did with a jog wheel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum JogInput {
    /// The platter top was touched or let go.
    Touch(bool),
    /// The platter turned while touched: the hand moves the record.
    Scratch(i32),
    /// The jog's edge turned: bend the tempo, or move the playhead on a stopped deck.
    Nudge(i32),
    /// The jog turned with SHIFT held: move quickly through the track.
    Search(i32),
}

/// The hand on the record. Each block, the platter's movement since the previous block
/// becomes the speed the hand asks for over the next one; the playhead follows that speed
/// through a critically damped smoothing, which never reverses and never loses a frame, so the
/// music ends exactly where the platter puts it.
#[derive(Debug, Clone, Copy)]
struct Hand {
    /// How far the platter turned since the previous block, in track frames.
    pending: f64,
    /// The speed the hand asks for over this block, in track frames per output frame.
    wanted: f64,
    /// The first and second smoothing stages of the playhead's speed.
    smoothing: f64,
    velocity: f64,
    /// Whether this is a scrub on a stopped deck rather than a touched platter.
    scrub: bool,
    /// Whether the hand let go; the motor takes over at the next block.
    released: bool,
    /// Frames since the last tick, for a scrub to let go.
    idle_frames: f64,
}

/// What a deck needs to tell its platter at the start of a block.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BlockStart {
    /// The speed the motor turns the record at, in track frames per output frame: the tempo
    /// while playing, 0 while stopped.
    pub(crate) motor_step: f64,
    /// Frames in the block.
    pub(crate) frames: usize,
}

/// A deck's platter: it turns the motor's speed, the hand and the jog's edge into how far the
/// playhead moves on each output frame.
#[derive(Debug, Clone)]
pub(crate) struct Platter {
    output_rate: f64,
    ticks_per_revolution: f64,
    track_rate: f64,
    vinyl_mode: bool,
    hand: Option<Hand>,
    nudge_ticks: i64,
    nudge_target: f64,
    nudge_velocity: f64,
    held_bend_target: f64,
    held_bend: f64,
    search_ticks: i64,
    carry: f64,
    last_step: f64,
    hand_coefficient: f64,
    nudge_coefficient: f64,
    release_decay: f64,
}

impl Platter {
    pub(crate) fn new(output_rate: u32) -> Self {
        let output_rate = f64::from(output_rate.max(1));
        Self {
            output_rate,
            ticks_per_revolution: f64::from(DEFAULT_JOG_TICKS_PER_REVOLUTION),
            track_rate: output_rate,
            vinyl_mode: true,
            hand: None,
            nudge_ticks: 0,
            nudge_target: 0.0,
            nudge_velocity: 0.0,
            held_bend_target: 0.0,
            held_bend: 0.0,
            search_ticks: 0,
            carry: 0.0,
            last_step: 0.0,
            hand_coefficient: 1.0 - (-TAU * HAND_RESPONSE_HZ / output_rate).exp(),
            nudge_coefficient: 1.0 - (-1.0 / (NUDGE_SMOOTHING_SECONDS * output_rate)).exp(),
            release_decay: (-1.0 / (RELEASE_SECONDS * output_rate)).exp(),
        }
    }

    pub(crate) fn set_track_rate(&mut self, track_rate: f64) {
        self.track_rate = track_rate;
    }

    pub(crate) fn set_ticks_per_revolution(&mut self, ticks: u32) {
        self.ticks_per_revolution = f64::from(ticks.max(1));
    }

    /// Vinyl mode on makes touching the platter top hold the record; off, the top nudges like
    /// the edge.
    pub(crate) fn set_vinyl_mode(&mut self, enabled: bool) {
        self.vinyl_mode = enabled;
        if !enabled {
            self.let_go();
        }
    }

    pub(crate) fn vinyl_mode(&self) -> bool {
        self.vinyl_mode
    }

    /// Whether a hand holds the record, or the record is still settling after one let go:
    /// the music sounds whether or not the deck plays.
    pub(crate) fn is_holding(&self) -> bool {
        self.hand.is_some() || self.carry != 0.0
    }

    /// Whether a hand is on the platter top.
    pub(crate) fn is_touched(&self) -> bool {
        self.hand.is_some_and(|hand| !hand.scrub && !hand.released)
    }

    /// How far the playhead moved on the last frame, in track frames.
    pub(crate) fn last_step(&self) -> f64 {
        self.last_step
    }

    /// Forgets the hand and every bend, for a new track.
    pub(crate) fn stop(&mut self) {
        self.hand = None;
        self.nudge_ticks = 0;
        self.nudge_target = 0.0;
        self.nudge_velocity = 0.0;
        self.search_ticks = 0;
        self.carry = 0.0;
        self.last_step = 0.0;
    }

    fn track_frames_per_tick(&self, seconds_per_revolution: f64) -> f64 {
        self.track_rate * seconds_per_revolution / self.ticks_per_revolution
    }

    /// Takes one jog event. Turns of the top or the edge move the record while a hand holds it,
    /// bend the tempo of a playing deck and scrub a stopped one.
    pub(crate) fn apply(&mut self, input: JogInput, playing: bool) {
        match input {
            JogInput::Touch(true) if self.vinyl_mode => self.grab(false),
            JogInput::Touch(true) => {}
            JogInput::Touch(false) => {
                if self.is_touched() {
                    self.let_go();
                }
            }
            JogInput::Scratch(ticks) | JogInput::Nudge(ticks) => {
                if self.hand.is_some() {
                    self.turn_hand(ticks);
                } else if playing {
                    self.nudge_ticks += i64::from(ticks);
                } else {
                    self.grab(true);
                    self.turn_hand(ticks);
                }
            }
            JogInput::Search(ticks) => self.search_ticks += i64::from(ticks),
        }
    }

    /// Puts a hand on the record. The music keeps the speed it had and slows down to the
    /// hand's along the smoothing, so a hand landing on a playing record stops it in a few
    /// milliseconds without rocking back.
    fn grab(&mut self, scrub: bool) {
        if let Some(hand) = &mut self.hand {
            hand.scrub &= scrub;
            hand.released = false;
            return;
        }
        self.carry = 0.0;
        self.nudge_velocity = 0.0;
        self.nudge_target = 0.0;
        self.nudge_ticks = 0;
        self.hand = Some(Hand {
            pending: 0.0,
            wanted: 0.0,
            smoothing: self.last_step,
            velocity: self.last_step,
            scrub,
            released: false,
            idle_frames: 0.0,
        });
    }

    fn turn_hand(&mut self, ticks: i32) {
        let frames_per_tick = self.track_frames_per_tick(SCRATCH_SECONDS_PER_REVOLUTION);
        if let Some(hand) = &mut self.hand {
            hand.pending += f64::from(ticks) * frames_per_tick;
            hand.idle_frames = 0.0;
        }
    }

    fn let_go(&mut self) {
        if let Some(hand) = &mut self.hand {
            hand.released = true;
        }
    }

    /// Lets the record run at the motor's speed at once, without the inertia left by a hand that
    /// just let go, for a slip landing back on the shadow playhead.
    pub(crate) fn drop_carry(&mut self) {
        self.carry = 0.0;
    }

    /// Bends the tempo by `bend`, a share of the tempo, until set back to 0, like holding a
    /// pitch bend button. The bend glides in and out.
    pub(crate) fn set_held_bend(&mut self, bend: f64) {
        self.held_bend_target = bend;
    }

    /// Takes the SHIFT search the wheel asked for since the previous block, in track frames.
    pub(crate) fn take_search(&mut self) -> Option<f64> {
        if self.search_ticks == 0 {
            return None;
        }
        let frames =
            self.search_ticks as f64 * self.track_frames_per_tick(SEARCH_SECONDS_PER_REVOLUTION);
        self.search_ticks = 0;
        Some(frames)
    }

    /// Updates the platter from the ticks that arrived since the previous block.
    pub(crate) fn begin_block(&mut self, block: BlockStart) {
        let frames = block.frames.max(1) as f64;
        let nudge_revolutions = self.nudge_ticks as f64 / self.ticks_per_revolution;
        self.nudge_target = nudge_revolutions * self.output_rate / frames;
        self.nudge_ticks = 0;
        let scrub_release_frames = SCRUB_RELEASE_SECONDS * self.output_rate;
        let Some(hand) = &mut self.hand else {
            return;
        };
        if hand.released {
            self.carry = hand.velocity - block.motor_step;
            self.hand = None;
            return;
        }
        hand.wanted = hand.pending / frames;
        if hand.pending == 0.0 {
            hand.idle_frames += frames;
        }
        hand.pending = 0.0;
        let still = hand.velocity.abs() < STILL_STEP && hand.smoothing.abs() < STILL_STEP;
        if hand.scrub && hand.idle_frames >= scrub_release_frames && still {
            hand.released = true;
        }
    }

    /// How far the playhead moves on the next output frame, in track frames.
    #[inline]
    pub(crate) fn next_step(&mut self, motor_step: f64) -> f64 {
        let step = match &mut self.hand {
            Some(hand) => {
                hand.smoothing += (hand.wanted - hand.smoothing) * self.hand_coefficient;
                hand.velocity += (hand.smoothing - hand.velocity) * self.hand_coefficient;
                hand.velocity
            }
            None => {
                self.nudge_velocity +=
                    (self.nudge_target - self.nudge_velocity) * self.nudge_coefficient;
                self.held_bend += (self.held_bend_target - self.held_bend) * self.nudge_coefficient;
                let bend = (self.nudge_velocity * NUDGE_SECONDS_PER_REVOLUTION + self.held_bend)
                    .clamp(-MAX_NUDGE_BEND, MAX_NUDGE_BEND);
                if self.carry != 0.0 {
                    self.carry *= self.release_decay;
                    if self.carry.abs() < STILL_STEP * 0.01 {
                        self.carry = 0.0;
                    }
                }
                motor_step * (1.0 + bend) + self.carry
            }
        };
        self.last_step = step;
        step
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;
    const BLOCK: usize = 256;

    struct Rig {
        platter: Platter,
        position: f64,
        steps: Vec<f64>,
    }

    impl Rig {
        fn new() -> Self {
            let mut platter = Platter::new(RATE);
            platter.set_track_rate(f64::from(RATE));
            Self {
                platter,
                position: 0.0,
                steps: Vec::new(),
            }
        }

        fn run(
            &mut self,
            blocks: usize,
            motor_step: f64,
            ticks_per_block: i32,
            input: fn(i32) -> JogInput,
        ) {
            for _ in 0..blocks {
                if ticks_per_block != 0 {
                    self.platter.apply(input(ticks_per_block), motor_step > 0.0);
                }
                self.platter.begin_block(BlockStart {
                    motor_step,
                    frames: BLOCK,
                });
                for _ in 0..BLOCK {
                    let step = self.platter.next_step(motor_step);
                    self.position += step;
                    self.steps.push(step);
                }
            }
        }
    }

    #[test]
    fn a_hand_on_a_playing_record_stops_it_without_rocking_back() {
        let mut rig = Rig::new();
        rig.run(20, 1.0, 0, JogInput::Nudge);
        rig.platter.apply(JogInput::Touch(true), true);
        rig.steps.clear();
        rig.run(20, 1.0, 0, JogInput::Nudge);
        assert!(
            rig.steps.iter().all(|step| *step >= -1e-9),
            "the record rocked back"
        );
        let halved_after = rig.steps.iter().position(|step| *step < 0.5).unwrap();
        assert!(
            halved_after < RATE as usize / 100,
            "took {halved_after} frames to halve"
        );
        let stopped_after = rig.steps.iter().position(|step| *step < 0.01).unwrap();
        assert!(
            stopped_after < RATE as usize / 25,
            "took {stopped_after} frames to stop"
        );
        assert!(rig.steps.last().unwrap().abs() < 1e-6);
    }

    #[test]
    fn a_scratch_moves_the_music_with_the_platter_and_back_to_the_same_frame() {
        let mut rig = Rig::new();
        rig.platter.apply(JogInput::Touch(true), false);
        rig.run(10, 0.0, 0, JogInput::Scratch);
        let start = rig.position;
        rig.run(60, 0.0, 12, JogInput::Scratch);
        rig.run(40, 0.0, 0, JogInput::Scratch);
        let forward = rig.position - start;
        let expected = 720.0 * f64::from(RATE) * SCRATCH_SECONDS_PER_REVOLUTION / 720.0;
        assert!(
            (forward - expected).abs() < 1.0,
            "moved {forward}, expected {expected}"
        );
        rig.run(60, 0.0, -12, JogInput::Scratch);
        rig.run(60, 0.0, 0, JogInput::Scratch);
        assert!(
            (rig.position - start).abs() < 0.5,
            "ended {} frames away",
            rig.position - start
        );
    }

    #[test]
    fn a_steady_scratch_plays_at_a_steady_speed() {
        let mut rig = Rig::new();
        rig.platter.apply(JogInput::Touch(true), false);
        let ticks_per_block = 2;
        rig.run(200, 0.0, ticks_per_block, JogInput::Scratch);
        let expected =
            f64::from(ticks_per_block) * SCRATCH_SECONDS_PER_REVOLUTION * f64::from(RATE)
                / 720.0
                / BLOCK as f64;
        let settled = &rig.steps[rig.steps.len() - 20 * BLOCK..];
        for step in settled {
            assert!(
                (step / expected - 1.0).abs() < 0.01,
                "speed {step}, expected {expected}"
            );
        }
    }

    #[test]
    fn letting_go_returns_smoothly_to_the_tempo() {
        let mut rig = Rig::new();
        rig.run(4, 1.04, 0, JogInput::Nudge);
        rig.platter.apply(JogInput::Touch(true), true);
        rig.run(20, 1.04, 0, JogInput::Nudge);
        rig.platter.apply(JogInput::Touch(false), true);
        rig.steps.clear();
        rig.run(40, 1.04, 0, JogInput::Nudge);
        let largest_change = rig
            .steps
            .windows(2)
            .map(|pair| (pair[1] - pair[0]).abs())
            .fold(0.0, f64::max);
        assert!(largest_change < 0.01, "speed jumped by {largest_change}");
        assert!((rig.steps.last().unwrap() - 1.04).abs() < 1e-4);
    }

    #[test]
    fn a_nudge_moves_the_music_by_the_same_amount_however_it_is_split() {
        let mut rig = Rig::new();
        rig.run(10, 1.0, 0, JogInput::Nudge);
        let before = rig.position;
        rig.run(10, 1.0, 9, JogInput::Nudge);
        rig.run(200, 1.0, 0, JogInput::Nudge);
        let moved_ahead = rig.position - before - 210.0 * BLOCK as f64;
        let expected = 90.0 / 720.0 * NUDGE_SECONDS_PER_REVOLUTION * f64::from(RATE);
        assert!(
            (moved_ahead - expected).abs() < 2.0,
            "{moved_ahead} vs {expected}"
        );
        assert!((rig.steps.last().unwrap() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn turning_a_stopped_deck_scrubs_and_lets_go_by_itself() {
        let mut rig = Rig::new();
        rig.run(5, 0.0, 6, JogInput::Nudge);
        assert!(rig.platter.is_holding());
        rig.run(60, 0.0, 0, JogInput::Nudge);
        let expected = 30.0 * SCRATCH_SECONDS_PER_REVOLUTION * f64::from(RATE) / 720.0;
        assert!((rig.position - expected).abs() < 1.0);
        assert!(!rig.platter.is_holding());
    }

    #[test]
    fn search_moves_fifteen_seconds_per_revolution() {
        let mut platter = Platter::new(RATE);
        platter.set_track_rate(44_100.0);
        platter.apply(JogInput::Search(360), true);
        assert_eq!(platter.take_search(), Some(7.5 * 44_100.0));
        assert_eq!(platter.take_search(), None);
    }

    #[test]
    fn with_vinyl_mode_off_a_touch_does_not_hold_the_record() {
        let mut platter = Platter::new(RATE);
        platter.set_vinyl_mode(false);
        platter.apply(JogInput::Touch(true), true);
        assert!(!platter.is_holding());
    }
}
