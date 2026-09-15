//! Circuit breaker with a growing cooldown.
//!
//! `threshold` consecutive retryable failures open the breaker for
//! `cooldown`. When the cooldown ends, exactly one caller gets through as the
//! half-open probe. A failed probe re-opens the breaker for twice the
//! previous cooldown, up to `max_cooldown`; a success closes it and resets
//! the cooldown to its base. This is the per-API back-off: nothing above the
//! transport schedules its own.

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Whether an attempt may proceed right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gate {
    Allow,
    /// Open; `Duration` is how long until the next probe may run.
    OpenFor(Duration),
}

#[derive(Default)]
struct Inner {
    consecutive_failures: usize,
    open_until: Option<Instant>,
    /// The cooldown the NEXT opening will use.
    cooldown: Duration,
    /// A probe is in flight; other callers keep waiting until it reports.
    probing: bool,
}

pub(crate) struct CircuitBreaker {
    threshold: usize,
    base_cooldown: Duration,
    max_cooldown: Duration,
    inner: Mutex<Inner>,
}

/// How long a waiter sleeps before re-checking while a probe is in flight.
const PROBE_POLL: Duration = Duration::from_millis(5);

impl CircuitBreaker {
    pub(crate) fn new(threshold: usize, base_cooldown: Duration, max_cooldown: Duration) -> Self {
        Self {
            threshold: threshold.max(1),
            base_cooldown,
            max_cooldown: max_cooldown.max(base_cooldown),
            inner: Mutex::new(Inner {
                cooldown: base_cooldown,
                ..Inner::default()
            }),
        }
    }

    pub(crate) fn gate(&self) -> Gate {
        let mut s = self.inner.lock().unwrap();
        match s.open_until {
            None => Gate::Allow,
            Some(until) => {
                let now = Instant::now();
                if now < until {
                    return Gate::OpenFor(until - now);
                }
                if s.probing {
                    return Gate::OpenFor(PROBE_POLL);
                }
                s.probing = true;
                Gate::Allow
            }
        }
    }

    /// Returns `true` when this success closed an open breaker.
    pub(crate) fn record_success(&self) -> bool {
        let mut s = self.inner.lock().unwrap();
        let was_open = s.open_until.is_some();
        s.consecutive_failures = 0;
        s.open_until = None;
        s.probing = false;
        s.cooldown = self.base_cooldown;
        was_open
    }

    /// Returns `Some(cooldown)` when this failure opened (or re-opened) the
    /// breaker.
    pub(crate) fn record_failure(&self) -> Option<Duration> {
        let mut s = self.inner.lock().unwrap();
        s.consecutive_failures += 1;
        if s.probing {
            // The probe failed: stay open, twice as long.
            s.probing = false;
            s.cooldown = (s.cooldown * 2).min(self.max_cooldown);
            let cooldown = s.cooldown;
            s.open_until = Some(Instant::now() + cooldown);
            return Some(cooldown);
        }
        if s.open_until.is_none() && s.consecutive_failures >= self.threshold {
            let cooldown = s.cooldown;
            s.open_until = Some(Instant::now() + cooldown);
            return Some(cooldown);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_after_threshold_and_doubles_on_failed_probe() {
        let b = CircuitBreaker::new(2, Duration::from_millis(10), Duration::from_millis(25));
        assert_eq!(b.gate(), Gate::Allow);
        assert_eq!(b.record_failure(), None);
        assert_eq!(b.record_failure(), Some(Duration::from_millis(10)));
        assert!(matches!(b.gate(), Gate::OpenFor(_)));
        std::thread::sleep(Duration::from_millis(12));
        assert_eq!(b.gate(), Gate::Allow, "half-open probe");
        assert_eq!(b.gate(), Gate::OpenFor(PROBE_POLL), "one probe at a time");
        assert_eq!(b.record_failure(), Some(Duration::from_millis(20)));
        std::thread::sleep(Duration::from_millis(22));
        assert_eq!(b.gate(), Gate::Allow);
        assert_eq!(b.record_failure(), Some(Duration::from_millis(25)), "capped");
    }

    #[test]
    fn success_closes_and_resets() {
        let b = CircuitBreaker::new(1, Duration::from_millis(10), Duration::from_millis(40));
        assert_eq!(b.record_failure(), Some(Duration::from_millis(10)));
        std::thread::sleep(Duration::from_millis(12));
        assert_eq!(b.gate(), Gate::Allow);
        assert_eq!(b.record_failure(), Some(Duration::from_millis(20)));
        std::thread::sleep(Duration::from_millis(22));
        assert_eq!(b.gate(), Gate::Allow);
        assert!(b.record_success());
        assert_eq!(b.record_failure(), Some(Duration::from_millis(10)), "back to base");
    }
}
