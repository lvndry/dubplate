//! Choosing the tempo master, following it, and Smart Fader.

use crate::curves::crossfader_gains;
use crate::deck::{Deck, DeckId};
use crate::smart_fader::{TEMPO_GLIDE_SECONDS, bass_swap, shared_bpm};
use crate::sync::SyncMode;
use crate::tempo::DeckLevel;

use super::Renderer;

impl Renderer {
    pub(super) fn choose_tempo_master(&mut self) -> Option<DeckId> {
        let (a_gain, b_gain) = crossfader_gains(self.crossfader);
        let levels = DeckId::ALL.map(|deck_id| {
            let deck = &self.decks[deck_id.index()];
            let crossfader_gain = if deck_id == DeckId::A { a_gain } else { b_gain };
            DeckLevel {
                deck: deck_id,
                has_beat_grid: deck.has_beat_grid(),
                level: deck.level(crossfader_gain),
                playing: deck.is_playing(),
            }
        });
        self.tempo_master.update(&levels)
    }

    /// Brings every synced deck other than the tempo master to the master's tempo, and with
    /// beat sync to its beats, for the next block.
    /// Smart Fader: the tempo master plays the tempo the crossfader puts between the two
    /// decks' own tempos, gliding there, and the LOW knobs swap across the middle of the fader.
    pub(super) fn drive_smart_fader(&mut self, master: Option<DeckId>, frame_count: usize) {
        if !self.smart_fader {
            return;
        }
        let lows = bass_swap(self.crossfader);
        for (deck, low) in self.decks.iter_mut().zip(lows) {
            deck.set_automatic_low(Some(low));
        }
        let own = self.decks.each_ref().map(Deck::own_bpm);
        let (Some(master), [Some(bpm_a), Some(bpm_b)]) = (master, own) else {
            return;
        };
        let target = shared_bpm(self.crossfader, bpm_a, bpm_b);
        let glide = 1.0
            - (-(frame_count as f64) / (TEMPO_GLIDE_SECONDS * f64::from(self.sample_rate))).exp();
        let bpm = match self.smart_bpm {
            Some(current) => current + (target - current) * glide,
            None => self.decks[master.index()].bpm().unwrap_or(target),
        };
        self.smart_bpm = Some(bpm);
        for deck_id in DeckId::ALL {
            let smart_bpm = (deck_id == master).then_some(bpm);
            self.decks[deck_id.index()].set_smart_bpm(smart_bpm);
        }
    }

    pub(super) fn set_smart_fader(&mut self, enabled: bool) {
        self.smart_fader = enabled;
        if !enabled {
            self.smart_bpm = None;
            for deck in &mut self.decks {
                deck.set_automatic_low(None);
                deck.set_smart_bpm(None);
            }
        }
    }

    pub(super) fn follow_tempo_master(&mut self, master: Option<DeckId>) {
        let master_beat = master.and_then(|deck| self.decks[deck.index()].master_beat(deck));
        let smart = self.smart_fader;
        for deck_id in DeckId::ALL {
            let deck = &mut self.decks[deck_id.index()];
            match master_beat {
                Some(beat)
                    if beat.deck != deck_id && (smart || deck.sync_mode() != SyncMode::Off) =>
                {
                    deck.follow(beat, self.sync_half_and_double, smart, &mut self.retire);
                }
                _ => deck.stop_following(),
            }
        }
    }
}
