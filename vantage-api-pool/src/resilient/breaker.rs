//! Circuit breaker with a growing cooldown.
//!
//! `threshold` consecutive failures open the breaker for `cooldown`. When the
//! cooldown ends, exactly one caller gets through as the half-open probe. A
//! failed probe re-opens it for twice the previous cooldown, up to
//! `max_cooldown`; a success closes it and resets the cooldown.
//!
//! The breaker tracks reachability, not correctness: only a `5xx` or a
//! transport error counts toward opening it.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gate {
    /// `probe` carries the grant's epoch when this grant is the half-open
    /// probe; the caller must hold a [`ProbeGuard`] for that attempt.
    Allow { probe: Option<u64> },
    /// Open; how long until the next probe may run.
    OpenFor(Duration),
}

/// What a [`ResilientClient`](crate::ResilientClient)'s breaker is doing right
/// now, for consumers that poll instead of tracking `TransportEvent`s.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BreakerState {
    /// Requests go straight through.
    Closed,
    /// Requests are rejected (or wait) until `until`.
    Open {
        /// When the cooldown ends.
        until: Instant,
    },
    /// The cooldown has elapsed: one caller may take the probe slot.
    HalfOpen,
}

#[derive(Default)]
struct Inner {
    consecutive_failures: usize,
    open_until: Option<Instant>,
    /// The cooldown the NEXT opening will use.
    cooldown: Duration,
    /// The epoch of the probe in flight. Every grant gets a fresh epoch so a
    /// superseded grant's outcome is never mistaken for the current probe's.
    probing: Option<u64>,
    next_epoch: u64,
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
            None => Gate::Allow { probe: None },
            Some(until) => {
                let now = Instant::now();
                if now < until {
                    return Gate::OpenFor(until - now);
                }
                if s.probing.is_some() {
                    return Gate::OpenFor(PROBE_POLL);
                }
                let epoch = s.next_epoch;
                s.next_epoch = s.next_epoch.wrapping_add(1);
                s.probing = Some(epoch);
                Gate::Allow { probe: Some(epoch) }
            }
        }
    }

    pub(crate) fn state(&self) -> BreakerState {
        let s = self.inner.lock().unwrap();
        match s.open_until {
            None => BreakerState::Closed,
            Some(until) if Instant::now() < until => BreakerState::Open { until },
            Some(_) => BreakerState::HalfOpen,
        }
    }

    /// Frees the probe slot held by `epoch`. A stale epoch is ignored, so a
    /// late-dropped guard cannot free the current probe's slot.
    pub(crate) fn release_probe(&self, epoch: u64) {
        let mut s = self.inner.lock().unwrap();
        if s.probing == Some(epoch) {
            s.probing = None;
        }
    }

    /// A `2xx`: closes the breaker and clears the failure run. Returns `true`
    /// when this closed an open breaker.
    pub(crate) fn record_success(&self) -> bool {
        let mut s = self.inner.lock().unwrap();
        let was_open = s.open_until.is_some();
        s.consecutive_failures = 0;
        s.open_until = None;
        s.probing = None;
        s.cooldown = self.base_cooldown;
        was_open
    }

    /// An answer that proves the API is reachable without proving it healthy
    /// (a `4xx`, a `401`). Closes an open breaker but keeps the failure run, so
    /// a server alternating `503` and `404` still reaches the threshold.
    /// Returns `true` when this closed an open breaker.
    pub(crate) fn record_reachable(&self) -> bool {
        let mut s = self.inner.lock().unwrap();
        let was_open = s.open_until.is_some();
        s.open_until = None;
        s.probing = None;
        s.cooldown = self.base_cooldown;
        was_open
    }

    /// A `5xx` or a transport error. Only a failure carrying the epoch of the
    /// probe in flight (`probe`) doubles the cooldown. Returns
    /// `Some(cooldown)` when this failure opened or re-opened the breaker.
    pub(crate) fn record_failure(&self, probe: Option<u64>) -> Option<Duration> {
        let mut s = self.inner.lock().unwrap();
        s.consecutive_failures += 1;
        if probe.is_some() && probe == s.probing {
            s.probing = None;
            s.cooldown = s.cooldown.saturating_mul(2).min(self.max_cooldown);
            let cooldown = s.cooldown;
            s.open_until = Some(deadline(cooldown));
            return Some(cooldown);
        }
        if s.open_until.is_none() && s.consecutive_failures >= self.threshold {
            let cooldown = s.cooldown;
            s.open_until = Some(deadline(cooldown));
            return Some(cooldown);
        }
        None
    }
}

/// `now + cooldown`, or `now` when that instant is not representable on this
/// platform's clock — an unrepresentable deadline is already past.
fn deadline(cooldown: Duration) -> Instant {
    let now = Instant::now();
    now.checked_add(cooldown).unwrap_or(now)
}

/// Holds the probe slot for one attempt. Dropping it, including when the
/// enclosing future is cancelled, frees the slot so an attempt that never
/// records an outcome cannot jam the breaker open.
pub(crate) struct ProbeGuard {
    breaker: Arc<CircuitBreaker>,
    epoch: u64,
}

impl ProbeGuard {
    pub(crate) fn new(breaker: Arc<CircuitBreaker>, epoch: u64) -> Self {
        Self { breaker, epoch }
    }

    pub(crate) fn epoch(&self) -> u64 {
        self.epoch
    }
}

impl Drop for ProbeGuard {
    fn drop(&mut self) {
        self.breaker.release_probe(self.epoch);
    }
}
