//! Offline tests of the effect units: where they apply, how they switch and how they keep time.

mod common;

use common::{
    DECLICK_FRAMES, SAMPLE_RATE, constant_track, largest_step, left_channel, render_frames,
    right_channel, sine_track,
};
use dubplate_engine::{
    BeatGrid, BeatLength, Controller, DeckId, EFFECT_UNIT_COUNT, EffectKind, EffectTarget,
    EffectUnitId, EngineError, MasterTempo, ParameterValue, Renderer, Track, offline_engine,
};

const BLOCK: usize = 480;
const FIRST_UNIT: EffectUnitId = EffectUnitId::ALL[0];

fn seconds_to_frames(seconds: f64) -> usize {
    (seconds * f64::from(SAMPLE_RATE)).round() as usize
}

/// A silent track with a single full-scale frame `impulse_seconds` in.
fn impulse_track(impulse_seconds: f64, seconds: f64) -> Track {
    let mut samples = vec![0.0_f32; seconds_to_frames(seconds) * 2];
    let impulse = seconds_to_frames(impulse_seconds) * 2;
    samples[impulse] = 1.0;
    samples[impulse + 1] = 1.0;
    Track::from_interleaved_stereo(&samples, SAMPLE_RATE).expect("valid test track")
}

type Step = fn(&mut Controller) -> Result<(), EngineError>;

/// Plays deck A on the left and deck B on the right, with the crossfader in the middle.
fn two_decks_on_their_own_sides() -> Result<(Controller, Renderer), EngineError> {
    let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
    controller.load_track(DeckId::A, sine_track(220.0, 0.5, 0.0, 10.0, SAMPLE_RATE))?;
    controller.load_track(DeckId::B, sine_track(330.0, 0.0, 0.5, 10.0, SAMPLE_RATE))?;
    for deck in DeckId::ALL {
        controller.set_beat_grid(deck, Some(BeatGrid::new(124.0, 0.0)?))?;
        controller.play(deck)?;
    }
    render_frames(&mut renderer, BLOCK, BLOCK);
    Ok((controller, renderer))
}

#[test]
fn an_effect_on_deck_a_changes_only_deck_a_and_only_while_on() {
    let (_reference_controller, mut reference) = two_decks_on_their_own_sides().unwrap();
    let (mut controller, mut renderer) = two_decks_on_their_own_sides().unwrap();
    controller
        .set_effect_kind(FIRST_UNIT, EffectKind::Crusher)
        .unwrap();
    controller
        .set_effect_target(FIRST_UNIT, EffectTarget::Deck(DeckId::A))
        .unwrap();
    controller.set_effect_depth(FIRST_UNIT, 1.0).unwrap();

    let mut render_both = |frame_count: usize| {
        (
            render_frames(&mut reference, frame_count, BLOCK),
            render_frames(&mut renderer, frame_count, BLOCK),
        )
    };

    let (expected, before) = render_both(BLOCK * 10);
    assert_eq!(before, expected, "the unit changed the sound while off");

    controller.set_effect_enabled(FIRST_UNIT, true).unwrap();
    let (expected, during) = render_both(BLOCK * 20);
    assert_eq!(right_channel(&during), right_channel(&expected));
    let changed = left_channel(&during)
        .iter()
        .zip(left_channel(&expected))
        .filter(|(actual, dry)| (*actual - dry).abs() > 1e-3)
        .count();
    assert!(changed > BLOCK * 10, "the crusher barely changed deck A");
    assert!(controller.effect_unit(FIRST_UNIT).sounding);

    controller.set_effect_enabled(FIRST_UNIT, false).unwrap();
    let (expected, fading) = render_both(BLOCK * 10);
    assert_eq!(right_channel(&fading), right_channel(&expected));
    let (expected, after) = render_both(BLOCK * 10);
    assert_eq!(after, expected, "the unit kept changing the sound once off");
    assert!(!controller.effect_unit(FIRST_UNIT).sounding);
}

#[test]
fn an_off_filter_unit_leaves_the_audio_bit_identical() {
    let (_reference_controller, mut reference) = two_decks_on_their_own_sides().unwrap();
    let (mut controller, mut renderer) = two_decks_on_their_own_sides().unwrap();
    controller
        .set_effect_kind(FIRST_UNIT, EffectKind::LowPass)
        .unwrap();
    controller
        .set_effect_target(FIRST_UNIT, EffectTarget::Deck(DeckId::A))
        .unwrap();
    controller.set_effect_depth(FIRST_UNIT, 1.0).unwrap();

    let (expected, actual) = (
        render_frames(&mut reference, BLOCK * 20, BLOCK),
        render_frames(&mut renderer, BLOCK * 20, BLOCK),
    );
    assert_eq!(expected, actual, "the off filter changed the audio");
    let unit = controller.effect_unit(FIRST_UNIT);
    assert!(!unit.enabled);
    assert!(!unit.sounding);

    controller.set_effect_enabled(FIRST_UNIT, true).unwrap();
    let during = render_frames(&mut renderer, BLOCK * 20, BLOCK);
    let changed = left_channel(&during)
        .iter()
        .filter(|sample| **sample != 0.0)
        .count();
    assert!(changed > 0, "deck A is silent");
    assert!(controller.effect_unit(FIRST_UNIT).sounding);
}

#[test]
fn an_off_echo_still_rings_out_its_trail() {
    let (_reference_controller, mut reference) = two_decks_on_their_own_sides().unwrap();
    let (mut controller, mut renderer) = two_decks_on_their_own_sides().unwrap();
    controller
        .set_effect_kind(FIRST_UNIT, EffectKind::Echo)
        .unwrap();
    controller
        .set_effect_target(FIRST_UNIT, EffectTarget::Deck(DeckId::A))
        .unwrap();
    controller.set_effect_depth(FIRST_UNIT, 1.0).unwrap();
    controller.set_effect_enabled(FIRST_UNIT, true).unwrap();
    render_frames(&mut reference, BLOCK * 200, BLOCK);
    render_frames(&mut renderer, BLOCK * 200, BLOCK);
    controller.set_effect_enabled(FIRST_UNIT, false).unwrap();

    // What the echo adds to deck A, against the same decks without it: still alive just after
    // the switch-off, and ringing down from there.
    let mut trail_peak = |frames: usize| {
        let dry = left_channel(&render_frames(&mut reference, frames, BLOCK));
        let wet = left_channel(&render_frames(&mut renderer, frames, BLOCK));
        wet.iter()
            .zip(&dry)
            .fold(0.0f32, |peak, (wet, dry)| peak.max((wet - dry).abs()))
    };
    trail_peak(BLOCK * 4);
    let first_peak = trail_peak(BLOCK * 8);
    assert!(
        first_peak > 0.01,
        "the echo stopped ringing the moment it was switched off"
    );
    let mut previous_peak = first_peak;
    for _ in 0..8 {
        let peak = trail_peak(BLOCK * 24);
        assert!(
            peak <= previous_peak + 1e-3,
            "the echo trail is growing: {previous_peak} -> {peak}"
        );
        previous_peak = peak;
    }
    assert!(previous_peak < first_peak, "the echo trail never rang down");
}

#[test]
fn moving_a_unit_or_changing_its_effect_never_clicks() {
    let amplitude = 0.5;
    let frequency = 60.0;
    let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
    for deck in DeckId::ALL {
        controller
            .load_track(
                deck,
                sine_track(frequency, amplitude, amplitude, 20.0, SAMPLE_RATE),
            )
            .unwrap();
        controller
            .set_beat_grid(deck, Some(BeatGrid::new(124.0, 0.0).unwrap()))
            .unwrap();
        controller.play(deck).unwrap();
    }
    controller.set_crossfader(0.0).unwrap();
    controller
        .set_effect_kind(FIRST_UNIT, EffectKind::LowPass)
        .unwrap();
    controller
        .set_effect_target(FIRST_UNIT, EffectTarget::Deck(DeckId::A))
        .unwrap();
    controller.set_effect_depth(FIRST_UNIT, 0.5).unwrap();
    controller.set_effect_enabled(FIRST_UNIT, true).unwrap();
    render_frames(&mut renderer, BLOCK * 20, BLOCK);

    let changes: [Step; 6] = [
        |controller| controller.set_effect_kind(FIRST_UNIT, EffectKind::HighPass),
        |controller| controller.set_effect_target(FIRST_UNIT, EffectTarget::Master),
        |controller| controller.set_effect_kind(FIRST_UNIT, EffectKind::Filter),
        |controller| controller.set_effect_target(FIRST_UNIT, EffectTarget::Deck(DeckId::A)),
        |controller| controller.set_effect_kind(FIRST_UNIT, EffectKind::Echo),
        |controller| controller.set_effect_target(FIRST_UNIT, EffectTarget::Deck(DeckId::B)),
    ];
    let mut output = left_channel(&render_frames(&mut renderer, BLOCK * 4, BLOCK));
    for change in changes {
        change(&mut controller).unwrap();
        output.extend(left_channel(&render_frames(
            &mut renderer,
            BLOCK * 4,
            BLOCK,
        )));
    }

    let sine_step = amplitude * (std::f64::consts::TAU * frequency / f64::from(SAMPLE_RATE)) as f32;
    let click_threshold = amplitude / DECLICK_FRAMES as f32;
    let step = largest_step(&output);
    assert!(
        step <= sine_step + click_threshold,
        "largest step {step} is above the sine's own {sine_step} plus {click_threshold}"
    );
}

/// Loads a deck with a click half a second in, on a grid of `bpm`.
fn load_click(controller: &mut Controller, deck: DeckId, bpm: f64) -> Result<(), EngineError> {
    controller.load_track(deck, impulse_track(0.5, 4.0))?;
    controller.set_beat_grid(deck, Some(BeatGrid::new(bpm, 0.0)?))
}

/// Switches on a half-beat echo on `target`, plays every loaded deck and returns the seconds
/// from the first click in the output to its first echo.
fn first_echo_delay(
    controller: &mut Controller,
    renderer: &mut Renderer,
    target: EffectTarget,
) -> Result<f64, EngineError> {
    controller.set_effect_kind(FIRST_UNIT, EffectKind::Echo)?;
    controller.set_effect_target(FIRST_UNIT, target)?;
    controller.set_effect_parameter(FIRST_UNIT, ParameterValue::Beats(BeatLength::HALF))?;
    controller.set_effect_depth(FIRST_UNIT, 1.0)?;
    controller.set_effect_enabled(FIRST_UNIT, true)?;
    for deck in DeckId::ALL {
        if controller.track(deck).is_some() {
            controller.play(deck)?;
        }
    }
    let output = left_channel(&render_frames(renderer, seconds_to_frames(1.5), BLOCK));
    let click = output
        .iter()
        .position(|sample| sample.abs() > 0.5)
        .expect("the click plays");
    let gap = seconds_to_frames(0.01);
    let echo = output[click + gap..]
        .iter()
        .position(|sample| sample.abs() > 0.05)
        .expect("the click echoes");
    Ok((echo + gap) as f64 / f64::from(SAMPLE_RATE))
}

fn assert_half_beat(delay: f64, bpm: f64) {
    let expected = 0.5 * 60.0 / bpm;
    assert!(
        (delay - expected).abs() < 0.001,
        "echo after {delay} s, expected half a beat at {bpm} BPM: {expected} s"
    );
}

#[test]
fn echoes_follow_the_bpm_and_tempo_of_the_deck_they_sit_on() {
    for (bpm, tempo) in [(120.0, 1.0), (128.0, 1.0), (120.0, 1.25)] {
        let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
        controller.set_crossfader(0.0).unwrap();
        load_click(&mut controller, DeckId::A, bpm).unwrap();
        controller.set_tempo(DeckId::A, tempo).unwrap();
        let delay = first_echo_delay(
            &mut controller,
            &mut renderer,
            EffectTarget::Deck(DeckId::A),
        )
        .unwrap();
        assert_half_beat(delay, bpm * tempo);
    }
}

#[test]
fn master_echoes_follow_the_louder_deck_or_the_chosen_tempo_master() {
    for (source, expected_master, expected_bpm) in [
        (MasterTempo::LouderDeck, DeckId::A, 120.0),
        (MasterTempo::Deck(DeckId::B), DeckId::B, 150.0),
    ] {
        let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
        load_click(&mut controller, DeckId::A, 120.0).unwrap();
        controller
            .load_track(DeckId::B, constant_track(0.0, 0.0, 4.0, SAMPLE_RATE))
            .unwrap();
        controller
            .set_beat_grid(DeckId::B, Some(BeatGrid::new(150.0, 0.0).unwrap()))
            .unwrap();
        controller.set_crossfader(0.0).unwrap();
        controller.set_channel_fader(DeckId::B, 0.3).unwrap();
        controller.set_master_tempo(source).unwrap();
        let delay = first_echo_delay(&mut controller, &mut renderer, EffectTarget::Master).unwrap();
        assert_eq!(controller.tempo_master_deck(), Some(expected_master));
        assert_half_beat(delay, expected_bpm);
    }
}

#[test]
fn the_controller_reports_unit_settings_and_rejects_bad_values() {
    let (mut controller, mut renderer) = offline_engine(SAMPLE_RATE);
    assert_eq!(EffectUnitId::ALL.len(), EFFECT_UNIT_COUNT);
    assert_eq!(EffectUnitId::new(2).unwrap(), EffectUnitId::ALL[2]);
    assert!(matches!(
        EffectUnitId::new(EFFECT_UNIT_COUNT),
        Err(EngineError::InvalidEffectUnit {
            unit: 3,
            unit_count: 3
        })
    ));
    assert!(matches!(
        controller.set_effect_depth(FIRST_UNIT, 1.5),
        Err(EngineError::InvalidParameter {
            name: "effect depth",
            ..
        })
    ));
    assert!(controller.set_effect_depth(FIRST_UNIT, f32::NAN).is_err());
    assert!(BeatGrid::new(5.0, 0.0).is_err());

    let unit = EffectUnitId::ALL[1];
    controller.set_effect_kind(unit, EffectKind::Gate).unwrap();
    controller
        .set_effect_target(unit, EffectTarget::Deck(DeckId::B))
        .unwrap();
    controller
        .set_effect_parameter(unit, ParameterValue::Beats(BeatLength::QUARTER))
        .unwrap();
    controller
        .set_effect_parameter(unit, ParameterValue::Percent(40))
        .unwrap();
    controller.set_effect_depth(unit, 0.75).unwrap();
    controller.toggle_effect(unit).unwrap();
    render_frames(&mut renderer, BLOCK, BLOCK);
    let state = controller.effect_unit(unit);
    assert_eq!(state.kind, EffectKind::Gate);
    assert_eq!(state.target, EffectTarget::Deck(DeckId::B));
    assert_eq!(
        state.parameter,
        Some(ParameterValue::Beats(BeatLength::QUARTER)),
        "a gate keeps its beat and ignores a percentage"
    );
    assert_eq!(state.depth, 0.75);
    assert!(state.enabled);
    assert!(state.sounding);
    assert!(!controller.effect_unit(FIRST_UNIT).enabled);
    assert_eq!(unit.to_string(), "FX 2");
}

#[test]
fn loading_a_track_clears_the_deck_beat_grid() {
    let (mut controller, _renderer) = offline_engine(SAMPLE_RATE);
    let grid = BeatGrid::new(124.0, 0.12).unwrap();
    controller
        .set_beat_grid(DeckId::A, Some(grid.clone()))
        .unwrap();
    assert_eq!(controller.beat_grid(DeckId::A), Some(grid));
    controller
        .load_track(DeckId::A, constant_track(0.1, 0.1, 1.0, SAMPLE_RATE))
        .unwrap();
    assert_eq!(controller.beat_grid(DeckId::A), None);
}
