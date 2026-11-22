//! What tells one track apart from every other, for what Dubplate keeps about a track: its
//! rekordbox id when rekordbox holds it, which stays the same when its file is relinked, and
//! otherwise its audio file.

use std::path::PathBuf;

/// One track, whichever library it comes from.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TrackIdentity {
    /// A track of the rekordbox collection, by its id there.
    Rekordbox(String),
    /// An audio file rekordbox does not hold, by its path.
    File(PathBuf),
}

/// How each kind of identity is written in Dubplate's database.
const REKORDBOX_KIND: &str = "rekordbox";
const FILE_KIND: &str = "file";

impl TrackIdentity {
    /// The kind and key the database keeps it as.
    pub(crate) fn storage_key(&self) -> (&'static str, std::borrow::Cow<'_, str>) {
        match self {
            Self::Rekordbox(id) => (REKORDBOX_KIND, std::borrow::Cow::Borrowed(id.as_str())),
            Self::File(path) => (FILE_KIND, path.to_string_lossy()),
        }
    }

    /// The identity the database keeps as `kind` and `key`, or `None` for a kind this version
    /// does not know.
    pub(crate) fn from_storage_key(kind: &str, key: String) -> Option<Self> {
        match kind {
            REKORDBOX_KIND => Some(Self::Rekordbox(key)),
            FILE_KIND => Some(Self::File(PathBuf::from(key))),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identities_round_trip_through_their_storage_key() {
        for identity in [
            TrackIdentity::Rekordbox("258436394".to_owned()),
            TrackIdentity::File(PathBuf::from("/Music/Café/one.mp3")),
        ] {
            let (kind, key) = identity.storage_key();
            assert_eq!(
                TrackIdentity::from_storage_key(kind, key.into_owned()),
                Some(identity)
            );
        }
        assert_eq!(
            TrackIdentity::from_storage_key("serato", String::new()),
            None
        );
    }
}
