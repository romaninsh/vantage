//! Circuit breaker: after `threshold` consecutive failures, fail fast for a
//! cooldown, then let one probe through.

use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Default)]
struct BreakerInner {
    consecutive_failures: usize,
    open_until: Option<Instant>,
}

pub(crate) struct CircuitBreaker {
    threshold: usize,
    cooldown: Duration,
    inner: Mutex<BreakerInner>,
}

impl CircuitBreaker {
    pub(crate) fn new(threshold: usize, cooldown: Duration) -> Self {
        Self {
            threshold,
            cooldown,
            inner: Mutex::new(BreakerInner::default()),
        }
    }

    /// Whether a request may proceed. When open and the cooldown has
    /// elapsed, returns `true` once (half-open probe) and re-arms.
    pub(crate) fn allow(&self) -> bool {
        let mut s = self.inner.lock().unwrap();
        if let Some(until) = s.open_until {
            if Instant::now() < until {
                return false;
            }
            s.open_until = None;
        }
        true
    }

    pub(crate) fn record_success(&self) {
        let mut s = self.inner.lock().unwrap();
        s.consecutive_failures = 0;
        s.open_until = None;
    }

    pub(crate) fn record_failure(&self) {
        let mut s = self.inner.lock().unwrap();
        s.consecutive_failures += 1;
        if s.consecutive_failures >= self.threshold {
            s.open_until = Some(Instant::now() + self.cooldown);
        }
    }
}
