use std::f32::consts::PI;

/// How many octaves an endless glide spreads its voices over.
pub(crate) const SHEPARD_VOICES: usize = 6;

/// The phase of an endless glide: a position from 0 to 1 that moves by one voice spacing per
/// cycle, and the bell-shaped level of each voice at that position, so a voice fades in at the
/// bottom, rises through the middle and fades out at the top while the sum never changes.
///
/// This is the Shepard illusion behind the endlessly rising or falling jets and tones of a DJ
/// mixer's Enigma Jet and Mobius.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct EndlessGlide {
    position: f64,
}

/// One voice of an endless glide: where it sits, from 0 at the bottom to 1 at the top, and how
/// loud it plays there.
#[derive(Clone, Copy, Debug)]
pub(crate) struct GlideVoice {
    pub(crate) height: f32,
    pub(crate) level: f32,
}

impl EndlessGlide {
    pub(crate) fn reset(&mut self) {
        self.position = 0.0;
    }

    /// Moves by `step` voice spacings: positive rises, negative falls.
    #[inline]
    pub(crate) fn advance(&mut self, step: f64) {
        self.position = (self.position + step).rem_euclid(1.0);
    }

    /// The voices, lowest first.
    #[inline]
    pub(crate) fn voices(&self) -> [GlideVoice; SHEPARD_VOICES] {
        std::array::from_fn(|index| {
            let height = ((index as f64 + self.position) / SHEPARD_VOICES as f64) as f32;
            let bell = (PI * height).sin();
            GlideVoice {
                height,
                level: bell * bell,
            }
        })
    }
}
