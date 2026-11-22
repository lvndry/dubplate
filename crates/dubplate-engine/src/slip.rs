/// Slip mode: while the DJ rolls, scratches or loops, a shadow playhead keeps playing at the
/// deck's tempo underneath, and the deck lands back on it, in rhythm, when they let go.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Slip {
    /// The SLIP button: scratches and loops slip too, not only rolls and held effects.
    pub(crate) enabled: bool,
    shadow: Option<f64>,
    holds: u8,
}

impl Slip {
    /// Starts a slipping action at `position`, in track frames. Actions can overlap; the shadow
    /// starts with the first and the deck returns when the last ends.
    pub(crate) fn begin(&mut self, position: f64) {
        if self.shadow.is_none() {
            self.shadow = Some(position);
        }
        self.holds = self.holds.saturating_add(1);
    }

    /// Ends a slipping action. Returns where the deck lands when it was the last one.
    pub(crate) fn end(&mut self) -> Option<f64> {
        if self.holds == 0 {
            return None;
        }
        self.holds -= 1;
        if self.holds == 0 {
            self.shadow.take()
        } else {
            None
        }
    }

    /// Moves the shadow on by `frames` of the track.
    pub(crate) fn advance(&mut self, frames: f64, end_frame: Option<f64>) {
        if let Some(shadow) = &mut self.shadow {
            *shadow += frames;
            if let Some(end) = end_frame {
                *shadow = shadow.min(end);
            }
        }
    }

    /// Where the shadow playhead is, while an action slips.
    pub(crate) fn shadow(&self) -> Option<f64> {
        self.shadow
    }

    /// Forgets the shadow, for a new track.
    pub(crate) fn clear(&mut self) {
        self.shadow = None;
        self.holds = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_deck_returns_when_the_last_overlapping_action_ends() {
        let mut slip = Slip::default();
        slip.begin(100.0);
        slip.advance(50.0, None);
        slip.begin(120.0);
        slip.advance(50.0, None);
        assert_eq!(slip.end(), None);
        assert_eq!(slip.shadow(), Some(200.0));
        assert_eq!(slip.end(), Some(200.0));
        assert_eq!(slip.end(), None);
        assert_eq!(slip.shadow(), None);
    }

    #[test]
    fn the_shadow_stops_at_the_end_of_the_track() {
        let mut slip = Slip::default();
        slip.begin(90.0);
        slip.advance(50.0, Some(100.0));
        assert_eq!(slip.shadow(), Some(100.0));
    }
}
