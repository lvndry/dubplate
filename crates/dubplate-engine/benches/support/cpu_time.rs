//! Timing the audio benches by the CPU time of the measuring thread instead of the wall clock.
//!
//! A machine busy with builds and other programs preempts the bench thread, and wall-clock time
//! counts every pause as if the code had run that long. The thread's own CPU time leaves those
//! pauses out, so before and after numbers stay comparable while the machine is busy. On macOS
//! the thread also asks to run at the user-interactive quality of service, so it stays on the
//! performance cores, where the audio thread itself runs.

use std::time::Duration;

use criterion::Criterion;
use criterion::measurement::{Measurement, ValueFormatter, WallTime};

/// CPU time spent by the calling thread.
pub struct ThreadCpuTime;

impl ThreadCpuTime {
    /// The calling thread's CPU time so far.
    pub fn now() -> Duration {
        let mut time = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: `time` is a valid, writable `timespec` for the call to fill in.
        let result = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut time) };
        assert_eq!(result, 0, "the thread's CPU clock is readable");
        Duration::new(
            u64::try_from(time.tv_sec).unwrap_or_default(),
            u32::try_from(time.tv_nsec).unwrap_or_default(),
        )
    }
}

static WALL_TIME: WallTime = WallTime;

impl Measurement for ThreadCpuTime {
    type Intermediate = Duration;
    type Value = Duration;

    fn start(&self) -> Duration {
        Self::now()
    }

    fn end(&self, started: Duration) -> Duration {
        Self::now().saturating_sub(started)
    }

    fn add(&self, first: &Duration, second: &Duration) -> Duration {
        *first + *second
    }

    fn zero(&self) -> Duration {
        Duration::ZERO
    }

    fn to_f64(&self, value: &Duration) -> f64 {
        value.as_nanos() as f64
    }

    fn formatter(&self) -> &dyn ValueFormatter {
        WALL_TIME.formatter()
    }
}

/// Criterion timing by [`ThreadCpuTime`], with the thread on the performance cores.
pub fn criterion() -> Criterion<ThreadCpuTime> {
    run_on_performance_cores();
    Criterion::default().with_measurement(ThreadCpuTime)
}

#[cfg(target_os = "macos")]
fn run_on_performance_cores() {
    // SAFETY: the call only changes the scheduling class of the calling thread.
    let result = unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE, 0)
    };
    assert_eq!(
        result, 0,
        "the bench thread can ask for the performance cores"
    );
}

#[cfg(not(target_os = "macos"))]
fn run_on_performance_cores() {}
