use crate::channel::KNOB_CENTRE;

/// Where the bass swap starts and ends on the crossfader's travel.
const SWAP_START: f32 = 1.0 / 3.0;
const SWAP_END: f32 = 2.0 / 3.0;
/// How long the shared tempo takes to glide most of the way to where the crossfader puts it.
pub(crate) const TEMPO_GLIDE_SECONDS: f64 = 0.4;

/// What moves Smart Fader's blend, as rekordbox's Smart Fader setting offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SmartFaderSource {
    /// The crossfader, with the channel faders leaning the blend toward the louder deck.
    #[default]
    CrossfaderAndChannelFaders,
    /// The crossfader alone.
    Crossfader,
    /// The channel faders alone, with the crossfader off or ignored: pulling one deck's fader
    /// down blends toward the other deck.
    ChannelFaders,
}

impl SmartFaderSource {
    /// Every source, as a settings list shows them.
    pub const ALL: [Self; 3] = [
        Self::CrossfaderAndChannelFaders,
        Self::Crossfader,
        Self::ChannelFaders,
    ];

    /// Whether it needs the crossfader on with a deck on each side.
    pub(crate) fn uses_crossfader(self) -> bool {
        self != Self::ChannelFaders
    }
}

/// Where Smart Fader's blend sits, from 0 (the left deck) to 1 (the right deck), for the
/// crossfader at `crossfader` and the left and right decks' channel faders at `faders`. Channel
/// faders level with each other leave the crossfader's position as it is; one lower than the
/// other leans the blend toward the louder deck by half the difference.
pub(crate) fn blend_position(source: SmartFaderSource, crossfader: f32, faders: [f32; 2]) -> f32 {
    let lean = (faders[1] - faders[0]) / 2.0;
    let position = match source {
        SmartFaderSource::Crossfader => crossfader,
        SmartFaderSource::CrossfaderAndChannelFaders => crossfader + lean,
        SmartFaderSource::ChannelFaders => 0.5 + lean,
    };
    position.clamp(0.0, 1.0)
}

/// The LOW knob positions Smart Fader gives the left and the right deck at a crossfader `position`: the
/// outgoing deck's bass turns down from the centre to a kill while the incoming deck's comes
/// up from a kill to the centre, across the middle third of the fader.
pub(crate) fn bass_swap(position: f32) -> [f32; 2] {
    let swap = ((position - SWAP_START) / (SWAP_END - SWAP_START)).clamp(0.0, 1.0);
    [KNOB_CENTRE * (1.0 - swap), KNOB_CENTRE * swap]
}

/// The tempo both decks share at a crossfader `position`: the left deck's own BPM at the left,
/// the right deck's at the right, and between them in equal steps of pitch, so the middle of the fader, where
/// both decks are equally loud, sits half way.
pub(crate) fn shared_bpm(position: f32, left_bpm: f64, right_bpm: f64) -> f64 {
    let weight = f64::from(position.clamp(0.0, 1.0));
    (left_bpm.ln() * (1.0 - weight) + right_bpm.ln() * weight).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_source_puts_the_blend_where_its_faders_say() {
        let level = [1.0, 1.0];
        for source in SmartFaderSource::ALL {
            let expected = if source == SmartFaderSource::ChannelFaders {
                0.5
            } else {
                0.2
            };
            assert_eq!(blend_position(source, 0.2, level), expected, "{source:?}");
        }
        let left_down = [0.4, 1.0];
        assert!(
            (blend_position(SmartFaderSource::ChannelFaders, 0.2, left_down) - 0.8).abs() < 1e-6
        );
        assert!(
            (blend_position(SmartFaderSource::CrossfaderAndChannelFaders, 0.2, left_down) - 0.5)
                .abs()
                < 1e-6
        );
        assert_eq!(
            blend_position(SmartFaderSource::Crossfader, 0.2, left_down),
            0.2
        );
        assert_eq!(
            blend_position(SmartFaderSource::ChannelFaders, 0.5, [0.0, 1.0]),
            1.0,
            "a closed left fader blends all the way to the right deck"
        );
    }

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
