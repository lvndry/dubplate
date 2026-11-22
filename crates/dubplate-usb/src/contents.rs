//! Where a track's audio file goes on the stick: `/Contents/<artist>/<album>/<file name>`, the
//! layout rekordbox uses, with names a FAT32 or exFAT stick accepts.

const CONTENTS: &str = "Contents";
const UNKNOWN_ARTIST: &str = "UnknownArtist";
const UNKNOWN_ALBUM: &str = "UnknownAlbum";
/// Characters FAT32 and exFAT refuse in a name.
const REFUSED: [char; 9] = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];
/// Characters kept of a folder or file name, well under FAT32's 255 so the whole path stays
/// short enough for the players.
const NAME_LIMIT: usize = 120;
const REPLACEMENT: char = '_';

/// The path players see for a file named `file_name` by `artist` on `album`.
pub(crate) fn contents_path(artist: Option<&str>, album: Option<&str>, file_name: &str) -> String {
    let folder = |name: Option<&str>, unknown: &str| {
        name.map(str::trim)
            .filter(|name| !name.is_empty())
            .map_or_else(|| unknown.to_owned(), |name| safe_name(name, NAME_LIMIT))
    };
    format!(
        "/{CONTENTS}/{}/{}/{}",
        folder(artist, UNKNOWN_ARTIST),
        folder(album, UNKNOWN_ALBUM),
        safe_file_name(file_name)
    )
}

/// `path` with ` (number)` before its extension, for a second file that would take the same
/// place: `/Contents/A/B/Track (2).mp3`.
pub(crate) fn numbered(path: &str, number: u32) -> String {
    let (folder, name) = path.rsplit_once('/').unwrap_or(("", path));
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => (stem, Some(extension)),
        _ => (name, None),
    };
    match extension {
        Some(extension) => format!("{folder}/{stem} ({number}).{extension}"),
        None => format!("{folder}/{stem} ({number})"),
    }
}

fn safe_file_name(file_name: &str) -> String {
    match file_name.rsplit_once('.') {
        Some((stem, extension)) if !stem.trim().is_empty() => format!(
            "{}.{}",
            safe_name(stem, NAME_LIMIT),
            safe_name(extension, NAME_LIMIT)
        ),
        _ => safe_name(file_name, NAME_LIMIT),
    }
}

/// `name` with the characters a stick refuses replaced, no leading or trailing spaces or
/// trailing dots, at most `limit` characters, and never empty.
fn safe_name(name: &str, limit: usize) -> String {
    let replaced: String = name
        .chars()
        .map(|character| {
            if REFUSED.contains(&character) || character.is_control() {
                REPLACEMENT
            } else {
                character
            }
        })
        .take(limit)
        .collect();
    let trimmed = replaced.trim().trim_end_matches('.').trim_end();
    if trimmed.is_empty() {
        REPLACEMENT.to_string()
    } else {
        trimmed.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_go_where_rekordbox_puts_them() {
        assert_eq!(
            contents_path(Some("Loopmasters"), None, "Demo Track 1.mp3"),
            "/Contents/Loopmasters/UnknownAlbum/Demo Track 1.mp3"
        );
        assert_eq!(
            contents_path(None, Some(" "), "SIREN.wav"),
            "/Contents/UnknownArtist/UnknownAlbum/SIREN.wav"
        );
    }

    #[test]
    fn names_a_stick_refuses_are_made_safe() {
        assert_eq!(
            contents_path(Some("AC/DC: Live?"), Some("Back..."), "What? *.flac"),
            "/Contents/AC_DC_ Live_/Back/What_ _.flac"
        );
        assert_eq!(
            contents_path(Some("Mörda & Sébastien"), None, "Ngiyabonga.m4a"),
            "/Contents/Mörda & Sébastien/UnknownAlbum/Ngiyabonga.m4a"
        );
    }

    #[test]
    fn a_second_file_in_the_same_place_is_numbered() {
        assert_eq!(
            numbered("/Contents/A/B/Track.mp3", 2),
            "/Contents/A/B/Track (2).mp3"
        );
        assert_eq!(
            numbered("/Contents/A/B/Track", 3),
            "/Contents/A/B/Track (3)"
        );
    }
}
