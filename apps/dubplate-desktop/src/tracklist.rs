//! A page of the history as a tracklist: plain text with each track's time into the mix, for
//! Mixcloud or a video's chapters, or CSV for a spreadsheet.

use dubplate_prep::Play;
use jiff::tz::TimeZone;

use crate::history::local_time;

/// The file a tracklist is written as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TracklistFormat {
    /// One line per track: `04:31  Artist - Title`.
    Text,
    /// A row per track with its start, deck and length.
    Csv,
}

impl TracklistFormat {
    /// Both formats, in the order the browser offers them.
    pub const ALL: [Self; 2] = [Self::Text, Self::Csv];

    /// The file extension.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Text => "txt",
            Self::Csv => "csv",
        }
    }

    /// The name on the export button.
    pub fn label(self) -> &'static str {
        match self {
            Self::Text => "TXT",
            Self::Csv => "CSV",
        }
    }
}

/// The CSV columns.
const CSV_HEADER: &str = "Start,Offset,Deck,Artist,Title,Seconds played,File";

/// `plays`, oldest first, as a tracklist in `format`, with times on the clock of `time_zone`.
pub fn tracklist(format: TracklistFormat, plays: &[&Play], time_zone: &TimeZone) -> String {
    let Some(first) = plays.first() else {
        return String::new();
    };
    let offset_of = |play: &Play| {
        play.started_at
            .duration_since(first.started_at)
            .unwrap_or_default()
            .as_secs()
    };
    let with_hours = plays
        .last()
        .is_some_and(|last| offset_of(last) >= SECONDS_PER_HOUR);
    match format {
        TracklistFormat::Text => {
            let started = local_time(first.started_at, time_zone);
            let mut text = format!(
                "Dubplate history, {}\n\n",
                started.strftime("%Y-%m-%d from %H:%M")
            );
            for play in plays {
                let offset = offset_clock(offset_of(play), with_hours);
                text.push_str(&format!("{offset}  {}\n", track_name(play)));
            }
            text
        }
        TracklistFormat::Csv => {
            let mut csv = format!("{CSV_HEADER}\r\n");
            for play in plays {
                let fields = [
                    local_time(play.started_at, time_zone)
                        .strftime("%Y-%m-%d %H:%M:%S")
                        .to_string(),
                    offset_clock(offset_of(play), with_hours),
                    play.deck.letter().to_string(),
                    play.artist.clone(),
                    play.title.clone(),
                    play.played.as_secs().to_string(),
                    play.track_path.display().to_string(),
                ];
                let row: Vec<String> = fields.iter().map(|field| csv_field(field)).collect();
                csv.push_str(&row.join(","));
                csv.push_str("\r\n");
            }
            csv
        }
    }
}

const SECONDS_PER_HOUR: u64 = 3600;

/// `Artist - Title`, or the title alone when the artist is unknown.
fn track_name(play: &Play) -> String {
    if play.artist.trim().is_empty() {
        play.title.clone()
    } else {
        format!("{} - {}", play.artist, play.title)
    }
}

/// Seconds into the mix as `mm:ss`, or `h:mm:ss` throughout a mix of an hour or more, as
/// video chapters and Mixcloud read them.
fn offset_clock(seconds: u64, with_hours: bool) -> String {
    let hours = seconds / SECONDS_PER_HOUR;
    let minutes = seconds / 60 % 60;
    let rest = seconds % 60;
    if with_hours {
        format!("{hours}:{minutes:02}:{rest:02}")
    } else {
        format!("{:02}:{rest:02}", seconds / 60)
    }
}

/// A CSV field, quoted when it holds a comma, a quote or a line break, as RFC 4180 has it.
fn csv_field(field: &str) -> String {
    if field.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{Duration, UNIX_EPOCH};

    use dubplate_prep::PlayDeck;

    use super::*;

    /// 2026-11-22 22:00 UTC.
    const SESSION_START: u64 = 1_795_384_800;

    fn play(artist: &str, title: &str, deck: char, offset_seconds: u64) -> Play {
        Play {
            set: None,
            track_path: PathBuf::from(format!("/Music/{title}.mp3")),
            rekordbox_id: None,
            title: title.to_owned(),
            artist: artist.to_owned(),
            started_at: UNIX_EPOCH + Duration::from_secs(SESSION_START + offset_seconds),
            played: Duration::from_secs(301),
            deck: PlayDeck::new(deck).unwrap(),
        }
    }

    #[test]
    fn text_counts_from_the_first_track() {
        let plays = [
            play("Kelvin Momo", "Amanxeba", 'A', 0),
            play("", "Untitled ID", 'B', 271),
        ];
        let plays: Vec<&Play> = plays.iter().collect();
        assert_eq!(
            tracklist(TracklistFormat::Text, &plays, &TimeZone::UTC),
            "Dubplate history, 2026-11-22 from 22:00\n\n\
             00:00  Kelvin Momo - Amanxeba\n\
             04:31  Untitled ID\n"
        );
    }

    #[test]
    fn a_mix_of_an_hour_or_more_shows_hours_on_every_line() {
        let plays = [play("A", "One", 'A', 0), play("B", "Two", 'B', 3_725)];
        let plays: Vec<&Play> = plays.iter().collect();
        let text = tracklist(TracklistFormat::Text, &plays, &TimeZone::UTC);
        assert!(text.contains("0:00:00  A - One\n"), "{text}");
        assert!(text.contains("1:02:05  B - Two\n"), "{text}");
    }

    #[test]
    fn csv_quotes_what_needs_quoting() {
        let plays = [
            play("Black Coffee, Pharrell", "Have \"Fun\"", 'B', 0),
            play("Shimza", "Lost", 'A', 330),
        ];
        let plays: Vec<&Play> = plays.iter().collect();
        assert_eq!(
            tracklist(TracklistFormat::Csv, &plays, &TimeZone::UTC),
            "Start,Offset,Deck,Artist,Title,Seconds played,File\r\n\
             2026-11-22 22:00:00,00:00,B,\"Black Coffee, Pharrell\",\"Have \"\"Fun\"\"\",301,\
             \"/Music/Have \"\"Fun\"\".mp3\"\r\n\
             2026-11-22 22:05:30,05:30,A,Shimza,Lost,301,/Music/Lost.mp3\r\n"
        );
    }

    #[test]
    fn an_empty_page_is_an_empty_file() {
        for format in TracklistFormat::ALL {
            assert_eq!(tracklist(format, &[], &TimeZone::UTC), "");
        }
    }
}
