//! Circuit breaker with a growing cooldown.
//!
//! `threshold` consecutive failures open the breaker for `cooldown`. When the
//! cooldown ends, exactly one caller gets through as the half-open probe. A
//! failed probe re-opens the breaker for twice the previous cooldown, up to
//! `max_cooldown`; a success closes it and resets the cooldown to its base.
//! This is the per-API back-off: nothing above the transport schedules its
//! own.
//!
//! The breaker tracks *reachability*, not correctness. Only a `5xx` or a
//! transport error counts toward opening it; an answer a retry cannot fix
//! proves the API is up and closes it again (see [`record_reachable`]).
//!
//! [`record_reachable`]: CircuitBreaker::record_reachable

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Whether an attempt may proceed right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gate {
    /// `probe` carries the grant's epoch when this grant is the half-open
    /// probe — the caller must hold a [`ProbeGuard`] for the duration of that
    /// attempt so the slot is freed no matter how the attempt ends.
    Allow { probe: Option<u64> },
    /// Open; `Duration` is how long until the next probe may run.
    OpenFor(Duration),
}

/// What a [`ResilientClient`](crate::ResilientClient)'s breaker is doing right
/// now, for consumers that poll instead of tracking `TransportEvent`s.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakerState {
    /// Requests go straight through.
    Closed,
    /// Requests are rejected (or wait) until `until`.
    Open { until: Instant },
    /// The cooldown has elapsed: one caller may take the probe slot.
    HalfOpen,
}

#[derive(Default)]
struct Inner {
    consecutive_failures: usize,
    open_until: Option<Instant>,
    /// The cooldown the NEXT opening will use.
    cooldown: Duration,
    /// The epoch of the probe in flight; other callers keep waiting until it
    /// reports. Every grant gets a fresh epoch so an outcome recorded by a
    /// caller whose grant has already been superseded cannot be mistaken for
    /// the current probe's.
    probing: Option<u64>,
    /// The epoch the next probe grant will carry.
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

    /// Frees the half-open probe slot held by `epoch`, without touching
    /// `open_until` or the cooldown. A stale epoch is ignored, so a guard
    /// dropped after its grant was superseded cannot clear the flag that the
    /// current probe holder just set — which is what lets
    /// [`ProbeGuard::drop`] call this unconditionally.
    pub(crate) fn release_probe(&self, epoch: u64) {
        let mut s = self.inner.lock().unwrap();
        if s.probing == Some(epoch) {
            s.probing = None;
        }
    }

    /// A `2xx`: the API works. Closes the breaker and clears the failure run.
    /// Returns `true` when this closed an open breaker.
    pub(crate) fn record_success(&self) -> bool {
        let mut s = self.inner.lock().unwrap();
        let was_open = s.open_until.is_some();
        s.consecutive_failures = 0;
        s.open_until = None;
        s.probing = None;
        s.cooldown = self.base_cooldown;
        was_open
    }

    /// An answer that proves the API is reachable but says nothing about its
    /// health — a `4xx` a retry cannot fix, or a `401`. Closes an open
    /// breaker, releases the probe and resets the cooldown to its base, but
    /// keeps the failure run: a server alternating `503` and `404` must still
    /// reach the threshold. Returns `true` when this closed an open breaker.
    pub(crate) fn record_reachable(&self) -> bool {
        let mut s = self.inner.lock().unwrap();
        let was_open = s.open_until.is_some();
        s.open_until = None;
        s.probing = None;
        s.cooldown = self.base_cooldown;
        was_open
    }

    /// A `5xx` or a transport error. `probe` is the epoch of the grant this
    /// failure belongs to, when the attempt held the probe slot: only a
    /// failure matching the probe in flight doubles the cooldown; anything
    /// else counts as an ordinary failure.
    ///
    /// Returns `Some(cooldown)` when this failure opened (or re-opened) the
    /// breaker.
    pub(crate) fn record_failure(&self, probe: Option<u64>) -> Option<Duration> {
        let mut s = self.inner.lock().unwrap();
        s.consecutive_failures += 1;
        if probe.is_some() && probe == s.probing {
            // The probe failed: stay open, twice as long.
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

/// Holds the half-open probe slot for one attempt. Dropping it — on a
/// normal return, an early `?`, or the enclosing future being cancelled —
/// releases the slot, so a probe attempt that never calls `record_success`
/// or `record_failure` cannot jam the breaker open forever.
pub(crate) struct ProbeGuard {
    breaker: Arc<CircuitBreaker>,
    epoch: u64,
}

impl ProbeGuard {
    pub(crate) fn new(breaker: Arc<CircuitBreaker>, epoch: u64) -> Self {
        Self { breaker, epoch }
    }

    /// The grant this guard holds, for `record_failure`.
    pub(crate) fn epoch(&self) -> u64 {
        self.epoch
    }
}

impl Drop for ProbeGuard {
    fn drop(&mut self) {
        self.breaker.release_probe(self.epoch);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Take the probe grant `gate()` just handed out, or panic.
    fn probe_epoch(gate: Gate) -> u64 {
        match gate {
            Gate::Allow { probe: Some(e) } => e,
            other => panic!("expected a probe grant, got {other:?}"),
        }
    }

    #[test]
    fn opens_after_threshold_and_doubles_on_failed_probe() {
        let b = CircuitBreaker::new(2, Duration::from_millis(10), Duration::from_millis(25));
        assert_eq!(b.gate(), Gate::Allow { probe: None });
        assert_eq!(b.record_failure(None), None);
        assert_eq!(b.record_failure(None), Some(Duration::from_millis(10)));
        assert!(matches!(b.gate(), Gate::OpenFor(_)));
        std::thread::sleep(Duration::from_millis(12));
        let first = probe_epoch(b.gate());
        assert_eq!(b.gate(), Gate::OpenFor(PROBE_POLL), "one probe at a time");
        assert_eq!(
            b.record_failure(Some(first)),
            Some(Duration::from_millis(20))
        );
        std::thread::sleep(Duration::from_millis(22));
        let second = probe_epoch(b.gate());
        assert_ne!(second, first, "each grant gets a fresh epoch");
        assert_eq!(
            b.record_failure(Some(second)),
            Some(Duration::from_millis(25)),
            "capped"
        );
    }

    #[test]
    fn success_closes_and_resets() {
        let b = CircuitBreaker::new(1, Duration::from_millis(10), Duration::from_millis(40));
        assert_eq!(b.record_failure(None), Some(Duration::from_millis(10)));
        std::thread::sleep(Duration::from_millis(12));
        let epoch = probe_epoch(b.gate());
        assert_eq!(
            b.record_failure(Some(epoch)),
            Some(Duration::from_millis(20))
        );
        std::thread::sleep(Duration::from_millis(22));
        probe_epoch(b.gate());
        assert!(b.record_success());
        assert_eq!(
            b.record_failure(None),
            Some(Duration::from_millis(10)),
            "back to base"
        );
    }

    #[test]
    fn reachable_closes_without_clearing_the_failure_run() {
        let b = CircuitBreaker::new(3, Duration::from_millis(10), Duration::from_millis(10));
        // 503, 404, 503, 404, 503 — the 404s prove the server answers, so
        // they must not reset the run that opens the breaker.
        assert_eq!(b.record_failure(None), None);
        assert!(!b.record_reachable(), "nothing was open to close");
        assert_eq!(b.record_failure(None), None);
        assert!(!b.record_reachable());
        assert_eq!(
            b.record_failure(None),
            Some(Duration::from_millis(10)),
            "the third 5xx reaches the threshold"
        );
        assert!(b.record_reachable(), "a 4xx closes an open breaker");
        assert_eq!(b.state(), BreakerState::Closed);
    }

    #[test]
    fn state_moves_closed_open_half_open() {
        let b = CircuitBreaker::new(1, Duration::from_millis(10), Duration::from_millis(10));
        assert_eq!(b.state(), BreakerState::Closed);
        b.record_failure(None);
        assert!(matches!(b.state(), BreakerState::Open { .. }));
        std::thread::sleep(Duration::from_millis(12));
        assert_eq!(b.state(), BreakerState::HalfOpen);
        assert!(b.record_success());
        assert_eq!(b.state(), BreakerState::Closed);
    }

    #[test]
    fn dropped_probe_guard_releases_the_slot() {
        let b = Arc::new(CircuitBreaker::new(
            1,
            Duration::from_millis(10),
            Duration::from_millis(10),
        ));
        assert_eq!(b.record_failure(None), Some(Duration::from_millis(10)));
        std::thread::sleep(Duration::from_millis(12));
        let epoch = probe_epoch(b.gate());
        let guard = ProbeGuard::new(Arc::clone(&b), epoch);
        assert_eq!(
            b.gate(),
            Gate::OpenFor(PROBE_POLL),
            "the probe is in flight"
        );
        drop(guard);
        assert!(
            matches!(b.gate(), Gate::Allow { probe: Some(_) }),
            "dropping the guard without recording an outcome frees the slot"
        );
    }

    #[test]
    fn a_stale_guard_does_not_free_the_current_probe() {
        let b = Arc::new(CircuitBreaker::new(
            1,
            Duration::from_millis(10),
            Duration::from_millis(10),
        ));
        b.record_failure(None);
        std::thread::sleep(Duration::from_millis(12));
        let stale = ProbeGuard::new(Arc::clone(&b), probe_epoch(b.gate()));
        // The stale grant's outcome is recorded, re-opening the breaker, and
        // a second caller is granted the probe before the guard is dropped.
        b.record_failure(Some(stale.epoch()));
        std::thread::sleep(Duration::from_millis(22));
        let fresh = probe_epoch(b.gate());
        drop(stale);
        assert_eq!(
            b.gate(),
            Gate::OpenFor(PROBE_POLL),
            "the fresh grant still holds the slot"
        );
        assert!(b.record_failure(Some(fresh)).is_some());
    }

    #[test]
    fn a_failure_outside_the_probe_does_not_double_the_cooldown() {
        let b = Arc::new(CircuitBreaker::new(
            1,
            Duration::from_millis(10),
            Duration::from_millis(80),
        ));
        b.record_failure(None);
        std::thread::sleep(Duration::from_millis(12));
        let epoch = probe_epoch(b.gate());
        // A concurrent caller's failure arrives while the probe is in flight.
        assert_eq!(b.record_failure(None), None, "the breaker is already open");
        assert_eq!(
            b.record_failure(Some(epoch)),
            Some(Duration::from_millis(20)),
            "only the probe's own failure doubles the cooldown"
        );
    }
}
