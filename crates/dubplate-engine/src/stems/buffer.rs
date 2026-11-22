//! A track's four stems in memory, read on the audio thread at the deck's playhead.

use std::sync::Arc;

use crate::error::EngineError;
use crate::stems::Stem;
use crate::track::{FrameRead, MAX_TRACK_SECONDS, check_sample_rate};
use crate::voice::StereoFrames;

/// Channels of one stem frame: left and right of each stem, in the order of [`Stem::ALL`].
const STEM_CHANNELS: usize = Stem::COUNT * 2;

/// Channels of one part frame: every stem side by side, then the track itself.
pub(crate) const PART_CHANNELS: usize = STEM_CHANNELS + 2;

/// One frame of every stem and of the track, as key lock stretches them together.
pub(crate) type PartFrame = [f32; PART_CHANNELS];

const SAMPLE_SCALE: f32 = 1.0 / 32_768.0;

/// The four stems of a track, ready for a deck to play.
///
/// The stems are kept as 16-bit samples, as the separation writes them, so they take a quarter
/// of the memory of four decoded tracks: about 320 MB for seven minutes at 48 kHz. Each frame
/// holds every stem side by side, so reading the four stems at one position touches one cache
/// line.
#[derive(Clone)]
pub struct StemTracks {
    buffer: Arc<StemBuffer>,
}

struct StemBuffer {
    sample_rate: u32,
    frames: Box<[[i16; STEM_CHANNELS]]>,
}

impl StemTracks {
    /// Builds the stems from four stereo stems of interleaved 16-bit samples at `sample_rate`,
    /// in the order of [`Stem::ALL`]. Fails when the stems differ in length, the sample rate is
    /// outside 8 kHz to 384 kHz or they last longer than [`MAX_TRACK_SECONDS`].
    pub fn from_interleaved_stereo(
        stems: [&[i16]; Stem::COUNT],
        sample_rate: u32,
    ) -> Result<Self, EngineError> {
        check_sample_rate(sample_rate)?;
        let samples = stems[0].len();
        if stems.iter().any(|stem| stem.len() != samples) || !samples.is_multiple_of(2) {
            return Err(EngineError::InvalidParameter {
                name: "stem length in samples",
                value: samples as f64,
            });
        }
        let frame_count = samples / 2;
        if frame_count > MAX_TRACK_SECONDS * sample_rate as usize {
            return Err(EngineError::InvalidParameter {
                name: "stem length in seconds",
                value: frame_count as f64 / f64::from(sample_rate),
            });
        }
        let frames = (0..frame_count)
            .map(|frame| {
                let mut stem_frame = [0; STEM_CHANNELS];
                for (stem, samples) in stems.iter().enumerate() {
                    stem_frame[stem * 2] = samples[frame * 2];
                    stem_frame[stem * 2 + 1] = samples[frame * 2 + 1];
                }
                stem_frame
            })
            .collect();
        Ok(Self {
            buffer: Arc::new(StemBuffer {
                sample_rate,
                frames,
            }),
        })
    }

    /// The sample rate of the stems in Hz.
    pub fn sample_rate(&self) -> u32 {
        self.buffer.sample_rate
    }

    /// Length in seconds.
    pub fn duration_seconds(&self) -> f64 {
        self.buffer.frames.len() as f64 / f64::from(self.buffer.sample_rate)
    }
}

impl std::fmt::Debug for StemTracks {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StemTracks")
            .field("sample_rate", &self.sample_rate())
            .field("duration_seconds", &self.duration_seconds())
            .finish()
    }
}

/// A track's stems as the track holds them, at the track's own sample rate, so they are read at
/// the very positions the track is.
pub(crate) struct AttachedStems {
    stems: StemTracks,
}

impl AttachedStems {
    pub(crate) fn new(stems: StemTracks) -> Self {
        Self { stems }
    }

    /// The left and right channels of `stem`, for the resampling kernel.
    #[inline]
    pub(crate) fn channel(&self, stem: Stem) -> StemChannel<'_> {
        StemChannel {
            frames: &self.stems.buffer.frames,
            offset: stem.index() * 2,
        }
    }
}

/// One stem's left and right channels inside the frames of every stem.
pub(crate) struct StemChannel<'stems> {
    frames: &'stems [[i16; STEM_CHANNELS]],
    offset: usize,
}

impl StemChannel<'_> {
    #[inline]
    fn sample(&self, frame: &[i16; STEM_CHANNELS], channel: usize) -> f32 {
        f32::from(frame[self.offset + channel]) * SAMPLE_SCALE
    }
}

impl StereoFrames for StemChannel<'_> {
    #[inline]
    fn read_frame(&self, index: i64) -> FrameRead {
        match usize::try_from(index)
            .ok()
            .and_then(|index| self.frames.get(index))
        {
            Some(frame) => FrameRead::Frame(self.sample(frame, 0), self.sample(frame, 1)),
            None => FrameRead::Outside,
        }
    }

    #[inline]
    fn visit_frames<const N: usize>(
        &self,
        first: i64,
        mut visit: impl FnMut(usize, f32, f32),
    ) -> bool {
        let start = first.clamp(0, self.frames.len() as i64);
        let end = first
            .saturating_add(N as i64)
            .clamp(start, self.frames.len() as i64);
        for index in start..end {
            let frame = &self.frames[index as usize];
            visit(
                (index - first) as usize,
                self.sample(frame, 0),
                self.sample(frame, 1),
            );
        }
        false
    }
}

/// The gain of what the separation left out of every stem, the track minus the sum of its
/// stems. It sits mostly in the low end, where the network does not make the stems add up to
/// the track, so it follows the louder of the drums and the bass: the deck sounds exactly like
/// the track with every stem up, an instrumental is the track minus its vocals, and an acapella
/// is the vocals stem alone.
#[inline]
fn residual_gain(gains: &[f32; Stem::COUNT]) -> f32 {
    gains[Stem::Drums.index()].max(gains[Stem::Bass.index()])
}

/// Mixes the track's frame `track` and its stems' frame `stems` to stereo at `gains`: each stem
/// at its gain, and the track minus the stems at the residual gain.
#[inline]
pub(crate) fn mix_parts(
    track: (f32, f32),
    stems: &[f32],
    gains: &[f32; Stem::COUNT],
) -> (f32, f32) {
    let residual = residual_gain(gains);
    let mut left = track.0 * residual;
    let mut right = track.1 * residual;
    for (stem, gain) in gains.iter().enumerate() {
        let difference = gain - residual;
        left += stems[stem * 2] * difference;
        right += stems[stem * 2 + 1] * difference;
    }
    (left, right)
}

/// Whether every gain is full, so the deck plays the track itself.
#[inline]
pub(crate) fn all_full(gains: &[f32; Stem::COUNT]) -> bool {
    gains.iter().all(|gain| *gain == 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stems(frames: usize) -> StemTracks {
        let samples: Vec<Vec<i16>> = (0..Stem::COUNT)
            .map(|stem| {
                (0..frames * 2)
                    .map(|sample| (stem as i16 + 1) * 1000 + sample as i16)
                    .collect()
            })
            .collect();
        StemTracks::from_interleaved_stereo(
            std::array::from_fn(|stem| samples[stem].as_slice()),
            44_100,
        )
        .unwrap()
    }

    #[test]
    fn each_stem_reads_back_on_its_own_channels() {
        let attached = AttachedStems::new(stems(8));
        for stem in Stem::ALL {
            let first = (stem.index() as f32 + 1.0) * 1000.0;
            assert_eq!(
                attached.channel(stem).read_frame(3),
                FrameRead::Frame((first + 6.0) * SAMPLE_SCALE, (first + 7.0) * SAMPLE_SCALE)
            );
            let mut visited = Vec::new();
            attached
                .channel(stem)
                .visit_frames::<4>(-2, |offset, left, _| visited.push((offset, left)));
            assert_eq!(
                visited,
                [(2, first * SAMPLE_SCALE), (3, (first + 2.0) * SAMPLE_SCALE)]
            );
        }
    }

    #[test]
    fn outside_the_stems_is_outside() {
        let attached = AttachedStems::new(stems(4));
        assert_eq!(
            attached.channel(Stem::Bass).read_frame(-1),
            FrameRead::Outside
        );
        assert_eq!(
            attached.channel(Stem::Bass).read_frame(4),
            FrameRead::Outside
        );
    }

    #[test]
    fn every_gain_full_mixes_back_the_track() {
        let stems = [0.1, -0.1, 0.2, 0.3, -0.4, 0.0, 0.05, 0.5];
        assert_eq!(
            mix_parts((0.7, -0.2), &stems, &[1.0; Stem::COUNT]),
            (0.7, -0.2)
        );
        let (left, right) = mix_parts((0.7, -0.2), &stems, &[0.0, 0.0, 0.0, 1.0]);
        assert!((left - 0.05).abs() < 1e-6 && (right - 0.5).abs() < 1e-6);
    }

    #[test]
    fn stems_of_different_lengths_are_refused() {
        let long = [0_i16; 8];
        let short = [0_i16; 6];
        assert!(
            StemTracks::from_interleaved_stereo([&long, &long, &short, &long], 44_100).is_err()
        );
        assert!(StemTracks::from_interleaved_stereo([&long; 4], 0).is_err());
        assert_eq!(
            StemTracks::from_interleaved_stereo([&long; 4], 44_100)
                .unwrap()
                .duration_seconds(),
            4.0 / 44_100.0
        );
    }
}
