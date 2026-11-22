//! The four parts a track is separated into, which a deck can turn up, down or off one by one.

/// One of the four parts a track is separated into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Stem {
    /// Drums and percussion, the log drum's attack included.
    Drums,
    /// Bass lines and bass synths, the log drum's body included.
    Bass,
    /// Everything that is not drums, bass or vocals: keys, pads, leads and guitars.
    Melody,
    /// Lead and backing vocals.
    Vocals,
}

impl Stem {
    /// How many stems a track is separated into.
    pub const COUNT: usize = 4;

    /// Every stem, in the order of [`Stem::index`].
    pub const ALL: [Stem; Stem::COUNT] = [Stem::Drums, Stem::Bass, Stem::Melody, Stem::Vocals];

    /// The stem's place in [`Stem::ALL`], for arrays holding one value per stem.
    pub const fn index(self) -> usize {
        match self {
            Stem::Drums => 0,
            Stem::Bass => 1,
            Stem::Melody => 2,
            Stem::Vocals => 3,
        }
    }

    /// The name shown to the DJ, such as `Vocals`.
    pub const fn label(self) -> &'static str {
        match self {
            Stem::Drums => "Drums",
            Stem::Bass => "Bass",
            Stem::Melody => "Melody",
            Stem::Vocals => "Vocals",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_stem_sits_at_its_index() {
        for (index, stem) in Stem::ALL.into_iter().enumerate() {
            assert_eq!(stem.index(), index);
        }
    }
}
