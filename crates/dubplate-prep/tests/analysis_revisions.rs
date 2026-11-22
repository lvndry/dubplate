//! Caller revisions survive queue coalescing and cached replies.

use dubplate_prep::{AnalysisCache, AnalysisQueue, AudioAnalysis, Priority};
use std::{fs, time::Duration};

#[test]
fn coalesced_requests_echo_the_latest_revision_and_keep_earliest_priority() {
    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("first.wav");
    let second = directory.path().join("second.wav");
    let cache_path = directory.path().join("cache.db");
    let analysis = AudioAnalysis {
        duration_seconds: 1.0,
        bpm: None,
        beat_grid: None,
        key: None,
        energy: None,
    };
    let mut cache = AnalysisCache::open(&cache_path).unwrap();
    for path in [&first, &second] {
        fs::write(path, b"fingerprint fixture; must be answered from cache").unwrap();
        cache.put(path, &analysis).unwrap();
    }
    drop(cache);
    let queue = AnalysisQueue::start(cache_path).unwrap();
    queue.request_all_versioned([
        (first.clone(), Priority::Set, 1),
        (second.clone(), Priority::Shortlist, 3),
        (first.clone(), Priority::Library, 2),
    ]);
    let answer = queue
        .results()
        .recv_timeout(Duration::from_secs(10))
        .unwrap();
    assert_eq!(answer.path, first);
    assert_eq!(answer.revision, 2);
    assert!(answer.outcome.is_ok());
    let answer = queue
        .results()
        .recv_timeout(Duration::from_secs(10))
        .unwrap();
    assert_eq!(answer.path, second);
    assert_eq!(answer.revision, 3);
    assert!(answer.outcome.is_ok());
    assert_eq!(queue.decoded_count(), 0);
}
