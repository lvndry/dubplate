/// White noise from a xorshift generator: cheap, allocation-free and the same on every run.
#[derive(Clone, Copy, Debug)]
pub(crate) struct WhiteNoise {
    state: u32,
}

impl Default for WhiteNoise {
    fn default() -> Self {
        Self { state: 0x9E37_79B9 }
    }
}

impl WhiteNoise {
    /// The next sample, from -1 to 1.
    #[inline]
    pub(crate) fn next(&mut self) -> f32 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 17;
        self.state ^= self.state << 5;
        (self.state >> 8) as f32 / (1_u32 << 23) as f32 - 1.0
    }
}
