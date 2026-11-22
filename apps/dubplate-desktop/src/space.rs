//! Which deck Space plays or pauses: the one the room can hear, so the key follows the mix
//! instead of always meaning deck A. Shift+Space and the controller's PLAY buttons keep their
//! own deck.

use dubplate_engine::{ChannelFaderCurve, Crossfader, DeckId};

/// A channel fader this far up, and a crossfader letting this much of the channel through,
/// make a deck audible.
const AUDIBLE: f32 = 0.2;

/// What decides whether the room hears a deck.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeckOnAir {
    /// The channel fader, from 0.0 to 1.0.
    pub channel_fader: f32,
    /// Whether the deck is the tempo master that synced decks follow.
    pub is_tempo_master: bool,
}

/// The deck Space acts on, with the channel faders on `curve` and `crossfader` as the engine
/// plays them (its position, curve, assign switches, or off): the audible deck; of two
/// audible decks, the tempo master, else the louder one; deck A when neither is audible.
/// Never both, so one key can never stop a whole mix.
pub fn space_deck(
    decks: [DeckOnAir; 2],
    crossfader: &Crossfader,
    curve: ChannelFaderCurve,
) -> DeckId {
    let side = |deck: DeckId| crossfader.gain(deck);
    let audible = |deck: DeckId| {
        let channel = decks[deck.index()].channel_fader;
        channel > AUDIBLE && side(deck) > AUDIBLE
    };
    let loudness = |deck: DeckId| curve.gain(decks[deck.index()].channel_fader) * side(deck);
    match (audible(DeckId::A), audible(DeckId::B)) {
        (true, false) => DeckId::A,
        (false, true) => DeckId::B,
        (false, false) => DeckId::A,
        (true, true) => DeckId::ALL
            .into_iter()
            .find(|deck| decks[deck.index()].is_tempo_master)
            .unwrap_or(if loudness(DeckId::B) > loudness(DeckId::A) {
                DeckId::B
            } else {
                DeckId::A
            }),
    }
}

#[cfg(test)]
mod tests {
    use dubplate_engine::CrossfaderAssign;

    use super::*;

    fn on_air(decks: [DeckOnAir; 2], position: f32) -> DeckId {
        let crossfader = Crossfader {
            position,
            ..Crossfader::default()
        };
        space_deck(decks, &crossfader, ChannelFaderCurve::default())
    }

    fn deck(channel_fader: f32) -> DeckOnAir {
        DeckOnAir {
            channel_fader,
            is_tempo_master: false,
        }
    }

    #[test]
    fn space_plays_the_only_deck_the_room_can_hear() {
        assert_eq!(on_air([deck(0.0), deck(1.0)], 0.5), DeckId::B);
        assert_eq!(on_air([deck(1.0), deck(0.1)], 0.5), DeckId::A);
    }

    #[test]
    fn the_crossfader_can_cut_a_deck_out() {
        assert_eq!(
            on_air([deck(1.0), deck(1.0)], 1.0),
            DeckId::B,
            "the crossfader fully on B silences A"
        );
        assert_eq!(on_air([deck(1.0), deck(1.0)], 0.0), DeckId::A);
    }

    #[test]
    fn of_two_audible_decks_the_tempo_master_then_the_louder_one() {
        let master_b = DeckOnAir {
            is_tempo_master: true,
            ..deck(0.6)
        };
        assert_eq!(on_air([deck(1.0), master_b], 0.5), DeckId::B);
        assert_eq!(on_air([deck(0.6), deck(0.9)], 0.5), DeckId::B);
        assert_eq!(
            on_air([deck(0.8), deck(0.8)], 0.5),
            DeckId::A,
            "level decks without a master: deck A"
        );
    }

    #[test]
    fn with_nothing_audible_space_falls_back_to_deck_a() {
        assert_eq!(on_air([deck(0.1), deck(0.0)], 0.5), DeckId::A);
    }

    #[test]
    fn a_crossfader_turned_off_or_on_thru_cuts_no_deck_out() {
        let off = Crossfader {
            enabled: false,
            position: 1.0,
            ..Crossfader::default()
        };
        let master_a = DeckOnAir {
            is_tempo_master: true,
            ..deck(1.0)
        };
        let curve = ChannelFaderCurve::default();
        assert_eq!(space_deck([master_a, deck(1.0)], &off, curve), DeckId::A);
        let a_on_thru = Crossfader {
            position: 1.0,
            assign: [CrossfaderAssign::Thru, CrossfaderAssign::B],
            ..Crossfader::default()
        };
        assert_eq!(
            space_deck([master_a, deck(1.0)], &a_on_thru, curve),
            DeckId::A
        );
    }
}
