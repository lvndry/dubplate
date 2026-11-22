//! Formatting and domain editor exports for the set preparation views.

use std::time::Duration;

pub use dubplate_prep::{SetEditor, SetTrackRef};

/// `duration` as `h:mm:ss`, or `m:ss` under an hour.
pub fn running_time(duration: Duration) -> String {
    let seconds = duration.as_secs();
    let hours = seconds / 3600;
    let minutes = seconds / 60 % 60;
    let rest = seconds % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{rest:02}")
    } else {
        format!("{minutes}:{rest:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_times_read_like_a_clock() {
        assert_eq!(running_time(Duration::from_secs(59)), "0:59");
        assert_eq!(running_time(Duration::from_secs(61 * 60 + 5)), "1:01:05");
    }
}
