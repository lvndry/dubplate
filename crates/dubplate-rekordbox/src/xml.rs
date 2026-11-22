//! Writes a library in rekordbox's XML format, which rekordbox reads from the file set in
//! Preferences > Advanced > Database > rekordbox xml and shows in its rekordbox xml tree.
//!
//! The format follows Pioneer's "rekordbox xml format" list and the files rekordbox 6 and 7
//! write themselves: a `COLLECTION` of `TRACK`s, each with its `TEMPO` beat grid markers and
//! `POSITION_MARK` cues, then `PLAYLISTS` whose tracks point at the collection by `TrackID`.
//! Times are written in seconds with three decimals and tempos with two, as rekordbox does.

use std::path::Path;

use crate::model::HotCuePad;

/// The `DJ_PLAYLISTS` version of the format.
const FORMAT_VERSION: &str = "1.0.0";
/// The product named in the file.
const PRODUCT_NAME: &str = "Dubplate";
/// What rekordbox expects every `Location` to start with.
const LOCATION_PREFIX: &str = "file://localhost";
/// The only metre Dubplate's grids have.
const FOUR_FOUR: &str = "4/4";

/// A whole rekordbox XML library: the tracks and the playlists that order them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct XmlLibrary {
    /// The collection, each track with its own `track_id`.
    pub tracks: Vec<XmlTrack>,
    /// The playlists at the top of the playlist tree, in order.
    pub playlists: Vec<XmlPlaylist>,
}

/// A track of the collection.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct XmlTrack {
    /// The number playlists use to point at the track, unique in the file.
    pub track_id: u32,
    /// The audio file. rekordbox finds the track by it.
    pub location: std::path::PathBuf,
    /// The title.
    pub name: Option<String>,
    /// The artist.
    pub artist: Option<String>,
    /// The album.
    pub album: Option<String>,
    /// The genre.
    pub genre: Option<String>,
    /// The record label.
    pub label: Option<String>,
    /// The comment.
    pub comments: Option<String>,
    /// The key, as rekordbox names it in its key column.
    pub tonality: Option<String>,
    /// The average tempo in beats per minute.
    pub average_bpm: Option<f64>,
    /// The length in whole seconds.
    pub total_time_seconds: Option<u32>,
    /// The file's size in bytes.
    pub size: Option<u64>,
    /// The rating from 0 to 5 stars.
    pub rating: Option<u8>,
    /// When the track joined the library, as `yyyy-mm-dd`.
    pub date_added: Option<String>,
    /// The beat grid: a marker where it starts and wherever its tempo or its beats change.
    pub tempos: Vec<TempoMarker>,
    /// Memory cues, hot cues and saved loops.
    pub position_marks: Vec<PositionMark>,
}

/// Where a stretch of the beat grid starts, its tempo, and where that first beat is in its bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TempoMarker {
    /// The first beat of the stretch, in milliseconds from the start of the track.
    pub start_ms: u32,
    /// The tempo in hundredths of a BPM, as rekordbox's analysis keeps it.
    pub tempo_hundredths: u16,
    /// Where the first beat falls in its bar, from 1 (the downbeat) to 4.
    pub beat_in_bar: u16,
}

/// A cue or a saved loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PositionMark {
    /// The name rekordbox shows beside it.
    pub name: String,
    /// Where it is, in milliseconds from the start of the track.
    pub start_ms: u32,
    /// Where the loop ends, for a saved loop.
    pub loop_end_ms: Option<u32>,
    /// A memory cue, or a hot cue on its pad.
    pub slot: MarkSlot,
}

/// Whether a cue is a memory cue or sits on a hot cue pad.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkSlot {
    /// A memory cue.
    Memory,
    /// A hot cue on a pad.
    Hot(HotCuePad),
}

/// A playlist of collection tracks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct XmlPlaylist {
    /// The name.
    pub name: String,
    /// The `track_id` of each track, in playing order.
    pub track_ids: Vec<u32>,
}

impl XmlLibrary {
    /// The library as rekordbox XML, UTF-8, ready to write to a file.
    pub fn to_xml(&self) -> String {
        let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\n");
        out.push_str("<DJ_PLAYLISTS");
        attribute(&mut out, "Version", FORMAT_VERSION);
        out.push_str(">\n  <PRODUCT");
        attribute(&mut out, "Name", PRODUCT_NAME);
        attribute(&mut out, "Version", env!("CARGO_PKG_VERSION"));
        attribute(&mut out, "Company", PRODUCT_NAME);
        out.push_str("/>\n  <COLLECTION");
        attribute(&mut out, "Entries", &self.tracks.len().to_string());
        out.push_str(">\n");
        for track in &self.tracks {
            write_track(&mut out, track);
        }
        out.push_str("  </COLLECTION>\n  <PLAYLISTS>\n    <NODE Type=\"0\" Name=\"ROOT\"");
        attribute(&mut out, "Count", &self.playlists.len().to_string());
        out.push_str(">\n");
        for playlist in &self.playlists {
            write_playlist(&mut out, playlist);
        }
        out.push_str("    </NODE>\n  </PLAYLISTS>\n</DJ_PLAYLISTS>\n");
        out
    }
}

fn write_track(out: &mut String, track: &XmlTrack) {
    out.push_str("    <TRACK");
    attribute(out, "TrackID", &track.track_id.to_string());
    let texts = [
        ("Name", &track.name),
        ("Artist", &track.artist),
        ("Album", &track.album),
        ("Genre", &track.genre),
    ];
    for (name, value) in texts {
        optional_attribute(out, name, value.as_deref());
    }
    optional_attribute(
        out,
        "Size",
        track.size.map(|size| size.to_string()).as_deref(),
    );
    optional_attribute(
        out,
        "TotalTime",
        track
            .total_time_seconds
            .map(|seconds| seconds.to_string())
            .as_deref(),
    );
    optional_attribute(
        out,
        "AverageBpm",
        track.average_bpm.map(|bpm| format!("{bpm:.2}")).as_deref(),
    );
    optional_attribute(out, "DateAdded", track.date_added.as_deref());
    optional_attribute(
        out,
        "Rating",
        track
            .rating
            .map(|stars| rating_value(stars).to_string())
            .as_deref(),
    );
    optional_attribute(out, "Comments", track.comments.as_deref());
    optional_attribute(out, "Label", track.label.as_deref());
    optional_attribute(out, "Tonality", track.tonality.as_deref());
    attribute(out, "Location", &location_uri(&track.location));
    if track.tempos.is_empty() && track.position_marks.is_empty() {
        out.push_str("/>\n");
        return;
    }
    out.push_str(">\n");
    for tempo in &track.tempos {
        out.push_str("      <TEMPO");
        attribute(out, "Inizio", &seconds(tempo.start_ms));
        attribute(out, "Bpm", &bpm(tempo.tempo_hundredths));
        attribute(out, "Metro", FOUR_FOUR);
        attribute(out, "Battito", &battito(tempo.beat_in_bar).to_string());
        out.push_str("/>\n");
    }
    for mark in &track.position_marks {
        write_position_mark(out, mark);
    }
    out.push_str("    </TRACK>\n");
}

fn write_position_mark(out: &mut String, mark: &PositionMark) {
    out.push_str("      <POSITION_MARK");
    attribute(out, "Name", &mark.name);
    attribute(out, "Type", mark_type(mark));
    attribute(out, "Start", &seconds(mark.start_ms));
    optional_attribute(out, "End", mark.loop_end_ms.map(seconds).as_deref());
    let number = match mark.slot {
        MarkSlot::Memory => "-1".to_owned(),
        MarkSlot::Hot(pad) => pad.index().to_string(),
    };
    attribute(out, "Num", &number);
    out.push_str("/>\n");
}

/// The `Type` of a position mark: `0` for a cue, `4` for a loop.
fn mark_type(mark: &PositionMark) -> &'static str {
    match mark.loop_end_ms {
        Some(_) => "4",
        None => "0",
    }
}

fn write_playlist(out: &mut String, playlist: &XmlPlaylist) {
    out.push_str("      <NODE");
    attribute(out, "Name", &playlist.name);
    attribute(out, "Type", "1");
    attribute(out, "KeyType", "0");
    attribute(out, "Entries", &playlist.track_ids.len().to_string());
    out.push_str(">\n");
    for track_id in &playlist.track_ids {
        out.push_str("        <TRACK");
        attribute(out, "Key", &track_id.to_string());
        out.push_str("/>\n");
    }
    out.push_str("      </NODE>\n");
}

/// A rating in stars as rekordbox writes it: 0, 51, 102, 153, 204 or 255.
fn rating_value(stars: u8) -> u16 {
    u16::from(stars.min(5)) * 51
}

/// Milliseconds as seconds with three decimals, such as `12.345`, exactly.
fn seconds(milliseconds: u32) -> String {
    format!("{}.{:03}", milliseconds / 1000, milliseconds % 1000)
}

/// Hundredths of a BPM as a tempo with two decimals, such as `122.00`, exactly.
fn bpm(tempo_hundredths: u16) -> String {
    format!("{}.{:02}", tempo_hundredths / 100, tempo_hundredths % 100)
}

fn optional_attribute(out: &mut String, name: &str, value: Option<&str>) {
    if let Some(value) = value.filter(|value| !value.is_empty()) {
        attribute(out, name, value);
    }
}

fn attribute(out: &mut String, name: &str, value: &str) {
    out.push(' ');
    out.push_str(name);
    out.push_str("=\"");
    escape_into(out, value);
    out.push('"');
}

/// Writes `text` as the value of a double-quoted attribute. Tabs and line breaks are written
/// as character references, which attribute normalisation would otherwise turn into spaces,
/// and characters XML 1.0 cannot carry at all are left out.
fn escape_into(out: &mut String, text: &str) {
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' => out.push_str("&#9;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            allowed if is_xml_character(allowed) => out.push(allowed),
            _ => {}
        }
    }
}

fn is_xml_character(character: char) -> bool {
    matches!(
        character,
        '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}'
    )
}

/// The place in a 4/4 bar rekordbox reads, from 1 to 4, whatever count a grid carries: a beat
/// numbered 0 or past the bar after a downbeat shift would otherwise be dropped on import.
fn battito(beat_in_bar: u16) -> u16 {
    beat_in_bar.saturating_sub(1) % 4 + 1
}

/// The `Location` of an audio file as rekordbox writes it: `file://localhost` and the absolute
/// path with forward slashes, each byte of a character outside the set rekordbox leaves as it
/// is written as `%` and two lowercase hex digits, so `/Music/Café Del Mar.mp3` becomes
/// `file://localhost/Music/Caf%c3%a9%20Del%20Mar.mp3`.
pub fn location_uri(path: &Path) -> String {
    let bytes = path_bytes(path);
    let mut uri = String::with_capacity(LOCATION_PREFIX.len() + bytes.len() + 1);
    uri.push_str(LOCATION_PREFIX);
    if bytes.first() != Some(&b'/') {
        uri.push('/');
    }
    for byte in bytes.iter() {
        match byte {
            b'\\' => uri.push('/'),
            byte if is_left_unescaped(*byte) => uri.push(char::from(*byte)),
            byte => {
                const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";
                uri.push('%');
                uri.push(char::from(HEX_DIGITS[usize::from(byte >> 4)]));
                uri.push(char::from(HEX_DIGITS[usize::from(byte & 0x0f)]));
            }
        }
    }
    uri
}

/// The bytes rekordbox writes as they are in a `Location`: letters, digits, the path's slashes
/// and the drive colon, and the punctuation it never escapes.
fn is_left_unescaped(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'/' | b':' | b'-' | b'_' | b'.' | b'~' | b'(' | b')' | b',' | b'+' | b'!'
        )
}

#[cfg(unix)]
fn path_bytes(path: &Path) -> std::borrow::Cow<'_, [u8]> {
    use std::os::unix::ffi::OsStrExt;
    std::borrow::Cow::Borrowed(path.as_os_str().as_bytes())
}

#[cfg(not(unix))]
fn path_bytes(path: &Path) -> std::borrow::Cow<'_, [u8]> {
    match path.to_string_lossy() {
        std::borrow::Cow::Borrowed(text) => std::borrow::Cow::Borrowed(text.as_bytes()),
        std::borrow::Cow::Owned(text) => std::borrow::Cow::Owned(text.into_bytes()),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    #[test]
    fn battito_stays_in_the_bar() {
        assert_eq!([0, 1, 4, 5, 8].map(battito), [1, 1, 4, 1, 4]);
    }

    use super::*;

    #[test]
    fn locations_are_escaped_as_rekordbox_writes_them() {
        assert_eq!(
            location_uri(Path::new(
                "/Users/dj/Music/DJ tracks/Lemon & Herb - Are We Safe.m4a"
            )),
            "file://localhost/Users/dj/Music/DJ%20tracks/Lemon%20%26%20Herb%20-%20Are%20We%20Safe.m4a"
        );
        assert_eq!(
            location_uri(Path::new("/Music/Rønhöff (Edit), B’utiza!+.mp3")),
            "file://localhost/Music/R%c3%b8nh%c3%b6ff%20(Edit),%20B%e2%80%99utiza!+.mp3"
        );
        assert_eq!(
            location_uri(Path::new("/Music/50% #1 [Mix]?'.wav")),
            "file://localhost/Music/50%25%20%231%20%5bMix%5d%3f%27.wav"
        );
        assert_eq!(
            location_uri(Path::new(r"C:\Music\Track.mp3")),
            "file://localhost/C:/Music/Track.mp3"
        );
    }

    #[test]
    fn times_and_tempos_are_written_exactly() {
        assert_eq!(seconds(0), "0.000");
        assert_eq!(seconds(41), "0.041");
        assert_eq!(seconds(460_369), "460.369");
        assert_eq!(bpm(12_200), "122.00");
        assert_eq!(bpm(12_805), "128.05");
        assert_eq!(rating_value(3), 153);
        assert_eq!(rating_value(9), 255);
    }

    #[test]
    fn attribute_values_escape_markup_and_drop_what_xml_cannot_hold() {
        let mut out = String::new();
        escape_into(&mut out, "A&B <C> \"D\" 'E'\tF\nG\u{1}H");
        assert_eq!(
            out,
            "A&amp;B &lt;C&gt; &quot;D&quot; &apos;E&apos;&#9;F&#10;GH"
        );
    }

    #[test]
    fn a_track_without_grid_or_cues_is_one_empty_element() {
        let library = XmlLibrary {
            tracks: vec![XmlTrack {
                track_id: 1,
                location: PathBuf::from("/Music/Track.mp3"),
                ..XmlTrack::default()
            }],
            playlists: Vec::new(),
        };
        let xml = library.to_xml();
        assert!(
            xml.contains("<TRACK TrackID=\"1\" Location=\"file://localhost/Music/Track.mp3\"/>")
        );
        assert!(xml.contains("<NODE Type=\"0\" Name=\"ROOT\" Count=\"0\">"));
    }
}
