//! Random sequences of loop actions while a deck plays, with the invariants a DJ relies on
//! checked after every step. The sequences are seeded, so a failure repeats.

mod common;

use common::{SAMPLE_RATE, sine_track};
use dubplate_engine::{
    BeatGrid, BeatLength, Controller, DeckId, MasterTempo, Renderer, SyncMode, offline_engine,
};

const BLOCK: usize = 256;
const STEPS: usize = 60;
const DEFAULT_SEEDS: u64 = 10;
const START_SECONDS: f64 = 60.0;
const DECK: DeckId = DeckId::A;

struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }

    fn pick<T: Copy>(&mut self, choices: &[T]) -> T {
        choices[self.below(choices.len() as u64) as usize]
    }
}

#[derive(Debug, Clone, Copy)]
enum Action {
    LoopIn,
    LoopOut,
    FourBeatOrExit,
    BeatLoopPad(BeatLength),
    Halve,
    Double,
    ReloopOrExit,
    Roll(BeatLength),
    SlipToggle,
    BeatJump(f64),
    HotCueInsideLoop,
    Nudge(i32),
    Scratch(i32),
    QuantizeToggle,
    KeyLockToggle,
    TempoChange,
    SyncChange,
}

const PAD_LENGTHS: [BeatLength; 8] = [
    BeatLength::QUARTER,
    BeatLength::HALF,
    BeatLength::ONE,
    BeatLength::TWO,
    BeatLength::FOUR,
    BeatLength::EIGHT,
    BeatLength::SIXTEEN,
    BeatLength::THIRTY_TWO,
];

fn random_action(random: &mut Random) -> Action {
    match random.below(22) {
        0 => Action::LoopIn,
        1 | 2 => Action::LoopOut,
        3 | 4 => Action::FourBeatOrExit,
        5 | 6 => Action::BeatLoopPad(random.pick(&PAD_LENGTHS)),
        7 | 8 => Action::Halve,
        9 | 10 => Action::Double,
        11 => Action::ReloopOrExit,
        12 => Action::Roll(random.pick(&PAD_LENGTHS)),
        13 => Action::SlipToggle,
        14 => Action::BeatJump(random.pick(&[-4.0, -2.0, -1.0, 1.0, 2.0, 4.0])),
        15 => Action::HotCueInsideLoop,
        16 => Action::Nudge(random.pick(&[-30, -8, 8, 30])),
        17 => Action::Scratch(random.pick(&[-40, -12, 12, 40])),
        18 => Action::QuantizeToggle,
        19 => Action::KeyLockToggle,
        20 => Action::TempoChange,
        _ => Action::SyncChange,
    }
}

#[derive(Default)]
struct Model {
    has_loop: bool,
    active: bool,
    entered: bool,
    start: Option<f64>,
    slip: bool,
    quantize: bool,
    key_lock: bool,
    shadow: Option<f64>,
    sync: Option<SyncMode>,
}

struct Rig {
    controller: Controller,
    renderer: Renderer,
    random: Random,
    model: Model,
    log: Vec<String>,
}

impl Rig {
    fn new(seed: u64) -> Self {
        let (mut controller, renderer) = offline_engine(SAMPLE_RATE);
        for (deck, bpm) in [(DECK, 120.0), (DeckId::B, 126.0)] {
            controller
                .load_track(deck, sine_track(440.0, 0.5, 0.5, 150.0, SAMPLE_RATE))
                .expect("load the test track");
            controller
                .set_beat_grid(deck, Some(BeatGrid::new(bpm, 0.0).expect("grid")))
                .expect("set the grid");
        }
        controller.set_crossfader(0.0).expect("set the crossfader");
        controller.seek(DECK, START_SECONDS).expect("seek");
        controller.play(DECK).expect("play");
        controller.seek(DeckId::B, 20.0).expect("seek");
        controller.play(DeckId::B).expect("play");
        controller
            .set_master_tempo(MasterTempo::Deck(DeckId::B))
            .expect("choose the master");
        Self {
            controller,
            renderer,
            random: Random(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1),
            model: Model {
                quantize: true,
                ..Model::default()
            },
            log: Vec::new(),
        }
    }

    fn render(&mut self, frames: usize) {
        let mut rendered = 0;
        while rendered < frames {
            let block = BLOCK.min(frames - rendered);
            let mut output = vec![0.0_f32; block * 2];
            self.renderer.render(&mut output, 2);
            rendered += block;
        }
    }

    fn position(&self) -> f64 {
        self.controller.deck_state(DECK).position_seconds
    }

    fn fail(&self, message: &str) -> ! {
        let recent: Vec<&String> = self.log.iter().rev().take(400).collect();
        panic!("{message}\nrecent steps, newest first: {recent:#?}");
    }

    fn apply(&mut self, action: Action) {
        let controller = &mut self.controller;
        let before = controller.loop_state(DECK);
        match action {
            Action::LoopIn => controller.loop_in(DECK).expect("send the command"),
            Action::LoopOut => {
                controller.loop_out(DECK).expect("send the command");
                if before.pending_start_seconds.is_some() || self.model.has_loop {
                    self.model.has_loop = true;
                    self.model.active = true;
                }
            }
            Action::FourBeatOrExit => {
                if self.model.active {
                    controller.reloop_or_exit(DECK).expect("send the command");
                    self.model.active = false;
                } else {
                    controller
                        .beat_loop(DECK, BeatLength::FOUR)
                        .expect("send the command");
                    self.model.has_loop = true;
                    self.model.active = true;
                }
            }
            Action::BeatLoopPad(length) => {
                let same = before.active && before.length == Some(length);
                if same {
                    controller.reloop_or_exit(DECK).expect("send the command");
                    self.model.active = false;
                } else {
                    controller
                        .beat_loop(DECK, length)
                        .expect("send the command");
                    self.model.has_loop = true;
                    self.model.active = true;
                }
            }
            Action::Halve => {
                if self.model.active {
                    controller.halve_loop(DECK).expect("send the command");
                }
            }
            Action::Double => {
                if self.model.active {
                    controller.double_loop(DECK).expect("send the command");
                }
            }
            Action::ReloopOrExit => {
                if self.model.has_loop {
                    controller.reloop_or_exit(DECK).expect("send the command");
                    self.model.active = !self.model.active;
                }
            }
            Action::Roll(length) => {
                controller
                    .beat_roll(DECK, length)
                    .expect("send the command");
            }
            Action::SlipToggle => {
                self.model.slip = !self.model.slip;
                controller
                    .set_slip(DECK, self.model.slip)
                    .expect("send the command");
            }
            Action::BeatJump(beats) => controller.beat_jump(DECK, beats).expect("send the command"),
            Action::HotCueInsideLoop => {
                let seconds = match (before.start_seconds, before.end_seconds) {
                    (Some(start), Some(end)) if before.active => {
                        start + (end - start) * (self.random.below(1000) as f64 / 1000.0)
                    }
                    _ => START_SECONDS + 3.0,
                };
                controller.hot_cue(DECK, seconds).expect("send the command");
            }
            Action::Nudge(ticks) => controller.jog_nudge(DECK, ticks).expect("send the command"),
            Action::Scratch(ticks) => {
                controller
                    .set_jog_touch(DECK, true)
                    .expect("send the command");
                controller
                    .jog_scratch(DECK, ticks)
                    .expect("send the command");
            }
            Action::QuantizeToggle => {
                self.model.quantize = !self.model.quantize;
                controller
                    .set_quantize(self.model.quantize)
                    .expect("send the command");
            }
            Action::KeyLockToggle => {
                self.model.key_lock = !self.model.key_lock;
                controller
                    .set_key_lock(DECK, self.model.key_lock)
                    .expect("send the command");
            }
            Action::TempoChange => {
                let tempo = 0.92 + self.random.below(17) as f64 / 100.0;
                controller.set_tempo(DECK, tempo).expect("send the command");
            }
            Action::SyncChange => {
                let mode = self
                    .random
                    .pick(&[SyncMode::Off, SyncMode::Tempo, SyncMode::Beat]);
                controller.set_sync(DECK, mode).expect("send the command");
                self.model.sync = Some(mode);
            }
        }
    }

    fn settle(&mut self, action: Action) {
        let frames = 1 + self.random.below(4000) as usize;
        match action {
            Action::Roll(_) => {
                self.render(frames);
                let state = self.controller.loop_state(DECK);
                self.log.push(format!(
                    "   rolling: {:?}..{:?} playhead {} shadow {:?}",
                    state.start_seconds,
                    state.end_seconds,
                    self.position(),
                    self.controller.slip_state(DECK).shadow_seconds
                ));
                self.controller
                    .release_roll(DECK)
                    .expect("send the command");
                self.render(1);
                self.log
                    .push(format!("   released: playhead {}", self.position()));
            }
            Action::Scratch(_) => {
                self.render(frames);
                self.controller
                    .set_jog_touch(DECK, false)
                    .expect("send the command");
            }
            _ => {}
        }
        self.render(frames);
    }

    fn check(&mut self, action: Action) {
        let state = self.controller.loop_state(DECK);
        let moves_loop = matches!(
            action,
            Action::LoopOut
                | Action::FourBeatOrExit
                | Action::BeatLoopPad(_)
                | Action::BeatJump(_)
                | Action::Roll(_)
                | Action::ReloopOrExit
        );
        if state.active != self.model.active {
            self.fail(&format!(
                "loop active is {} but the DJ's last presses leave it {}",
                state.active, self.model.active
            ));
        }
        if self.model.has_loop != state.start_seconds.is_some() {
            self.fail("the loop's presence does not match");
        }
        if !self.model.active {
            self.model.start = None;
            self.model.entered = false;
            return;
        }
        let start = state.start_seconds.expect("an active loop has a start");
        let end = state.end_seconds.expect("an active loop has an end");
        if matches!(action, Action::Halve | Action::Double)
            && self
                .model
                .start
                .is_some_and(|before| (before - start).abs() > 1e-9)
        {
            self.fail("halving or doubling moved the loop in point");
        }
        if self
            .model
            .start
            .is_none_or(|before| (before - start).abs() > 1e-9)
            || moves_loop
        {
            self.model.entered = false;
        }
        self.model.start = Some(start);
        let position = self.position();
        if position >= end {
            self.fail(&format!(
                "the playhead {position} is past the loop out {end} of an active loop"
            ));
        }
        if position >= start {
            self.model.entered = true;
        } else if self.model.entered {
            self.fail(&format!(
                "the playhead {position} fell behind the loop in {start} of an active loop"
            ));
        }
    }

    fn check_slip(&mut self, action: Action) {
        let shadow = self.controller.slip_state(DECK).shadow_seconds;
        let may_begin = matches!(
            action,
            Action::SlipToggle
                | Action::LoopOut
                | Action::FourBeatOrExit
                | Action::BeatLoopPad(_)
                | Action::ReloopOrExit
                | Action::Roll(_)
                | Action::Scratch(_)
                | Action::HotCueInsideLoop
                | Action::BeatJump(_)
                | Action::Halve
                | Action::Double
        );
        if let (Some(before), Some(now)) = (self.model.shadow, shadow)
            && !may_begin
            && now < before
        {
            self.fail(&format!("the slip shadow went back from {before} to {now}"));
        }
        self.model.shadow = shadow;
    }

    fn run(&mut self) {
        for step in 0..STEPS {
            let action = random_action(&mut self.random);
            self.log.push(format!("{step}: {action:?}"));
            self.apply(action);
            self.render(1);
            self.log
                .push(format!("   after one frame: playhead {}", self.position()));
            self.settle(action);
            let state = self.controller.loop_state(DECK);
            self.log.push(format!(
                "   -> loop {:?}..{:?} active {} length {:?} pending {:?} playhead {} tempo {} slip {} quantize {} key lock {} sync {:?} shadow {:?}",
                state.start_seconds,
                state.end_seconds,
                state.active,
                state.length.map(BeatLength::beats),
                state.pending_start_seconds,
                self.position(),
                self.controller.deck_state(DECK).tempo,
                self.model.slip,
                self.model.quantize,
                self.model.key_lock,
                self.model.sync,
                self.controller.slip_state(DECK).shadow_seconds,
            ));
            self.check(action);
            self.check_slip(action);
        }
    }
}

#[test]
fn random_loop_actions_keep_the_loop_going_and_the_playhead_inside() {
    let seeds = std::env::var("LOOP_PROPERTY_SEEDS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_SEEDS);
    for seed in 1..=seeds {
        Rig::new(seed).run();
    }
}
