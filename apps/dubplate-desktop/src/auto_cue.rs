//! Where a track is cued when it loads, as rekordbox's Preferences > Controller > Deck: at the
//! first memory or hot cue when the track has one (Load Setting), else at the first sound,
//! landed on the beat it starts with (Auto Cue), so a deck never waits at 0:00 in silence.

use dubplate_engine::Track;

use crate::waveform::BeatMarks;

/// The level the first sound must reach, as rekordbox's and the CDJ's default Auto Cue Level.
const FIRST_SOUND_DECIBELS: f32 = -36.0;
/// How far a beat may sit from the first sound and still take the cue: the onset of a kick is
/// a few milliseconds after its grid line.
const BEAT_SNAP_SECONDS: f64 = 0.08;
/// How far into a track Auto Cue looks for sound before it gives up and leaves the cue at the
/// start.
const SEARCH_SECONDS: f64 = 90.0;
/// Frames read at a time while looking.
const READ_FRAMES: usize = 4096;

/// What the search for a track's first sound found so far.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FirstSound {
    /// The track is heard from here, in seconds.
    At(f64),
    /// Nothing loud enough yet in the frames decoded; look again from this frame once more has
    /// decoded.
    Pending(usize),
    /// Nothing loud enough in the part searched: the cue stays at the start.
    Silent,
}

/// Looks for the first frame of `track` at or above Auto Cue's level, from `from_frame`.
pub fn first_sound(track: &Track, from_frame: usize) -> FirstSound {
    let threshold = 10.0_f32.powf(FIRST_SOUND_DECIBELS / 20.0);
    let sample_rate = f64::from(track.sample_rate());
    let search_frames = (SEARCH_SECONDS * sample_rate) as usize;
    let mut frames = [[0.0_f32; 2]; READ_FRAMES];
    let mut next = from_frame;
    while next < search_frames {
        let wanted = READ_FRAMES.min(search_frames - next);
        let read = track.read_frames(next, &mut frames[..wanted]);
        if let Some(offset) = frames[..read]
            .iter()
            .position(|[left, right]| left.abs().max(right.abs()) >= threshold)
        {
            return FirstSound::At((next + offset) as f64 / sample_rate);
        }
        next += read;
        if read < wanted {
            let decoded_to_the_end = track
                .duration_seconds()
                .is_some_and(|duration| next as f64 >= duration * sample_rate);
            return if decoded_to_the_end {
                FirstSound::Silent
            } else {
                FirstSound::Pending(next)
            };
        }
    }
    FirstSound::Silent
}

/// The cue for a track first heard at `first_sound` seconds: the beat it starts on when the
/// grid has one that close, so the deck starts exactly on the kick, else the first sound.
pub fn cue_at_first_sound(first_sound: f64, beats: Option<&BeatMarks>) -> f64 {
    beats
        .and_then(|beats| beats.nearest(first_sound))
        .filter(|beat| (beat - first_sound).abs() <= BEAT_SNAP_SECONDS && *beat >= 0.0)
        .unwrap_or(first_sound)
}

/// The cue Load Setting picks: the earliest of the track's memory cues and hot cues.
pub fn earliest_saved_cue(
    memory_cues: impl IntoIterator<Item = f64>,
    hot_cues: impl IntoIterator<Item = f64>,
) -> Option<f64> {
    memory_cues
        .into_iter()
        .chain(hot_cues)
        .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
        .min_by(f64::total_cmp)
}

#[cfg(test)]
mod tests {
    use dubplate_rekordbox::analysis::{Beat, BeatGrid};

    use super::*;

    const SAMPLE_RATE: u32 = 48_000;

    fn track_with_silence(silent_seconds: f64, total_seconds: f64) -> Track {
        let silent_frames = (silent_seconds * f64::from(SAMPLE_RATE)) as usize;
        let total_frames = (total_seconds * f64::from(SAMPLE_RATE)) as usize;
        let mut samples = vec![0.0_f32; total_frames * 2];
        for sample in &mut samples[silent_frames * 2..] {
            *sample = 0.25;
        }
        Track::from_interleaved_stereo(&samples, SAMPLE_RATE).unwrap()
    }

    #[test]
    fn the_first_sound_is_found_past_the_silence() {
        let track = track_with_silence(1.5, 3.0);
        let FirstSound::At(seconds) = first_sound(&track, 0) else {
            panic!("no sound found");
        };
        assert!((seconds - 1.5).abs() < 1e-4, "{seconds}");
    }

    #[test]
    fn quiet_noise_below_the_level_is_not_the_first_sound() {
        let mut samples = vec![0.001_f32; SAMPLE_RATE as usize * 2 * 2];
        samples[SAMPLE_RATE as usize * 2] = 0.5;
        let track = Track::from_interleaved_stereo(&samples, SAMPLE_RATE).unwrap();
        assert_eq!(first_sound(&track, 0), FirstSound::At(1.0));
    }

    #[test]
    fn a_silent_track_keeps_its_cue_at_the_start() {
        let track = track_with_silence(2.0, 2.0);
        assert_eq!(first_sound(&track, 0), FirstSound::Silent);
    }

    #[test]
    fn the_cue_lands_on_the_beat_the_sound_starts_with() {
        let grid = BeatGrid {
            beats: (0..8_u16)
                .map(|index| Beat {
                    beat_in_bar: index % 4 + 1,
                    tempo_hundredths: 12_000,
                    time_ms: 500 + u32::from(index) * 500,
                })
                .collect(),
        };
        let beats = BeatMarks::from_grid(&grid);
        assert!((cue_at_first_sound(1.03, Some(&beats)) - 1.0).abs() < 1e-9);
        assert!((cue_at_first_sound(1.25, Some(&beats)) - 1.25).abs() < 1e-9);
        assert!((cue_at_first_sound(1.25, None) - 1.25).abs() < 1e-9);
    }

    #[test]
    fn the_earliest_memory_or_hot_cue_wins() {
        assert_eq!(earliest_saved_cue([32.0, 8.0], [16.0, 4.5]), Some(4.5));
        assert_eq!(earliest_saved_cue([], [12.0]), Some(12.0));
        assert_eq!(earliest_saved_cue([], []), None);
    }
}
