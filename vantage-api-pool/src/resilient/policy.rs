//! Retry and back-off knobs.

use std::time::Duration;

/// Bounded retry: exponential from `base_backoff` (doubling per attempt),
/// capped at `max_backoff`, with jitter added before each sleep.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub max_retries: usize,
    pub base_backoff: Duration,
    pub max_backoff: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 4,
            base_backoff: Duration::from_millis(50),
            max_backoff: Duration::from_secs(10),
        }
    }
}

impl RetryPolicy {
    pub(crate) fn backoff(&self, attempt: usize) -> Duration {
        exponential(self.base_backoff, self.max_backoff, attempt)
    }
}

/// `base × 2^attempt`, capped.
pub(crate) fn exponential(base: Duration, max: Duration, attempt: usize) -> Duration {
    let factor = 2u32.saturating_pow(attempt.min(31) as u32);
    base.saturating_mul(factor).min(max)
}

/// Add up to +25% jitter so retrying clients don't synchronize.
pub(crate) fn with_jitter(d: Duration) -> Duration {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|t| t.subsec_nanos())
        .unwrap_or(0);
    let frac = (nanos % 250) as f64 / 1000.0;
    d + d.mul_f64(frac)
}

/// How many times, and how long apart, a failed attempt is repeated.
#[derive(Debug, Clone)]
pub enum RetryMode {
    /// One attempt. For work nobody is waiting on.
    None,
    /// The classic bounded retry.
    Bounded(RetryPolicy),
    /// Retry until the caller drops the future; exponential from `base`
    /// capped at `max`. For work someone is waiting on.
    UntilCancelled { base: Duration, max: Duration },
}

/// What an attempt does while the circuit breaker is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakerMode {
    /// Return `ErrorKind::BreakerOpen` at once.
    FailFast,
    /// Sleep until the cooldown ends and take the half-open probe slot.
    WaitForProbe,
}

/// The policy for one call.
#[derive(Debug, Clone)]
pub struct CallPolicy {
    pub retry: RetryMode,
    pub breaker: BreakerMode,
}

impl CallPolicy {
    /// One attempt, fail fast. Polls, refreshes, hydration.
    pub fn background() -> Self {
        Self {
            retry: RetryMode::None,
            breaker: BreakerMode::FailFast,
        }
    }

    /// Retry until cancelled (250 ms doubling to 10 s), wait for the probe.
    /// Cold loads, uncached viewports, writes.
    pub fn essential() -> Self {
        Self {
            retry: RetryMode::UntilCancelled {
                base: Duration::from_millis(250),
                max: Duration::from_secs(10),
            },
            breaker: BreakerMode::FailFast,
        }
        .wait_for_probe()
    }

    /// Bounded retry, fail fast — what `execute` does.
    pub fn bounded(policy: RetryPolicy) -> Self {
        Self {
            retry: RetryMode::Bounded(policy),
            breaker: BreakerMode::FailFast,
        }
    }

    pub fn wait_for_probe(mut self) -> Self {
        self.breaker = BreakerMode::WaitForProbe;
        self
    }

    /// The sleep before retry number `attempt` (0-based count of retries so
    /// far), or `None` when this mode has no retry left.
    pub(crate) fn next_backoff(&self, attempt: usize) -> Option<Duration> {
        match &self.retry {
            RetryMode::None => None,
            RetryMode::Bounded(p) => (attempt < p.max_retries).then(|| p.backoff(attempt)),
            RetryMode::UntilCancelled { base, max } => Some(exponential(*base, *max, attempt)),
        }
    }
}

/// Statuses a retry may fix. Everything else in 4xx is final.
pub(crate) fn is_retryable_status(status: u16) -> bool {
    status == 408 || status == 429 || (500..600).contains(&status)
}
