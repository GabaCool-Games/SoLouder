//! Render-time statistics and real-time pacing.

use std::time::{Duration, Instant};

/// Statistics of a run of render times.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Timing {
    /// Renders timed.
    pub renders: usize,
    /// Mean render time.
    pub mean: Duration,
    /// 99th percentile (nearest rank).
    pub p99: Duration,
    /// Longest render.
    pub max: Duration,
}

/// Summarizes `times`; all zero when it is empty.
pub fn summarize(mut times: Vec<Duration>) -> Timing {
    let renders = times.len();
    if renders == 0 {
        return Timing {
            renders,
            mean: Duration::ZERO,
            p99: Duration::ZERO,
            max: Duration::ZERO,
        };
    }
    times.sort_unstable();
    let total: Duration = times.iter().sum();
    let rank = (renders * 99).div_ceil(100).max(1) - 1;
    Timing {
        renders,
        mean: total / renders as u32,
        p99: times[rank],
        max: times[renders - 1],
    }
}

/// The time a device callback has to render `frames` frames at `rate`.
pub fn budget(frames: u32, rate: u32) -> Duration {
    Duration::from_secs_f64(f64::from(frames) / f64::from(rate))
}

/// Waits until `deadline`: sleeps while more than 2 ms remain, then yields.
pub fn wait_until(deadline: Instant) {
    loop {
        let now = Instant::now();
        let Some(left) = deadline.checked_duration_since(now) else {
            return;
        };
        if left > Duration::from_millis(2) {
            std::thread::sleep(left - Duration::from_millis(1));
        } else {
            std::thread::yield_now();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statistics_use_the_nearest_rank() {
        let times = (1..=200).rev().map(Duration::from_micros).collect();
        let timing = summarize(times);
        assert_eq!(timing.renders, 200);
        assert_eq!(timing.p99, Duration::from_micros(198));
        assert_eq!(timing.max, Duration::from_micros(200));
        assert_eq!(timing.mean, Duration::from_nanos(100_500));
        assert_eq!(summarize(Vec::new()).max, Duration::ZERO);
        assert_eq!(summarize(vec![Duration::from_micros(5)]).p99.as_micros(), 5);
    }

    #[test]
    fn the_budget_is_the_buffer_length() {
        assert_eq!(budget(480, 48_000), Duration::from_millis(10));
    }
}
