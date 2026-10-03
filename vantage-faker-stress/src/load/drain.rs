//! Drain-time probe: how long a subscriber takes to consume the backlog it
//! had at a sample tick.
//!
//! At a tick with a backlog, the probe arms with a target of "events seen
//! so far + backlog". The subscriber advances the probe on every receive,
//! and once it has seen the target the elapsed time is recorded. The next
//! tick reports that time, so a drain is reported one tick late. A probe
//! still armed at a tick reports the time elapsed so far, so a consumer that
//! never catches up shows growing lag.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

const DISARMED: u64 = u64::MAX;

#[derive(Default)]
struct State {
    armed: Option<Instant>,
    last_ms: f64,
}

pub struct Drain {
    /// Events received or skipped (`Lagged`) by the subscriber.
    seen: AtomicU64,
    /// `seen` value that completes the armed probe; `DISARMED` otherwise.
    target: AtomicU64,
    state: Mutex<State>,
}

impl Default for Drain {
    fn default() -> Self {
        Self {
            seen: AtomicU64::new(0),
            target: AtomicU64::new(DISARMED),
            state: Mutex::default(),
        }
    }
}

impl Drain {
    /// Count `n` events as drained; called by the subscriber task.
    pub fn advance(&self, n: u64) {
        let seen = self.seen.fetch_add(n, Ordering::Relaxed) + n;
        if seen < self.target.load(Ordering::Relaxed) {
            return;
        }
        let mut state = self.state.lock().unwrap();
        if let Some(t0) = state.armed
            && seen >= self.target.load(Ordering::Relaxed)
        {
            state.last_ms = ms_since(t0);
            state.armed = None;
            self.target.store(DISARMED, Ordering::Relaxed);
        }
    }

    /// Lag for this tick and the backlog, then arm a probe for the backlog
    /// if none is armed. `backlog` is read after `seen`, so events drained
    /// in between can only lower the target, never leave it out of reach.
    pub fn tick(&self, backlog: impl FnOnce() -> usize) -> (f64, usize) {
        let mut state = self.state.lock().unwrap();
        let seen = self.seen.load(Ordering::Relaxed);
        let backlog = backlog();
        if let Some(t0) = state.armed {
            return (ms_since(t0), backlog);
        }
        let lag = state.last_ms;
        if backlog > 0 {
            state.armed = Some(Instant::now());
            self.target.store(seen + backlog as u64, Ordering::Relaxed);
        } else {
            state.last_ms = 0.0;
        }
        (lag, backlog)
    }
}

fn ms_since(t0: Instant) -> f64 {
    t0.elapsed().as_secs_f64() * 1000.0
}

#[cfg(test)]
mod tests {
    use std::thread::sleep;
    use std::time::Duration;

    use super::*;

    fn lag(d: &Drain, backlog: usize) -> f64 {
        d.tick(|| backlog).0
    }

    #[test]
    fn no_backlog_reads_zero() {
        let d = Drain::default();
        assert_eq!(d.tick(|| 0), (0.0, 0));
        assert_eq!(lag(&d, 0), 0.0);
    }

    #[test]
    fn drain_time_is_reported_next_tick() {
        let d = Drain::default();
        assert_eq!(d.tick(|| 3), (0.0, 3));
        sleep(Duration::from_millis(50));
        d.advance(2);
        d.advance(1);
        sleep(Duration::from_millis(200));
        let drained = lag(&d, 0);
        assert!((40.0..200.0).contains(&drained), "{drained}");
        assert_eq!(lag(&d, 0), 0.0, "an empty tick resets the reading");
    }

    #[test]
    fn skipped_events_count_as_drained() {
        let d = Drain::default();
        lag(&d, 10);
        d.advance(10);
        assert!(lag(&d, 0) < 100.0);
    }

    #[test]
    fn stuck_consumer_shows_growing_lag() {
        let d = Drain::default();
        lag(&d, 5);
        d.advance(4);
        sleep(Duration::from_millis(100));
        let first = lag(&d, 1);
        sleep(Duration::from_millis(100));
        let second = lag(&d, 1);
        assert!(first >= 100.0, "{first}");
        assert!(second >= first + 100.0, "{first} then {second}");
    }
}
