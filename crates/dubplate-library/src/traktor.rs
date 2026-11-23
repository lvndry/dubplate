//! Reads a collection file written by Traktor Pro 3 and 4, in the NML format.
//!
//! Traktor keeps its collection in `collection.nml`: an `NML` root with a `COLLECTION` of
//! `ENTRY` elements, each with its `LOCATION`, `ALBUM`, `INFO`, `TEMPO`, `MUSICAL_KEY` and
//! `CUE_V2` cues, then `PLAYLISTS`, a tree of `NODE` folders and playlists whose `PLAYLIST`
//! elements name their tracks by a `PRIMARYKEY`. The file is only read.
//!
//! Traktor writes paths with `/:` as the separator, such as `/:Users/:me/:Music/:`, and
//! prefixes each playlist key with the disk's name as its `VOLUME`; on the startup disk the
//! path starts at `/`. Cues are in milliseconds; a `TYPE 4` cue is the beat grid anchor, and
//! a `HOTCUE` of 0 to 7 puts a cue on a hot cue slot.

use std::path::{Path, PathBuf};

use roxmltree::{Document, Node};
use std::collections::HashMap;

/// A whole Traktor collection: the tracks and the playlist tree that orders them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TraktorCollection {
    /// Every track in the `COLLECTION`, in file order.
    pub tracks: Vec<TraktorTrack>,
    /// The folders and playlists at the top of the playlist tree, in order.
    pub playlists: Vec<TraktorPlaylistNode>,
}

/// A folder or a playlist of the playlist tree, in order, with its children below it.
#[derive(Debug, Clone, PartialEq)]
pub enum TraktorPlaylistNode {
    /// A folder, with its folders and playlists in order.
    Folder {
        /// The name as Traktor shows it.
        name: String,
        /// The folders and playlists inside, in order.
        children: Vec<TraktorPlaylistNode>,
    },
    /// A playlist, with the tracks it plays in playing order.
    Playlist {
        /// The name as Traktor shows it.
        name: String,
        /// The audio files of its tracks, in playing order.
        track_paths: Vec<PathBuf>,
    },
}

/// A track of the collection.
#[derive(Debug, Clone, PartialEq)]
pub struct TraktorTrack {
    /// The audio file, from its `LOCATION`.
    pub path: PathBuf,
    /// The title.
    pub title: Option<String>,
    /// The artist.
    pub artist: Option<String>,
    /// The album.
    pub album: Option<String>,
    /// The genre.
    pub genre: Option<String>,
    /// The comment.
    pub comment: Option<String>,
    /// The tempo in beats per minute.
    pub bpm: Option<f64>,
    /// The key: the pitch class from 0 for C to 11 for B, and whether it is minor.
    pub key: Option<(u8, bool)>,
    /// The rating from 0 to 5 stars.
    pub rating: u8,
    /// The length in seconds.
    pub duration_seconds: Option<f64>,
    /// The beat grid anchor: where the downbeat is, in milliseconds.
    pub grid_anchor_ms: Option<f64>,
    /// The hot cues, by slot from 0 to 7, in file order.
    pub hot_cues: Vec<(u8, TraktorCue)>,
    /// Every other cue: memory cues, saved loops, fades and loads, in file order.
    pub memory_cues: Vec<TraktorCue>,
}

/// One cue, in milliseconds from the start of the track.
#[derive(Debug, Clone, PartialEq)]
pub struct TraktorCue {
    /// Where the cue is, in milliseconds from the start of the track.
    pub start_ms: f64,
    /// The name as Traktor shows it.
    pub name: String,
    /// The loop length in milliseconds, when the cue is a loop.
    pub loop_length_ms: Option<f64>,
}

/// Why a Traktor collection file could not be read.
#[derive(Debug, thiserror::Error)]
pub enum TraktorError {
    /// The file could not be read.
    #[error("could not read the Traktor collection file {path}")]
    Read {
        /// The file that failed.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
    /// The file is not well-formed NML.
    #[error("the Traktor collection file {path} is not valid NML")]
    Malformed {
        /// The file that failed.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: roxmltree::Error,
    },
    /// Playlist folders exceed the reader's nesting limit.
    #[error("the playlist folders in {path} are nested too deeply")]
    PlaylistDepth {
        /// The input file.
        path: PathBuf,
    },
    /// The XML has a different root element.
    #[error("{path} does not contain an NML collection")]
    WrongRoot {
        /// The input file.
        path: PathBuf,
    },
}

/// Reads the collection file `path` as a [`TraktorCollection`].
///
/// Unknown attributes and elements are ignored, and a number Traktor wrote wrongly skips only
/// that field. The file is only read.
pub fn read_traktor_collection(path: &Path) -> Result<TraktorCollection, TraktorError> {
    let xml = std::fs::read_to_string(path).map_err(|source| TraktorError::Read {
        path: path.to_owned(),
        source,
    })?;
    let doc = Document::parse(&xml).map_err(|source| TraktorError::Malformed {
        path: path.to_owned(),
        source,
    })?;
    let root = doc.root_element();
    if !root.has_tag_name("NML") {
        return Err(TraktorError::WrongRoot {
            path: path.to_owned(),
        });
    }
    let mut tracks = Vec::new();
    let mut paths = HashMap::new();
    for entry in children(root, "COLLECTION").flat_map(|section| children(section, "ENTRY")) {
        if let Some((key, track)) = parse_track(entry) {
            paths.insert(key, track.path.clone());
            tracks.push(track);
        }
    }
    // Limit recursive construction and destruction of the public playlist tree.
    for section in children(root, "PLAYLISTS") {
        if section
            .descendants()
            .filter(|node| node.has_tag_name("NODE"))
            .any(|node| node.ancestors().nth(128).is_some())
        {
            return Err(TraktorError::PlaylistDepth {
                path: path.to_owned(),
            });
        }
    }
    let mut playlists = Vec::new();
    for node in children(root, "PLAYLISTS").flat_map(|section| children(section, "NODE")) {
        if node.attribute("NAME") == Some("$ROOT") && node.attribute("TYPE") == Some("FOLDER") {
            playlists.extend(node_children(node, &paths));
        } else if let Some(node) = parse_node(node, &paths) {
            playlists.push(node);
        }
    }
    Ok(TraktorCollection { tracks, playlists })
}

fn children<'a, 'input>(
    node: Node<'a, 'input>,
    name: &'static str,
) -> impl Iterator<Item = Node<'a, 'input>> {
    node.children()
        .filter(move |child| child.has_tag_name(name))
}

fn parse_track(entry: Node<'_, '_>) -> Option<(String, TraktorTrack)> {
    let location = children(entry, "LOCATION").next()?;
    let file = location.attribute("FILE").filter(|file| !file.is_empty())?;
    let dir = location.attribute("DIR")?;
    let volume = location.attribute("VOLUME").unwrap_or_default();
    let relative = format!("{}{file}", dir.replace("/:", "/"));
    let mut path = PathBuf::from(&relative);
    if let Some(mount) = location
        .attribute("MOUNTPOINT")
        .filter(|mount| mount.starts_with('/'))
    {
        if !path.starts_with(mount) {
            path = Path::new(mount).join(relative.trim_start_matches('/'));
        }
    } else if volume.len() == 2 && volume.ends_with(':') {
        path = PathBuf::from(format!("{volume}{relative}"));
    }
    let info = children(entry, "INFO").next();
    let field = |key| info.and_then(|info| info.attribute(key));
    let mut track = TraktorTrack {
        path,
        title: entry.attribute("TITLE").map(str::to_owned),
        artist: entry.attribute("ARTIST").map(str::to_owned),
        album: children(entry, "ALBUM")
            .next()
            .and_then(|album| album.attribute("TITLE"))
            .map(str::to_owned),
        genre: field("GENRE").map(str::to_owned),
        comment: field("COMMENT").map(str::to_owned),
        duration_seconds: field("PLAYTIME").and_then(parse_number),
        bpm: children(entry, "TEMPO")
            .next()
            .and_then(|tempo| tempo.attribute("BPM"))
            .and_then(parse_number),
        key: children(entry, "MUSICAL_KEY")
            .next()
            .and_then(|key| key.attribute("VALUE"))
            .and_then(parse_key),
        rating: field("RANKING").map_or(0, parse_rating),
        grid_anchor_ms: None,
        hot_cues: Vec::new(),
        memory_cues: Vec::new(),
    };
    for cue in children(entry, "CUE_V2") {
        read_cue(cue, &mut track);
    }
    Some((format!("{volume}{dir}{file}"), track))
}

fn read_cue(element: Node<'_, '_>, track: &mut TraktorTrack) {
    let Some(start) = element.attribute("START").and_then(parse_number) else {
        return;
    };
    let kind = element
        .attribute("TYPE")
        .and_then(|value| value.parse::<i64>().ok());
    let hot_cue = element
        .attribute("HOTCUE")
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(-1);
    let cue = TraktorCue {
        start_ms: start,
        name: element.attribute("NAME").unwrap_or_default().to_owned(),
        loop_length_ms: (kind == Some(5))
            .then_some(element)
            .and_then(|cue| cue.attribute("LEN"))
            .and_then(parse_number)
            .filter(|length| *length > 0.0),
    };
    match (kind, hot_cue) {
        // The grid cue only sets the anchor, it is not a cue.
        (Some(4), _) => {
            if track.grid_anchor_ms.is_none() {
                track.grid_anchor_ms = Some(start);
            }
        }
        (_, slot) if (0..8).contains(&slot) => {
            track.hot_cues.push((slot as u8, cue));
        }
        _ => track.memory_cues.push(cue),
    }
}

fn parse_node(node: Node<'_, '_>, paths: &HashMap<String, PathBuf>) -> Option<TraktorPlaylistNode> {
    let name = node.attribute("NAME").unwrap_or_default().to_owned();
    match node.attribute("TYPE")? {
        "FOLDER" => Some(TraktorPlaylistNode::Folder {
            name,
            children: node_children(node, paths),
        }),
        "PLAYLIST" => Some(TraktorPlaylistNode::Playlist {
            name,
            track_paths: children(node, "PLAYLIST")
                .flat_map(|playlist| children(playlist, "ENTRY"))
                .flat_map(|entry| children(entry, "PRIMARYKEY"))
                .filter(|key| key.attribute("TYPE") == Some("TRACK"))
                .filter_map(|key| paths.get(key.attribute("KEY")?).cloned())
                .collect(),
        }),
        _ => None,
    }
}

fn node_children(node: Node<'_, '_>, paths: &HashMap<String, PathBuf>) -> Vec<TraktorPlaylistNode> {
    children(node, "SUBNODES")
        .flat_map(|subnodes| children(subnodes, "NODE"))
        .filter_map(|node| parse_node(node, paths))
        .collect()
}

fn parse_number(value: &str) -> Option<f64> {
    let number: f64 = value.trim().parse().ok()?;
    number.is_finite().then_some(number)
}

/// Traktor's `RANKING` in stars of 51 each, from 0 to 255.
fn parse_rating(value: &str) -> u8 {
    value
        .parse::<i64>()
        .map_or(0, |ranking| (ranking / 51).clamp(0, 5)) as u8
}

/// Traktor's `MUSICAL_KEY`: 0 to 11 the majors, 12 to 23 the minors of the same pitch class.
fn parse_key(value: &str) -> Option<(u8, bool)> {
    value
        .parse::<u8>()
        .ok()
        .filter(|value| *value < 24)
        .map(|value| (value % 12, value >= 12))
}
