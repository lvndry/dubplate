//! Release FX through every native unit and assignment, including short gestures.

mod common;

use common::{SAMPLE_RATE, left_channel, render_frames, right_channel, sine_track};
use dubplate_engine::{
    BeatGrid, Controller, DeckId, EffectKind, EffectTarget, EffectUnitId, ReleaseFx, Renderer,
    offline_engine,
};

const BLOCK: usize = 256;

fn running_pair() -> (Controller, Renderer) {
    let (mut controller, renderer) = offline_engine(SAMPLE_RATE);
    for (deck, track) in [
        (DeckId::A, sine_track(220.0, 0.3, 0.0, 10.0, SAMPLE_RATE)),
        (DeckId::B, sine_track(330.0, 0.0, 0.3, 10.0, SAMPLE_RATE)),
    ] {
        controller.load_track(deck, track).unwrap();
        controller
            .set_beat_grid(deck, Some(BeatGrid::new(120.0, 0.0).unwrap()))
            .unwrap();
        controller.play(deck).unwrap();
    }
    controller.set_crossfader_enabled(false).unwrap();
    (controller, renderer)
}

fn seconds(renderer: &mut Renderer, duration: f64) -> [Vec<f32>; 2] {
    let frames = render_frames(
        renderer,
        (duration * f64::from(SAMPLE_RATE)).round() as usize,
        BLOCK,
    );
    [left_channel(&frames), right_channel(&frames)]
}

fn rms(samples: &[f32]) -> f32 {
    let power: f32 = samples.iter().map(|&sample| sample * sample).sum();
    (power / samples.len() as f32).sqrt()
}

fn covers(target: EffectTarget, deck: DeckId) -> bool {
    match target {
        EffectTarget::Deck(assigned) => assigned == deck,
        EffectTarget::BothDecks | EffectTarget::Master => true,
    }
}

#[test]
fn each_release_switches_off_beat_fx_and_returns_the_live_signal_on_every_unit_and_target() {
    let (mut controller, mut renderer) = running_pair();
    let dry_rms = 0.3 * std::f32::consts::FRAC_1_SQRT_2;
    for unit in EffectUnitId::ALL {
        for (index, target) in EffectTarget::ALL.into_iter().enumerate() {
            // Exercise each pair of release and target across all three otherwise-identical units.
            let release = ReleaseFx::ALL[(index + unit.index()) % ReleaseFx::ALL.len()];
            let label = format!("{release:?} on {unit} / {target:?}");
            for deck in DeckId::ALL {
                controller.seek(deck, 0.0).unwrap();
            }
            controller.set_effect_kind(unit, EffectKind::Trans).unwrap();
            controller.set_effect_target(unit, target).unwrap();
            controller.set_effect_depth(unit, 0.0).unwrap();
            controller.set_effect_enabled(unit, true).unwrap();
            controller.set_release_fx(unit, release).unwrap();
            seconds(&mut renderer, 1.5);

            controller.hold_release_fx(unit, true).unwrap();
            let status = controller.effect_unit(unit);
            assert_eq!(status.release, release, "{label}");
            assert!(status.releasing, "{label}");
            assert!(!status.enabled, "{label}");
            seconds(&mut renderer, 2.5);
            let held = seconds(&mut renderer, 0.25);
            for deck in DeckId::ALL {
                let level = rms(&held[deck.index()]);
                if covers(target, deck) {
                    assert!(level < 0.06, "{label} left {deck:?} at {level}");
                } else {
                    assert!(
                        (level - dry_rms).abs() < 0.01,
                        "{label} spilled onto {deck:?}: {level}"
                    );
                }
            }

            controller.hold_release_fx(unit, false).unwrap();
            seconds(&mut renderer, 5.0);
            let returned = seconds(&mut renderer, 0.25);
            let status = controller.effect_unit(unit);
            assert!(!status.releasing && !status.enabled, "{label}");
            for deck in DeckId::ALL {
                let index = deck.index();
                let level = rms(&returned[index]);
                assert!(
                    (level - dry_rms).abs() < 0.01,
                    "{label} did not return {deck:?}: {level}"
                );
                assert!(
                    returned[index].iter().all(|sample| sample.is_finite()),
                    "{label}"
                );
            }
        }
    }
}

#[test]
fn a_brief_release_fx_press_does_not_pause_or_rewind_either_deck() {
    for release in ReleaseFx::ALL {
        let (mut controller, mut renderer) = running_pair();
        let unit = EffectUnitId::ALL[0];
        controller
            .set_effect_target(unit, EffectTarget::BothDecks)
            .unwrap();
        controller.set_release_fx(unit, release).unwrap();
        seconds(&mut renderer, 1.0);
        let start = DeckId::ALL.map(|deck| controller.deck_state(deck).position_seconds);

        controller.hold_release_fx(unit, true).unwrap();
        assert!(controller.effect_unit(unit).releasing);
        seconds(&mut renderer, 0.06);
        controller.hold_release_fx(unit, false).unwrap();
        let returned = seconds(&mut renderer, 0.25);
        for deck in DeckId::ALL {
            let index = deck.index();
            let deck_state = controller.deck_state(deck);
            assert!(deck_state.playing, "{release:?} stopped {deck:?}");
            let advance = deck_state.position_seconds - start[index];
            assert!(
                (advance - 0.31).abs() < 0.01,
                "{release:?} jumped {deck:?} by {advance}"
            );
            assert!(
                rms(&returned[index][2 * BLOCK..]) > 0.18,
                "{release:?} failed to return {deck:?}"
            );
        }
    }
}
