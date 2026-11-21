//! The deck buttons as a CDJ plays them, carried out on the engine: CUE, jumps, hot cues and
//! instant doubles. Every input reaches the engine through these, so the keyboard, the screen
//! and a controller behave the same.

use dubplate_engine::{Controller, DeckId, DeckState, EngineError};

/// A paused deck this close to its cue point, in seconds, is at the cue point.
const AT_CUE_SECONDS: f64 = 0.001;

/// Whether a deck sits at its cue point.
pub fn is_at_cue(state: &DeckState) -> bool {
    (state.position_seconds - state.cue_point_seconds).abs() <= AT_CUE_SECONDS
}

/// What CUE going down did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CueDown {
    /// The deck was playing: it went back to the cue point and stopped.
    BackToCue,
    /// The deck was stopped at its cue point: it plays until CUE comes up.
    Preview,
    /// The deck was stopped elsewhere: that place became the cue point.
    NewCuePoint,
}

/// CUE going down, as on a CDJ. A new cue point lands where `quantize` puts the deck's
/// position.
pub fn cue_down(
    controller: &mut Controller,
    deck: DeckId,
    quantize: impl FnOnce(f64) -> f64,
) -> Result<CueDown, EngineError> {
    let state = controller.deck_state(deck);
    if state.playing {
        controller.cue(deck)?;
        Ok(CueDown::BackToCue)
    } else if is_at_cue(&state) {
        controller.play(deck)?;
        Ok(CueDown::Preview)
    } else {
        jump(controller, deck, quantize(state.position_seconds))?;
        Ok(CueDown::NewCuePoint)
    }
}

/// CUE coming up after a [`CueDown::Preview`]: back to the cue point, stopped.
pub fn end_preview(controller: &mut Controller, deck: DeckId) -> Result<(), EngineError> {
    controller.cue(deck)
}

/// Moves a deck to `seconds`, playing or stopped as it was. A stopped deck also takes it as
/// its cue point, as a CDJ does.
pub fn jump(controller: &mut Controller, deck: DeckId, seconds: f64) -> Result<(), EngineError> {
    if !controller.deck_state(deck).playing {
        controller.set_cue_point(deck, seconds)?;
    }
    controller.seek(deck, seconds)
}

/// Puts the track of `source` on `target` at the same place and cue point, playing if
/// `source` plays: rekordbox's instant doubles. Returns `false` when `source` has no track.
pub fn double(
    controller: &mut Controller,
    source: DeckId,
    target: DeckId,
) -> Result<bool, EngineError> {
    let Some(track) = controller.track(source).cloned() else {
        return Ok(false);
    };
    let state = controller.deck_state(source);
    controller.load_track(target, track)?;
    if let Some(beat_grid) = controller.beat_grid(source) {
        controller.set_beat_grid(target, Some(beat_grid))?;
    }
    controller.set_cue_point(target, state.cue_point_seconds)?;
    controller.seek(target, state.position_seconds)?;
    if state.playing {
        controller.play(target)?;
    }
    Ok(true)
}

/// The deck across from `deck`.
pub fn other_deck(deck: DeckId) -> DeckId {
    match deck {
        DeckId::A => DeckId::B,
        DeckId::B => DeckId::A,
    }
}

#[cfg(test)]
mod tests {
    use dubplate_engine::{Renderer, Track, offline_engine};

    use super::*;

    const SAMPLE_RATE: u32 = 48_000;
    const BLOCK_FRAMES: usize = 480;

    /// An engine with a ten-second track on deck A.
    fn engine() -> (Controller, Renderer) {
        let (mut controller, renderer) = offline_engine(SAMPLE_RATE);
        let samples = vec![0.1; SAMPLE_RATE as usize * 10 * 2];
        let track = Track::from_interleaved_stereo(&samples, SAMPLE_RATE).unwrap();
        controller.load_track(DeckId::A, track).unwrap();
        (controller, renderer)
    }

    /// Plays one block of audio, which carries out every command sent before it.
    fn render(renderer: &mut Renderer) {
        let mut block = vec![0.0; BLOCK_FRAMES * 2];
        renderer.render(&mut block, 2);
    }

    fn block_seconds() -> f64 {
        BLOCK_FRAMES as f64 / f64::from(SAMPLE_RATE)
    }

    #[test]
    fn cue_while_stopped_away_from_the_cue_point_sets_a_quantized_one() {
        let (mut controller, mut renderer) = engine();
        controller.seek(DeckId::A, 2.3).unwrap();
        render(&mut renderer);
        let outcome = cue_down(&mut controller, DeckId::A, |_| 2.0).unwrap();
        render(&mut renderer);
        assert_eq!(outcome, CueDown::NewCuePoint);
        let state = controller.deck_state(DeckId::A);
        assert!(!state.playing);
        assert!((state.cue_point_seconds - 2.0).abs() < 1e-6, "{state:?}");
        assert!(is_at_cue(&state));
    }

    #[test]
    fn holding_cue_at_the_cue_point_plays_until_it_is_let_go() {
        let (mut controller, mut renderer) = engine();
        render(&mut renderer);
        assert_eq!(
            cue_down(&mut controller, DeckId::A, |seconds| seconds).unwrap(),
            CueDown::Preview
        );
        for _ in 0..10 {
            render(&mut renderer);
        }
        let playing = controller.deck_state(DeckId::A);
        assert!(playing.playing);
        assert!(
            playing.position_seconds >= 9.0 * block_seconds(),
            "{playing:?}"
        );
        end_preview(&mut controller, DeckId::A).unwrap();
        render(&mut renderer);
        let back = controller.deck_state(DeckId::A);
        assert!(!back.playing);
        assert!(is_at_cue(&back), "{back:?}");
    }

    #[test]
    fn cue_while_playing_goes_back_to_the_cue_point_and_stops() {
        let (mut controller, mut renderer) = engine();
        controller.play(DeckId::A).unwrap();
        for _ in 0..5 {
            render(&mut renderer);
        }
        assert_eq!(
            cue_down(&mut controller, DeckId::A, |seconds| seconds).unwrap(),
            CueDown::BackToCue
        );
        render(&mut renderer);
        let state = controller.deck_state(DeckId::A);
        assert!(!state.playing);
        assert_eq!(state.position_seconds, 0.0);
        for _ in 0..20 {
            render(&mut renderer);
        }
        assert_eq!(
            cue_down(&mut controller, DeckId::A, |seconds| seconds).unwrap(),
            CueDown::Preview,
            "{:?}",
            controller.deck_state(DeckId::A)
        );
    }

    #[test]
    fn a_jump_on_a_stopped_deck_moves_the_cue_point_with_it() {
        let (mut controller, mut renderer) = engine();
        render(&mut renderer);
        jump(&mut controller, DeckId::A, 0.0).unwrap();
        jump(&mut controller, DeckId::A, 6.5).unwrap();
        render(&mut renderer);
        let state = controller.deck_state(DeckId::A);
        assert!((state.position_seconds - 6.5).abs() < 1e-6);
        assert!(is_at_cue(&state));
    }

    #[test]
    fn instant_doubles_copy_the_place_the_cue_point_and_play() {
        let (mut controller, mut renderer) = engine();
        controller.set_cue_point(DeckId::A, 1.5).unwrap();
        controller.seek(DeckId::A, 3.0).unwrap();
        controller.play(DeckId::A).unwrap();
        for _ in 0..3 {
            render(&mut renderer);
        }
        assert!(double(&mut controller, DeckId::A, DeckId::B).unwrap());
        render(&mut renderer);
        let source = controller.deck_state(DeckId::A);
        let copy = controller.deck_state(DeckId::B);
        assert!(copy.loaded && copy.playing, "{copy:?}");
        assert!(
            (copy.position_seconds - source.position_seconds).abs() < 1e-6,
            "{source:?} {copy:?}"
        );
        assert!((copy.cue_point_seconds - 1.5).abs() < 1e-6);
    }

    #[test]
    fn an_empty_deck_has_nothing_to_double() {
        let (mut controller, _) = offline_engine(SAMPLE_RATE);
        assert!(!double(&mut controller, DeckId::B, DeckId::A).unwrap());
        assert_eq!(other_deck(DeckId::A), DeckId::B);
        assert_eq!(other_deck(DeckId::B), DeckId::A);
    }
}
