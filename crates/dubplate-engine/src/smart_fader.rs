use crate::channel::KNOB_CENTRE;

/// Where the bass swap starts and ends on the crossfader's travel.
const SWAP_START: f32 = 1.0 / 3.0;
const SWAP_END: f32 = 2.0 / 3.0;
/// How long the shared tempo takes to glide most of the way to where the crossfader puts it.
pub(crate) const TEMPO_GLIDE_SECONDS: f64 = 0.4;

/// The LOW knob positions Smart Fader gives decks A and B at a crossfader `position`: the
/// outgoing deck's bass turns down from the centre to a kill while the incoming deck's comes
/// up from a kill to the centre, across the middle third of the fader.
pub(crate) fn bass_swap(position: f32) -> [f32; 2] {
    let swap = ((position - SWAP_START) / (SWAP_END - SWAP_START)).clamp(0.0, 1.0);
    [KNOB_CENTRE * (1.0 - swap), KNOB_CENTRE * swap]
}

/// The tempo both decks share at a crossfader `position`: deck A's own BPM at the left, deck
/// B's at the right, and between them in equal steps of pitch, so the middle of the fader, where
/// both decks are equally loud, sits half way.
pub(crate) fn shared_bpm(position: f32, bpm_a: f64, bpm_b: f64) -> f64 {
    let weight = f64::from(position.clamp(0.0, 1.0));
    (bpm_a.ln() * (1.0 - weight) + bpm_b.ln() * weight).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bass_swaps_across_the_middle_third() {
        assert_eq!(bass_swap(0.0), [0.5, 0.0]);
        assert_eq!(bass_swap(1.0 / 3.0), [0.5, 0.0]);
        let [outgoing, incoming] = bass_swap(0.5);
        assert!((outgoing - 0.25).abs() < 1e-6 && (incoming - 0.25).abs() < 1e-6);
        assert_eq!(bass_swap(1.0), [0.0, 0.5]);
    }

    #[test]
    fn the_shared_tempo_moves_from_one_deck_to_the_other() {
        assert!((shared_bpm(0.0, 120.0, 130.0) - 120.0).abs() < 1e-9);
        assert!((shared_bpm(1.0, 120.0, 130.0) - 130.0).abs() < 1e-9);
        let middle = shared_bpm(0.5, 120.0, 130.0);
        assert!((middle - (120.0_f64 * 130.0).sqrt()).abs() < 1e-9);
    }
}
