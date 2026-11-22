//! A deck playing a track's stems: at full level they sound like the track, a muted stem fades
//! out without a click, and the stems stay on the playhead through tempo, loops, jumps and key
//! lock.

mod common;

use common::{
    SAMPLE_RATE, STEM_AMPLITUDE, STEM_TONES, largest_step, left_channel, mix_of_stems,
    render_frames, stems_like,
};
use dubplate_engine::{
    BeatGrid, BeatLength, Controller, DeckId, EngineError, Renderer, Stem, StemLevel, StemSettings,
    Track, offline_engine,
};

const BLOCK: usize = 256;
const SECONDS: f64 = 6.0;
/// 20 ms, the stems' fade.
const FADE_FRAMES: usize = 960;
/// The master limiter's look-ahead, 1.5 ms.
const LIMITER_FRAMES: usize = 72;

fn playing_deck(track: Track) -> (Controller, Renderer) {
    let (mut controller, renderer) = offline_engine(SAMPLE_RATE);
    controller.load_track(DeckId::A, track).expect("load");
    controller.set_crossfader(0.0).expect("crossfader");
    controller
        .set_beat_grid(DeckId::A, Some(BeatGrid::new(120.0, 0.0).expect("grid")))
        .expect("grid");
    controller.play(DeckId::A).expect("play");
    (controller, renderer)
}

fn render(renderer: &mut Renderer, frames: usize) -> Vec<f32> {
    left_channel(&render_frames(renderer, frames, BLOCK))
}

fn rms(samples: &[f32]) -> f64 {
    (samples
        .iter()
        .map(|sample| f64::from(*sample).powi(2))
        .sum::<f64>()
        / samples.len().max(1) as f64)
        .sqrt()
}

fn decibels(ratio: f64) -> f64 {
    20.0 * ratio.max(1e-12).log10()
}

/// How loud the difference of two renders is against the first, in dB.
fn residual_decibels(reference: &[f32], other: &[f32]) -> f64 {
    let difference: Vec<f32> = reference
        .iter()
        .zip(other)
        .map(|(first, second)| first - second)
        .collect();
    decibels(rms(&difference) / rms(reference))
}

/// The level of the `frequency` tone in `signal`, as the amplitude of a sine.
fn tone_level(signal: &[f32], frequency: f64) -> f64 {
    let (sine, cosine) =
        signal
            .iter()
            .enumerate()
            .fold((0.0, 0.0), |(sine, cosine), (index, sample)| {
                let phase =
                    std::f64::consts::TAU * frequency * index as f64 / f64::from(SAMPLE_RATE);
                (
                    sine + f64::from(*sample) * phase.sin(),
                    cosine + f64::from(*sample) * phase.cos(),
                )
            });
    2.0 * (sine * sine + cosine * cosine).sqrt() / signal.len() as f64
}

/// Renders the mix played as a track and the same mix played from its stems, `frames` long,
/// with `prepare` run on both decks first.
fn track_and_stems(
    stem_rate: u32,
    frames: usize,
    prepare: impl Fn(&mut Controller),
) -> (Vec<f32>, Vec<f32>) {
    let render_with = |stems: bool| {
        let track = mix_of_stems(SECONDS, SAMPLE_RATE);
        if stems {
            track
                .attach_stems(stems_like(SECONDS, stem_rate))
                .expect("attach");
        }
        let (mut controller, mut renderer) = playing_deck(track);
        prepare(&mut controller);
        render(&mut renderer, frames)
    };
    (render_with(false), render_with(true))
}

#[test]
fn stems_at_full_level_sound_like_the_track() {
    let (track, stems) = track_and_stems(SAMPLE_RATE, SAMPLE_RATE as usize * 2, |_| {});
    let residual = residual_decibels(&track, &stems);
    assert!(
        residual < -120.0,
        "stems at the track's rate: {residual:.1} dB"
    );

    let (track, stems) = track_and_stems(SAMPLE_RATE, SAMPLE_RATE as usize * 2, |controller| {
        controller
            .set_stems(
                DeckId::A,
                StemSettings::UNTOUCHED.with_level(Stem::Melody, StemLevel::new(0.999)),
            )
            .expect("touch the stems");
    });
    let melody_at_level = 0.999_f64;
    let residual = residual_decibels(&track, &stems);
    let expected = 20.0 * (1.0 - melody_at_level).log10();
    assert!(
        residual < expected + 3.0,
        "every stem read and mixed: {residual:.1} dB, the melody's step alone is {expected:.1} dB"
    );
}

#[test]
fn a_muted_stem_is_silent_after_its_fade_and_never_clicks() {
    let track = mix_of_stems(SECONDS, SAMPLE_RATE);
    track
        .attach_stems(stems_like(SECONDS, SAMPLE_RATE))
        .expect("attach");
    let (mut controller, mut renderer) = playing_deck(track);
    let before = render(&mut renderer, SAMPLE_RATE as usize);
    let steady_step = largest_step(&before);
    controller
        .set_stems(
            DeckId::A,
            StemSettings::UNTOUCHED.with_muted(Stem::Vocals, true),
        )
        .expect("mute");
    let fade = render(&mut renderer, FADE_FRAMES * 4);
    let after = render(&mut renderer, SAMPLE_RATE as usize);

    let vocals = STEM_TONES[Stem::Vocals.index()];
    assert!((tone_level(&before, vocals) - STEM_AMPLITUDE).abs() < 0.01);
    assert!(
        tone_level(&after, vocals) < 1e-4,
        "vocals still at {}",
        tone_level(&after, vocals)
    );
    for stem in [Stem::Drums, Stem::Bass, Stem::Melody] {
        let level = tone_level(&after, STEM_TONES[stem.index()]);
        assert!((level - STEM_AMPLITUDE).abs() < 0.01, "{stem:?} at {level}");
    }
    assert!(
        largest_step(&fade) <= steady_step * 1.05,
        "the fade stepped by {} where the music steps by {steady_step}",
        largest_step(&fade)
    );
}

#[test]
fn instrumental_acapella_and_levels_reach_their_gains() {
    let track = mix_of_stems(SECONDS, SAMPLE_RATE);
    track
        .attach_stems(stems_like(SECONDS, SAMPLE_RATE))
        .expect("attach");
    let (mut controller, mut renderer) = playing_deck(track);
    let settings = StemSettings::UNTOUCHED
        .with_level(Stem::Bass, StemLevel::new(0.5))
        .solo(Stem::Vocals);
    controller.set_stems(DeckId::A, settings).expect("acapella");
    render(&mut renderer, FADE_FRAMES * 4);
    let acapella = render(&mut renderer, SAMPLE_RATE as usize);
    controller
        .set_stems(DeckId::A, settings.without(Stem::Vocals))
        .expect("instrumental");
    render(&mut renderer, FADE_FRAMES * 4);
    let instrumental = render(&mut renderer, SAMPLE_RATE as usize);

    let level = |signal: &[f32], stem: Stem| tone_level(signal, STEM_TONES[stem.index()]);
    assert!((level(&acapella, Stem::Vocals) - STEM_AMPLITUDE).abs() < 0.01);
    assert!(level(&acapella, Stem::Drums) < 1e-4);
    assert!(level(&instrumental, Stem::Vocals) < 1e-4);
    assert!((level(&instrumental, Stem::Bass) - STEM_AMPLITUDE / 2.0).abs() < 0.01);
    assert!((level(&instrumental, Stem::Melody) - STEM_AMPLITUDE).abs() < 0.01);
}

#[test]
fn stems_stay_on_the_playhead_through_tempo_loops_and_jumps() {
    let frames = SAMPLE_RATE as usize * 4;
    let (track, stems) = track_and_stems(SAMPLE_RATE, frames, |controller| {
        controller.set_tempo(DeckId::A, 1.07).expect("tempo");
        controller
            .beat_loop(DeckId::A, BeatLength::ONE)
            .expect("loop");
    });
    let residual = residual_decibels(&track, &stems);
    assert!(residual < -100.0, "looping at +7%: {residual:.1} dB");

    let (track, stems) = track_and_stems(SAMPLE_RATE, frames, |controller| {
        controller.beat_jump(DeckId::A, 3.0).expect("jump");
        controller.set_slip(DeckId::A, true).expect("slip");
        controller
            .beat_roll(DeckId::A, BeatLength::QUARTER)
            .expect("roll");
    });
    let residual = residual_decibels(&track, &stems);
    assert!(
        residual < -120.0,
        "after a jump and a roll: {residual:.1} dB"
    );
}

#[test]
fn with_key_lock_the_stems_sound_like_the_track_and_mute_at_once() {
    let frames = SAMPLE_RATE as usize * 3;
    let (track, stems) = track_and_stems(SAMPLE_RATE, frames, |controller| {
        controller.set_key_lock(DeckId::A, true).expect("key lock");
        controller.set_tempo(DeckId::A, 1.06).expect("tempo");
    });
    let settled = SAMPLE_RATE as usize / 2;
    for stem in Stem::ALL {
        let tone = STEM_TONES[stem.index()];
        let from_track = tone_level(&track[settled..], tone);
        let from_stems = tone_level(&stems[settled..], tone);
        assert!(
            (from_track - from_stems).abs() < 0.01,
            "{stem:?}: {from_track} from the track, {from_stems} from the stems"
        );
    }

    let track = mix_of_stems(SECONDS, SAMPLE_RATE);
    track
        .attach_stems(stems_like(SECONDS, SAMPLE_RATE))
        .expect("attach");
    let (mut controller, mut renderer) = playing_deck(track);
    controller.set_key_lock(DeckId::A, true).expect("key lock");
    controller.set_tempo(DeckId::A, 1.06).expect("tempo");
    controller
        .set_stems(
            DeckId::A,
            StemSettings::UNTOUCHED.with_muted(Stem::Melody, true),
        )
        .expect("mute the melody");
    render(&mut renderer, SAMPLE_RATE as usize);
    controller
        .set_stems(
            DeckId::A,
            StemSettings::UNTOUCHED.with_muted(Stem::Vocals, true),
        )
        .expect("mute the vocals");
    let heard_from = FADE_FRAMES + common::mixer_latency_frames(&controller) + LIMITER_FRAMES;
    let after = render(&mut renderer, heard_from + SAMPLE_RATE as usize / 4);
    let vocals = tone_level(&after[heard_from..], STEM_TONES[Stem::Vocals.index()]);
    assert!(
        vocals < 0.002,
        "the vocals were still heard at {vocals} after {} ms",
        heard_from * 1_000 / SAMPLE_RATE as usize
    );
}

/// A track holding the four stems and a tone none of them holds, as a real separation leaves
/// part of the track out of every stem.
fn track_with_more_than_its_stems() -> Track {
    let rate = f64::from(SAMPLE_RATE);
    let stems = mix_of_stems(SECONDS, SAMPLE_RATE);
    let frames = (SECONDS * rate).round() as usize;
    let samples: Vec<f32> = (0..frames)
        .flat_map(|index| {
            let left_and_right = stems.frame(index).expect("a decoded frame");
            let missing = ((std::f64::consts::TAU * MISSING_TONE * index as f64 / rate).sin()
                * STEM_AMPLITUDE) as f32;
            [left_and_right[0] + missing, left_and_right[1] + missing]
        })
        .collect();
    Track::from_interleaved_stereo(&samples, SAMPLE_RATE).expect("valid test track")
}

const MISSING_TONE: f64 = 400.0;

#[test]
fn what_the_separation_left_out_plays_with_the_drums_and_bass_and_never_in_an_acapella() {
    let (_plain_controller, mut plain_renderer) = playing_deck(track_with_more_than_its_stems());
    let plain = render(&mut plain_renderer, SAMPLE_RATE as usize);

    let track = track_with_more_than_its_stems();
    track
        .attach_stems(stems_like(SECONDS, SAMPLE_RATE))
        .expect("attach");
    let (mut controller, mut renderer) = playing_deck(track);
    let untouched = render(&mut renderer, SAMPLE_RATE as usize);
    let residual = residual_decibels(&plain, &untouched);
    assert!(
        residual < -120.0,
        "every stem up: {residual:.1} dB from the track"
    );

    controller
        .set_stems(DeckId::A, StemSettings::UNTOUCHED.without(Stem::Vocals))
        .expect("instrumental");
    render(&mut renderer, FADE_FRAMES * 4);
    let instrumental = render(&mut renderer, SAMPLE_RATE as usize);
    assert!((tone_level(&instrumental, MISSING_TONE) - STEM_AMPLITUDE).abs() < 0.01);
    assert!(tone_level(&instrumental, STEM_TONES[Stem::Vocals.index()]) < 1e-4);

    controller
        .set_stems(DeckId::A, StemSettings::UNTOUCHED.solo(Stem::Vocals))
        .expect("acapella");
    render(&mut renderer, FADE_FRAMES * 4);
    let acapella = render(&mut renderer, SAMPLE_RATE as usize);
    assert!(tone_level(&acapella, MISSING_TONE) < 1e-3);
    assert!(
        (tone_level(&acapella, STEM_TONES[Stem::Vocals.index()]) - STEM_AMPLITUDE).abs() < 0.01
    );
}

#[test]
fn with_key_lock_every_stem_up_sounds_like_the_track() {
    let frames = SAMPLE_RATE as usize * 2;
    let render_with = |stems: bool| {
        let track = track_with_more_than_its_stems();
        if stems {
            track
                .attach_stems(stems_like(SECONDS, SAMPLE_RATE))
                .expect("attach");
        }
        let (mut controller, mut renderer) = playing_deck(track);
        controller.set_key_lock(DeckId::A, true).expect("key lock");
        controller.set_tempo(DeckId::A, 0.94).expect("tempo");
        if stems {
            controller
                .set_stems(
                    DeckId::A,
                    StemSettings::UNTOUCHED.with_muted(Stem::Melody, true),
                )
                .expect("touch the stems");
            controller
                .set_stems(DeckId::A, StemSettings::UNTOUCHED)
                .expect("every stem back up");
        }
        render(&mut renderer, frames)
    };
    let settled = SAMPLE_RATE as usize / 2;
    let track = render_with(false);
    let stems = render_with(true);
    for tone in STEM_TONES.into_iter().chain([MISSING_TONE]) {
        let from_track = tone_level(&track[settled..], tone);
        let from_stems = tone_level(&stems[settled..], tone);
        assert!(
            (from_track - from_stems).abs() < 0.01,
            "{tone} Hz: {from_track} from the track, {from_stems} from the stems"
        );
    }
}

#[test]
fn stems_of_another_length_or_rate_are_refused() {
    let track = mix_of_stems(SECONDS, SAMPLE_RATE);
    let error = track
        .attach_stems(stems_like(SECONDS + 1.0, SAMPLE_RATE))
        .unwrap_err();
    assert!(
        matches!(error, EngineError::StemsDoNotMatch { .. }),
        "{error}"
    );
    let error = track.attach_stems(stems_like(SECONDS, 44_100)).unwrap_err();
    assert!(
        matches!(error, EngineError::StemsSampleRate { .. }),
        "{error}"
    );
    track
        .attach_stems(stems_like(SECONDS, SAMPLE_RATE))
        .expect("attach");
    assert!(track.has_stems());
    assert!(matches!(
        track.attach_stems(stems_like(SECONDS, SAMPLE_RATE)),
        Err(EngineError::StemsAlreadyAttached)
    ));
}

#[test]
fn a_deck_without_stems_plays_the_track_whatever_the_stem_settings() {
    let track = mix_of_stems(SECONDS, SAMPLE_RATE);
    let (_plain_controller, mut plain_renderer) = playing_deck(track);
    let plain = render(&mut plain_renderer, SAMPLE_RATE as usize);
    let (mut controller, mut renderer) = playing_deck(mix_of_stems(SECONDS, SAMPLE_RATE));
    controller
        .set_stems(DeckId::A, StemSettings::UNTOUCHED.solo(Stem::Drums))
        .expect("solo");
    let rendered = render(&mut renderer, SAMPLE_RATE as usize);
    assert!(residual_decibels(&plain, &rendered) < -120.0);
}
