//! The echo out: an echo or a reverb on a deck rings on after the deck's channel fader closes,
//! because the effect units sit after the channel fader and the crossfader, so they keep the
//! last of the music the fader let through.

mod common;

use common::{
    SAMPLE_RATE, club_loop_track, largest_step, left_channel, render_frames, right_channel,
};
use dubplate_engine::{
    BeatGrid, Controller, DeckId, EffectKind, EffectTarget, EffectUnitId, Renderer, offline_engine,
};

const BLOCK: usize = 256;
const TEMPO_BPM: f64 = 124.0;
const UNIT: EffectUnitId = EffectUnitId::ALL[0];

fn beats_to_frames(beats: f64) -> usize {
    (beats * 60.0 / TEMPO_BPM * f64::from(SAMPLE_RATE)) as usize
}

fn decibels(level: f32) -> f32 {
    20.0 * level.max(1e-9).log10()
}

fn peak(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0, |peak, sample| peak.max(sample.abs()))
}

/// Both decks hold a club loop on a 124 BPM grid; `deck` plays alone, through effect unit 1
/// set to `kind` at half depth, with the crossfader at `deck`'s end, and Smart Fader and the
/// fader echo as given.
fn echo_on(
    deck: DeckId,
    kind: EffectKind,
    smart_fader: bool,
    fader_echo: bool,
) -> (Controller, Renderer) {
    let (mut controller, renderer) = offline_engine(SAMPLE_RATE);
    for each in DeckId::ALL {
        controller
            .load_track(each, club_loop_track(TEMPO_BPM, 30.0, SAMPLE_RATE))
            .expect("load");
        controller
            .set_beat_grid(each, Some(BeatGrid::new(TEMPO_BPM, 0.0).expect("grid")))
            .expect("grid");
    }
    let other = if deck == DeckId::A {
        DeckId::B
    } else {
        DeckId::A
    };
    controller.set_channel_fader(other, 0.0).expect("fader");
    controller
        .set_crossfader(if deck == DeckId::A { 0.0 } else { 1.0 })
        .expect("crossfader");
    controller
        .set_smart_fader(smart_fader)
        .expect("smart fader");
    controller.set_fader_echo(fader_echo).expect("fader echo");
    controller.set_effect_kind(UNIT, kind).expect("kind");
    controller
        .set_effect_target(UNIT, EffectTarget::Deck(deck))
        .expect("target");
    controller.set_effect_depth(UNIT, 0.5).expect("depth");
    controller.set_effect_enabled(UNIT, true).expect("on");
    controller.play(deck).expect("play");
    (controller, renderer)
}

#[test]
fn an_echo_or_reverb_rings_on_after_its_decks_channel_fader_closes() {
    let mut failures = Vec::new();
    for kind in [
        EffectKind::Echo,
        EffectKind::Delay,
        EffectKind::Spiral,
        EffectKind::Reverb,
        EffectKind::PingPong,
    ] {
        for deck in DeckId::ALL {
            for (smart_fader, fader_echo) in [(false, false), (true, false), (false, true)] {
                let (mut controller, mut renderer) = echo_on(deck, kind, smart_fader, fader_echo);
                render_frames(&mut renderer, beats_to_frames(4.0), BLOCK);
                controller.set_channel_fader(deck, 0.0).expect("fader");
                let tail = render_frames(&mut renderer, beats_to_frames(2.5), BLOCK);
                let channel = |samples: &[f32]| {
                    let left = left_channel(samples);
                    let right = right_channel(samples);
                    left.iter()
                        .zip(&right)
                        .map(|(l, r)| l.abs().max(r.abs()))
                        .collect::<Vec<f32>>()
                };
                let loudest = channel(&tail);
                // Delay repeats once, half a beat on; the others keep repeating.
                let (from, to) = if kind == EffectKind::Delay {
                    (0.25, 0.75)
                } else {
                    (1.5, 2.0)
                };
                let late = &loudest[beats_to_frames(from)..beats_to_frames(to)];
                let late_peak = decibels(peak(late));
                if late_peak < -40.0 {
                    failures.push(format!(
                        "{kind} on deck {deck:?}, smart fader {smart_fader}, fader echo {fader_echo}: {late_peak:.1} dBFS {from} to {to} beats after the fader closed"
                    ));
                }
                let step = largest_step(&left_channel(&tail));
                if step > 0.5 {
                    failures.push(format!(
                        "{kind} on deck {deck:?}, smart fader {smart_fader}, fader echo {fader_echo}: the tail jumps by {step:.2}"
                    ));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "the tail died:\n{}",
        failures.join("\n")
    );
}
