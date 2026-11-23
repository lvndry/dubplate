use euphoria_core::key::Key;

use crate::track::PrepTrack;

/// One track in a DJ's ordered set, with the facts the seam warnings read.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SetTrack<'set> {
    /// The track.
    pub track: &'set PrepTrack,
    /// The DJ tagged vocals coming in at the start of the track.
    pub vocals_at_start: bool,
    /// The DJ tagged vocals going out at the end of the track.
    pub vocals_at_end: bool,
    /// The DJ marked the seam after this track Free: that seam never warns.
    pub free_into_next: bool,
}

/// What the set does not like, on one seam or one track.
#[derive(Debug, Clone, PartialEq)]
pub enum WarningKind {
    /// Both keys are known and the move between them has no name in harmonic mixing.
    KeyClash {
        /// The key the DJ is leaving.
        outgoing: Key,
        /// The key the DJ is arriving in.
        incoming: Key,
    },
    /// Both tempos are known and the incoming one sits more than 6.0 percent
    /// away from the outgoing one, beyond the FLX4's default pitch range.
    TempoGap {
        /// Signed percent: incoming minus outgoing, over outgoing, times 100.
        percent: f32,
    },
    /// Vocals tagged out at the end of the outgoing track meet vocals tagged in at
    /// the start of the incoming one.
    VocalsOverlap,
    /// Both energies are known and the incoming one is lower by 3.0 or more.
    EnergyDrop {
        /// How much lower the incoming energy is, positive.
        by: f32,
    },
    /// This track's file already appears earlier in the set.
    Duplicate {
        /// The 0-based index of the earlier entry with the same file.
        first_position: usize,
    },
    /// An earlier track is another version of this one: same artist and base
    /// title, a different file.
    SimilarVersion {
        /// The 0-based index of the earliest earlier entry that is another version.
        first_position: usize,
    },
}

impl WarningKind {
    /// The words the Prepare screen shows for this warning.
    pub fn label(&self) -> String {
        match self {
            Self::KeyClash { outgoing, incoming } => {
                format!(
                    "Key clash: {} to {}",
                    outgoing.camelot(),
                    incoming.camelot()
                )
            }
            Self::TempoGap { percent } => {
                format!("Tempo gap {percent:+.1}%, needs a long blend")
            }
            Self::VocalsOverlap => "Vocals overlap".to_owned(),
            Self::EnergyDrop { by } => format!("Energy drops by {by:.0}"),
            Self::Duplicate { first_position } => {
                format!("Already in the set at {}", first_position + 1)
            }
            Self::SimilarVersion { first_position } => {
                format!("Another version is at {}", first_position + 1)
            }
        }
    }
}

/// Where a warning sits: on a seam between two entries, or on one entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarningPosition {
    /// The seam between two entries.
    Seam {
        /// The 0-based index of the outgoing entry.
        after: usize,
    },
    /// A single entry in the set.
    Track {
        /// The 0-based index of the warned entry.
        index: usize,
    },
}

/// One thing the set does not like, and where.
#[derive(Debug, Clone, PartialEq)]
pub struct SetWarning {
    /// Where the warning sits.
    pub position: WarningPosition,
    /// What the warning is.
    pub kind: WarningKind,
}

/// Every warning in a set: the seams in set order, each checked as key clash,
/// tempo gap, vocals overlap, energy drop, then the tracks in set order for
/// duplicates and similar versions. A seam the DJ marked Free never warns, and a
/// track that is a duplicate is not also reported as a similar version. Fewer
/// than two entries gives no seam warnings.
pub fn set_warnings(entries: &[SetTrack<'_>]) -> Vec<SetWarning> {
    warnings_for_slots(entries.len(), |index| {
        let entry = entries[index];
        Some((entry, entry.track.energy))
    })
}

/// Keeps unresolved tracks in their positions so warnings refer to the main set order.
pub(crate) fn warnings_for_slots<'track>(
    length: usize,
    entry: impl Fn(usize) -> Option<(SetTrack<'track>, Option<f32>)>,
) -> Vec<SetWarning> {
    let mut warnings = Vec::new();
    for outgoing_index in 0..length.saturating_sub(1) {
        let (Some((outgoing, outgoing_energy)), Some((incoming, incoming_energy))) =
            (entry(outgoing_index), entry(outgoing_index + 1))
        else {
            continue;
        };
        if outgoing.free_into_next {
            continue;
        }
        let position = WarningPosition::Seam {
            after: outgoing_index,
        };
        if let (Some(outgoing_key), Some(incoming_key)) = (outgoing.track.key, incoming.track.key)
            && outgoing_key.relation_to(&incoming_key).is_none()
        {
            warnings.push(SetWarning {
                position,
                kind: WarningKind::KeyClash {
                    outgoing: outgoing_key,
                    incoming: incoming_key,
                },
            });
        }
        if let (Some(outgoing_bpm), Some(incoming_bpm)) = (outgoing.track.bpm, incoming.track.bpm) {
            let tempo_change_percent = (incoming_bpm - outgoing_bpm) / outgoing_bpm * 100.0;
            if tempo_change_percent.abs() > 6.0 {
                warnings.push(SetWarning {
                    position,
                    kind: WarningKind::TempoGap {
                        percent: tempo_change_percent,
                    },
                });
            }
        }
        if outgoing.vocals_at_end && incoming.vocals_at_start {
            warnings.push(SetWarning {
                position,
                kind: WarningKind::VocalsOverlap,
            });
        }
        if let (Some(outgoing_energy), Some(incoming_energy)) = (outgoing_energy, incoming_energy) {
            let energy_drop = outgoing_energy - incoming_energy;
            if energy_drop >= 3.0 {
                warnings.push(SetWarning {
                    position,
                    kind: WarningKind::EnergyDrop { by: energy_drop },
                });
            }
        }
    }
    for later_index in 0..length {
        let Some((later, _)) = entry(later_index) else {
            continue;
        };
        let earlier = || {
            (0..later_index).filter_map(|index| {
                let (earlier, _) = entry(index)?;
                Some((index, earlier.track))
            })
        };
        let kind = earlier()
            .find(|(_, track)| track.track_path == later.track.track_path)
            .map(|(first_position, _)| WarningKind::Duplicate { first_position })
            .or_else(|| {
                earlier()
                    .find(|(_, track)| is_similar_version(track, later.track))
                    .map(|(first_position, _)| WarningKind::SimilarVersion { first_position })
            });
        if let Some(kind) = kind {
            warnings.push(SetWarning {
                position: WarningPosition::Track { index: later_index },
                kind,
            });
        }
    }
    warnings
}

/// Whether two tracks are versions of the same song: a different file,
/// the same artist and the same base title, compared case-insensitively after
/// trimming. Empty base titles never match.
fn is_similar_version(earlier: &PrepTrack, later: &PrepTrack) -> bool {
    if earlier.track_path == later.track_path {
        return false;
    }
    let earlier_base_title = base_title(&earlier.title).trim().to_lowercase();
    let later_base_title = base_title(&later.title).trim().to_lowercase();
    if earlier_base_title.is_empty() || later_base_title.is_empty() {
        return false;
    }
    earlier
        .artist
        .trim()
        .eq_ignore_ascii_case(later.artist.trim())
        && earlier_base_title == later_base_title
}

/// The title with any text in parentheses or square brackets dropped, so
/// "Jerusalema (Extended Mix)" and "Jerusalema [Radio Edit]" both read as
/// "Jerusalema".
fn base_title(title: &str) -> String {
    let mut base = String::with_capacity(title.len());
    let mut characters = title.chars();
    while let Some(character) = characters.next() {
        let Some(closing) = closing_bracket(character) else {
            base.push(character);
            continue;
        };
        for next in characters.by_ref() {
            if next == closing {
                break;
            }
        }
    }
    base
}

/// The character that closes the bracket `character` opens.
fn closing_bracket(character: char) -> Option<char> {
    match character {
        '(' => Some(')'),
        '[' => Some(']'),
        _ => None,
    }
}
