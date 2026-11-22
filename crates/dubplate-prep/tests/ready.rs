//! The per-track ready check: what is wrong, in which order, and how the badge says it.

use dubplate_prep::{ReadyProblem, Severity, TrackReadiness, ready_report};

fn readiness(
    file_found: bool,
    grid_confirmed: bool,
    has_mix_in_cue: bool,
    has_mix_out_cue: bool,
    key_known: bool,
) -> TrackReadiness {
    TrackReadiness {
        file_found,
        grid_confirmed,
        has_mix_in_cue,
        has_mix_out_cue,
        key_known,
    }
}

fn complete_readiness() -> TrackReadiness {
    readiness(true, true, true, true, true)
}

#[test]
fn a_complete_track_is_ready() {
    let report = ready_report(&complete_readiness());

    assert!(report.is_ready());
    assert_eq!(report.badge(), "Ready");
}

#[test]
fn problems_are_reported_in_the_ready_check_order() {
    let report = ready_report(&readiness(false, false, false, false, false));

    assert_eq!(
        report.problems,
        vec![
            ReadyProblem::FileMissing,
            ReadyProblem::GridUnconfirmed,
            ReadyProblem::NoMixInCue,
            ReadyProblem::NoMixOutCue,
            ReadyProblem::KeyUnknown,
        ]
    );
    assert!(!report.is_ready());
}

#[test]
fn the_badge_names_the_first_problem_only() {
    let cases = [
        (readiness(true, false, true, true, true), "Grid unconfirmed"),
        (readiness(true, true, false, true, true), "No mix-in cue"),
        (readiness(true, true, true, false, true), "No mix-out cue"),
        (readiness(true, true, true, true, false), "Key unknown"),
        (readiness(false, true, true, true, true), "File missing"),
    ];

    for (readiness_case, expected_badge) in cases {
        let report = ready_report(&readiness_case);
        assert_eq!(report.badge(), expected_badge);
    }
}

#[test]
fn the_badge_counts_the_remaining_problems() {
    let cases = [
        (
            readiness(false, false, false, false, false),
            "File missing, and 4 more",
        ),
        (readiness(true, false, true, true, true), "Grid unconfirmed"),
        (
            readiness(true, false, false, false, false),
            "Grid unconfirmed, and 3 more",
        ),
        (
            readiness(true, false, false, true, true),
            "Grid unconfirmed, and 1 more",
        ),
        (
            readiness(true, true, true, false, false),
            "No mix-out cue, and 1 more",
        ),
    ];

    for (readiness_case, expected_badge) in cases {
        let report = ready_report(&readiness_case);
        assert_eq!(report.badge(), expected_badge);
    }
}

#[test]
fn the_details_list_every_problem_in_order() {
    let report = ready_report(&readiness(false, false, false, true, true));
    assert_eq!(
        report.details(),
        "File missing\nGrid unconfirmed\nNo mix-in cue"
    );
    assert_eq!(
        ready_report(&readiness(true, true, true, true, true)).details(),
        ""
    );
}

#[test]
fn only_a_missing_file_is_blocking() {
    let problems = [
        ReadyProblem::FileMissing,
        ReadyProblem::GridUnconfirmed,
        ReadyProblem::NoMixInCue,
        ReadyProblem::NoMixOutCue,
        ReadyProblem::KeyUnknown,
    ];

    for problem in problems {
        let expected_severity = match problem {
            ReadyProblem::FileMissing => Severity::Blocking,
            _ => Severity::NeedsAttention,
        };
        assert_eq!(problem.severity(), expected_severity);
    }
}

#[test]
fn the_labels_are_the_words_the_prepare_screen_shows() {
    assert_eq!(ReadyProblem::FileMissing.label(), "File missing");
    assert_eq!(ReadyProblem::GridUnconfirmed.label(), "Grid unconfirmed");
    assert_eq!(ReadyProblem::NoMixInCue.label(), "No mix-in cue");
    assert_eq!(ReadyProblem::NoMixOutCue.label(), "No mix-out cue");
    assert_eq!(ReadyProblem::KeyUnknown.label(), "Key unknown");
}
