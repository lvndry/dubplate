const LOCK_SECONDS: f64 = 0.25;

/// The phase of a low-frequency oscillator locked to the beat grid.
///
/// The phase runs freely at the synced rate and is pulled gently towards the phase the beat
/// grid asks for, so a sweep lines up with bars but a jump in the grid (a hot cue, a new beat
/// length) glides into place instead of snapping the sweep.
#[derive(Clone, Debug)]
pub(crate) struct SyncedPhase {
    phase: f64,
    pull: f64,
    sample_rate: f32,
    locked: bool,
}

impl SyncedPhase {
    pub(crate) fn new() -> Self {
        Self {
            phase: 0.0,
            pull: 0.0,
            sample_rate: 0.0,
            locked: false,
        }
    }

    pub(crate) fn prepare(&mut self, sample_rate: f32) {
        if sample_rate != self.sample_rate {
            self.sample_rate = sample_rate;
            self.pull = 1.0 - (-1.0 / (LOCK_SECONDS * f64::from(sample_rate))).exp();
        }
    }

    pub(crate) fn unlock(&mut self) {
        self.locked = false;
    }

    /// Advances one sample and returns the phase, from 0 up to but not including 1.
    #[inline]
    pub(crate) fn next(
        &mut self,
        beat_position: f64,
        period_beats: f64,
        beats_per_sample: f64,
    ) -> f64 {
        let grid_phase = (beat_position / period_beats).rem_euclid(1.0);
        if self.locked {
            self.phase += beats_per_sample / period_beats;
            let error = (grid_phase - self.phase + 0.5).rem_euclid(1.0) - 0.5;
            self.phase = (self.phase + error * self.pull).rem_euclid(1.0);
        } else {
            self.phase = grid_phase;
            self.locked = true;
        }
        self.phase
    }
}

#[cfg(test)]
mod tests {
    use super::SyncedPhase;

    #[test]
    fn follows_the_beat_grid() {
        let mut phase = SyncedPhase::new();
        phase.prepare(48_000.0);
        let beats_per_sample = 2.0 / 48_000.0;
        let mut value = 0.0;
        for sample_index in 0..48_000 {
            value = phase.next(
                f64::from(sample_index) * beats_per_sample,
                4.0,
                beats_per_sample,
            );
        }
        let expected = (47_999.0 * beats_per_sample / 4.0_f64).rem_euclid(1.0);
        assert!((value - expected).abs() < 1e-9);
    }

    #[test]
    fn glides_after_the_grid_jumps() {
        let mut phase = SyncedPhase::new();
        phase.prepare(48_000.0);
        let beats_per_sample = 2.0 / 48_000.0;
        phase.next(0.0, 4.0, beats_per_sample);
        let after_jump = phase.next(2.0, 4.0, beats_per_sample);
        assert!(after_jump < 0.01);
    }
}
