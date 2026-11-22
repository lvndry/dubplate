/// What is wrong with a track for a set, in the order the ready check reports them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadyProblem {
    /// The audio file is not where rekordbox says it is.
    FileMissing,
    /// The beat grid has not been confirmed by the DJ.
    GridUnconfirmed,
    /// There is no cue marking where the track comes in.
    NoMixInCue,
    /// There is no cue marking where the track hands over.
    NoMixOutCue,
    /// The track's key is not known.
    KeyUnknown,
}

impl ReadyProblem {
    /// The word the Prepare screen shows for this problem.
    pub fn label(self) -> &'static str {
        match self {
            Self::FileMissing => "File missing",
            Self::GridUnconfirmed => "Grid unconfirmed",
            Self::NoMixInCue => "No mix-in cue",
            Self::NoMixOutCue => "No mix-out cue",
            Self::KeyUnknown => "Key unknown",
        }
    }

    /// How urgent this is: red only for a missing file, amber for everything else.
    pub fn severity(self) -> Severity {
        match self {
            Self::FileMissing => Severity::Blocking,
            _ => Severity::NeedsAttention,
        }
    }
}

/// How urgent a ready problem is on the Prepare screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Red: the track cannot be mixed until this is fixed.
    Blocking,
    /// Amber: the DJ should look at this before the set.
    NeedsAttention,
}

/// What set preparation knows about one track's readiness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrackReadiness {
    /// The audio file was found at its rekordbox path.
    pub file_found: bool,
    /// The DJ confirmed the beat grid.
    pub grid_confirmed: bool,
    /// A mix-in cue is set.
    pub has_mix_in_cue: bool,
    /// A mix-out cue is set.
    pub has_mix_out_cue: bool,
    /// The track's key is known.
    pub key_known: bool,
}

/// Every problem of one track, in the order the ready check reports them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadyReport {
    /// The problems, from the most to the least urgent.
    pub problems: Vec<ReadyProblem>,
}

/// Checks a track's readiness and reports every problem, starting with the file
/// and ending with the key.
pub fn ready_report(readiness: &TrackReadiness) -> ReadyReport {
    let mut problems = Vec::new();
    if !readiness.file_found {
        problems.push(ReadyProblem::FileMissing);
    }
    if !readiness.grid_confirmed {
        problems.push(ReadyProblem::GridUnconfirmed);
    }
    if !readiness.has_mix_in_cue {
        problems.push(ReadyProblem::NoMixInCue);
    }
    if !readiness.has_mix_out_cue {
        problems.push(ReadyProblem::NoMixOutCue);
    }
    if !readiness.key_known {
        problems.push(ReadyProblem::KeyUnknown);
    }
    ReadyReport { problems }
}

impl ReadyReport {
    /// Whether the track needs nothing more before it can be mixed.
    pub fn is_ready(&self) -> bool {
        self.problems.is_empty()
    }

    /// The badge the Prepare screen shows: "Ready" when there is nothing to
    /// fix, otherwise the first problem and how many more there are, such as
    /// "File missing, and 3 more".
    pub fn badge(&self) -> String {
        let Some(first_problem) = self.problems.first() else {
            return "Ready".to_owned();
        };
        match self.problems.len() - 1 {
            0 => first_problem.label().to_owned(),
            remaining => format!("{}, and {remaining} more", first_problem.label()),
        }
    }

    /// Every problem, one per line, from the most to the least urgent, for the
    /// badge's tooltip. Empty when the track is ready.
    pub fn details(&self) -> String {
        self.problems
            .iter()
            .map(|problem| problem.label())
            .collect::<Vec<_>>()
            .join("\n")
    }
}
