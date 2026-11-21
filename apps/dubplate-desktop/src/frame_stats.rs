//! Frame timing and controller latency, printed once a second when `DUBPLATE_FRAME_STATS` is
//! set, to check that the window keeps up with the screen and the controller while tracks play.

use std::time::{Duration, Instant};

const ENVIRONMENT_VARIABLE: &str = "DUBPLATE_FRAME_STATS";
const REPORT_EVERY: Duration = Duration::from_secs(1);

/// The mean and the worst of a series of durations.
#[derive(Debug, Default)]
struct Timing {
    count: u32,
    total: Duration,
    worst: Duration,
}

impl Timing {
    fn record(&mut self, duration: Duration) {
        self.count += 1;
        self.total += duration;
        self.worst = self.worst.max(duration);
    }

    fn describe(&self) -> String {
        let mean = self.total.checked_div(self.count).unwrap_or_default();
        format!(
            "mean {:.2} ms, worst {:.2} ms",
            mean.as_secs_f64() * 1000.0,
            self.worst.as_secs_f64() * 1000.0
        )
    }
}

/// Frame intervals, render times and waveform paint times over the last second.
#[derive(Debug)]
pub struct FrameStats {
    report_started: Instant,
    last_render: Option<Instant>,
    intervals: Timing,
    renders: Timing,
    lane_paints: Timing,
    controls: Timing,
}

impl FrameStats {
    /// Frame statistics when `DUBPLATE_FRAME_STATS` is set.
    pub fn from_environment() -> Option<Self> {
        std::env::var_os(ENVIRONMENT_VARIABLE)?;
        Some(Self {
            report_started: Instant::now(),
            last_render: None,
            intervals: Timing::default(),
            renders: Timing::default(),
            lane_paints: Timing::default(),
            controls: Timing::default(),
        })
    }

    /// Records a render of the window that took `duration`.
    pub fn record_render(&mut self, duration: Duration) {
        let now = Instant::now();
        if let Some(last_render) = self.last_render.replace(now) {
            self.intervals.record(now - last_render);
        }
        self.renders.record(duration);
        if now - self.report_started >= REPORT_EVERY {
            eprintln!(
                "{} frames | interval {} | render {} | waveform paint {} | {} controls, arrival to command {}",
                self.intervals.count,
                self.intervals.describe(),
                self.renders.describe(),
                self.lane_paints.describe(),
                self.controls.count,
                self.controls.describe()
            );
            self.report_started = now;
            self.intervals = Timing::default();
            self.renders = Timing::default();
            self.lane_paints = Timing::default();
            self.controls = Timing::default();
        }
    }

    /// Records a controller message that took `duration` from reaching Dubplate to being
    /// carried out.
    pub fn record_control(&mut self, duration: Duration) {
        self.controls.record(duration);
    }

    /// Records the painting of the zoomed waveforms, which took `duration`.
    pub fn record_lane_paint(&mut self, duration: Duration) {
        self.lane_paints.record(duration);
    }
}
