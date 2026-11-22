//! The waveforms Dubplate draws from a track's audio, kept by the track's file for the session,
//! so the deck and the track check share one drawing and no track is drawn twice.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dubplate_engine::{DecodeStatus, Track};

use crate::audio_waveform::three_band_waveforms;
use crate::waveform::{OverviewWaveform, Zoom, ZoomedWaveform};

/// A track's waveforms drawn from its audio: the scrolling one at the lanes' zoom and the
/// whole-track overview.
#[derive(Debug)]
pub struct DrawnWaveforms {
    /// The scrolling waveform.
    pub zoomed: Arc<ZoomedWaveform>,
    /// The whole track's waveform.
    pub overview: Option<OverviewWaveform>,
}

impl DrawnWaveforms {
    /// Draws the waveforms of `audio`, which must be fully decoded. Heavy: run it off the
    /// interface thread. `None` when the audio is too short to draw.
    pub fn draw(audio: &Track) -> Option<Self> {
        let waveforms = three_band_waveforms(audio);
        Some(Self {
            zoomed: Arc::new(ZoomedWaveform::from_waveforms(&waveforms, Zoom::DEFAULT)?),
            overview: OverviewWaveform::from_waveforms(&waveforms),
        })
    }
}

/// Whether `audio` has finished decoding, successfully or not, so its waveform can be drawn
/// from what it holds.
pub fn decoded(audio: &Track) -> bool {
    audio.status() != DecodeStatus::Decoding
}

/// The waveforms drawn so far, and the tracks being drawn.
#[derive(Debug, Default)]
pub struct WaveformCache {
    drawn: HashMap<PathBuf, Arc<DrawnWaveforms>>,
    drawing: HashSet<PathBuf>,
}

impl WaveformCache {
    /// The waveforms drawn for the track whose audio file is `path`.
    pub fn get(&self, path: &Path) -> Option<Arc<DrawnWaveforms>> {
        self.drawn.get(path).cloned()
    }

    /// Takes on drawing the track at `path`. False when it is drawn or being drawn already,
    /// so whoever asks second waits for the first drawing instead of starting another.
    pub fn claim(&mut self, path: &Path) -> bool {
        !self.drawn.contains_key(path) && self.drawing.insert(path.to_path_buf())
    }

    /// Keeps the waveforms drawn for `path`.
    pub fn finish(&mut self, path: PathBuf, waveforms: Arc<DrawnWaveforms>) {
        self.drawing.remove(&path);
        self.drawn.insert(path, waveforms);
    }

    /// Gives up drawing `path`, so the next to ask draws it.
    pub fn abandon(&mut self, path: &Path) {
        self.drawing.remove(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn waveforms() -> Arc<DrawnWaveforms> {
        let seconds = 2;
        let samples: Vec<f32> = (0..48_000 * seconds * 2)
            .map(|index| ((index / 2) as f32 * 0.05).sin() * 0.5)
            .collect();
        let audio = Track::from_interleaved_stereo(&samples, 48_000).unwrap();
        assert!(decoded(&audio));
        Arc::new(DrawnWaveforms::draw(&audio).unwrap())
    }

    #[test]
    fn a_track_is_drawn_once_whoever_asks_first() {
        let mut cache = WaveformCache::default();
        let path = Path::new("/Music/DJ tracks/amapiano/warmup/Uthando.m4a");
        assert!(cache.claim(path), "the first to ask draws it");
        assert!(!cache.claim(path), "the second waits for that drawing");
        assert!(cache.get(path).is_none());
        let drawn = waveforms();
        cache.finish(path.to_path_buf(), Arc::clone(&drawn));
        assert!(cache.get(path).is_some_and(|got| Arc::ptr_eq(&got, &drawn)));
        assert!(!cache.claim(path), "a drawn track is never drawn again");
    }

    #[test]
    fn an_abandoned_drawing_can_be_taken_up_again() {
        let mut cache = WaveformCache::default();
        let path = Path::new("/Music/Track.mp3");
        assert!(cache.claim(path));
        cache.abandon(path);
        assert!(cache.claim(path));
    }
}
