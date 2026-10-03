//! One row of measurements a second: engine counters, process cost and
//! event flow.

use std::time::Instant;

use serde::{Deserialize, Serialize};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use vantage_faker::SimStats;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sample {
    /// Seconds since the sampler was created.
    pub t: f64,
    pub live: usize,
    pub spawned: u64,
    pub ended: u64,
    pub errored: u64,
    pub threads: usize,
    /// Percent of one core; 400 means four cores busy.
    pub cpu_pct: f64,
    pub rss_mb: f64,
    pub writes_per_s: f64,
    pub events_per_s: f64,
    /// Events dropped so far because a subscriber fell behind.
    pub lagged: u64,
    /// Slowest table's drain time for the backlog seen one tick earlier, or
    /// time elapsed so far if that backlog is still not drained.
    pub lag_ms: f64,
}

/// Event totals summed over every table's subscriber.
#[derive(Clone, Copy, Debug, Default)]
pub struct EventTotals {
    pub delivered: u64,
    pub lagged: u64,
    /// Events queued in the broadcast channels, not yet received.
    pub backlog: usize,
    /// Largest drain-probe reading over the tables (see `load::drain`).
    pub lag_ms: f64,
}

pub struct Sampler {
    sys: System,
    pid: Pid,
    started: Instant,
    last: Option<(Instant, u64, u64)>,
}

impl Sampler {
    pub fn new() -> Self {
        let mut s = Self {
            sys: System::new(),
            pid: sysinfo::get_current_pid().unwrap_or_else(|_| Pid::from_u32(std::process::id())),
            started: Instant::now(),
            last: None,
        };
        s.refresh();
        s
    }

    fn refresh(&mut self) {
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[self.pid]),
            true,
            ProcessRefreshKind::nothing().with_cpu().with_memory(),
        );
    }

    /// CPU percent since the previous refresh.
    pub fn cpu_now(&mut self) -> f64 {
        self.refresh();
        self.sys
            .process(self.pid)
            .map_or(0.0, |p| f64::from(p.cpu_usage()))
    }

    pub fn sample(&mut self, stats: SimStats, events: EventTotals) -> Sample {
        let cpu_pct = self.cpu_now();
        let rss_mb = self
            .sys
            .process(self.pid)
            .map_or(0.0, |p| p.memory() as f64 / (1024.0 * 1024.0));
        let now = Instant::now();
        let (writes_per_s, events_per_s) = match self.last {
            Some((at, writes, delivered)) => {
                let dt = now.duration_since(at).as_secs_f64().max(1e-3);
                (
                    stats.writes.saturating_sub(writes) as f64 / dt,
                    events.delivered.saturating_sub(delivered) as f64 / dt,
                )
            }
            None => (0.0, 0.0),
        };
        self.last = Some((now, stats.writes, events.delivered));
        Sample {
            t: now.duration_since(self.started).as_secs_f64(),
            live: stats.live,
            spawned: stats.spawned,
            ended: stats.ended,
            errored: stats.errored,
            threads: crate::threads::process_threads(),
            cpu_pct,
            rss_mb,
            writes_per_s,
            events_per_s,
            lagged: events.lagged,
            lag_ms: events.lag_ms,
        }
    }
}

impl Default for Sampler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vantage_faker::SimStats;

    fn writes(n: u64) -> SimStats {
        let mut stats = SimStats::default();
        stats.writes = n;
        stats
    }

    #[test]
    fn thread_count_sees_this_process() {
        assert!(crate::threads::process_threads() >= 1);
    }

    #[test]
    fn rates_are_deltas_per_second() {
        let mut s = Sampler::new();
        let first = s.sample(
            writes(10),
            EventTotals {
                delivered: 5,
                ..Default::default()
            },
        );
        assert_eq!(first.writes_per_s, 0.0, "first sample has no previous one");
        std::thread::sleep(std::time::Duration::from_millis(500));
        let second = s.sample(
            writes(60),
            EventTotals {
                delivered: 55,
                lagged: 2,
                backlog: 10,
                lag_ms: 42.0,
            },
        );
        assert!(
            (80.0..=120.0).contains(&second.writes_per_s),
            "{}",
            second.writes_per_s
        );
        assert!((80.0..=120.0).contains(&second.events_per_s));
        assert_eq!(second.lagged, 2);
        assert_eq!(second.lag_ms, 42.0, "lag is the subscribers' probe reading");
        assert!(second.rss_mb > 0.0);
    }

    #[test]
    fn panic_counter_counts() {
        crate::panics::install();
        let before = crate::panics::count();
        let _ = std::thread::spawn(|| panic!("counted")).join();
        assert_eq!(crate::panics::count(), before + 1);
    }
}
