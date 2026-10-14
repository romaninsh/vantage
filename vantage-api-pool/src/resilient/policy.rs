//! Retry and back-off knobs.

use std::time::Duration;

/// The shortest sleep any retry mode may schedule. A policy configured with a
/// zero base (or a zero ceiling) would otherwise spin.
const MIN_BACKOFF: Duration = Duration::from_millis(1);

/// Bounded retry: exponential from `base_backoff` (doubling per attempt),
/// capped at `max_backoff`, with jitter added before each sleep.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RetryPolicy {
    /// Retries after the first attempt.
    pub max_retries: usize,
    /// The back-off before the first retry; doubles for each one after.
    pub base_backoff: Duration,
    /// The ceiling the doubling stops at.
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
    /// A policy with explicit bounds.
    pub fn new(max_retries: usize, base_backoff: Duration, max_backoff: Duration) -> Self {
        Self {
            max_retries,
            base_backoff,
            max_backoff,
        }
    }

    pub(crate) fn backoff(&self, attempt: usize) -> Duration {
        exponential(self.base_backoff, self.max_backoff, attempt)
    }
}

/// `base × 2^attempt`, capped at `max`. Both bounds are floored at
/// [`MIN_BACKOFF`], so a policy configured with zeros still sleeps.
fn exponential(base: Duration, max: Duration, attempt: usize) -> Duration {
    let factor = 2u32.saturating_pow(attempt.min(31) as u32);
    base.max(MIN_BACKOFF)
        .saturating_mul(factor)
        .min(max.max(MIN_BACKOFF))
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
#[non_exhaustive]
pub enum RetryMode {
    /// One attempt. For work nobody is waiting on.
    None,
    /// The classic bounded retry.
    Bounded(RetryPolicy),
    /// Retry until the caller drops the future; exponential from `base`
    /// capped at `max`. For work someone is waiting on.
    #[non_exhaustive]
    UntilCancelled {
        /// The first back-off.
        base: Duration,
        /// The ceiling the doubling stops at.
        max: Duration,
    },
}

impl RetryMode {
    /// An [`UntilCancelled`](Self::UntilCancelled) mode.
    pub fn until_cancelled(base: Duration, max: Duration) -> Self {
        Self::UntilCancelled { base, max }
    }
}

/// What an attempt does while the circuit breaker is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BreakerMode {
    /// Return `ErrorKind::BreakerOpen` at once.
    FailFast,
    /// Sleep until the cooldown ends and take the half-open probe slot.
    WaitForProbe,
}

/// The policy for one call.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CallPolicy {
    /// How failed attempts are repeated.
    pub retry: RetryMode,
    /// What to do while the breaker is open.
    pub breaker: BreakerMode,
}

impl CallPolicy {
    /// A policy from its two parts. The named constructors cover the usual
    /// combinations.
    pub fn new(retry: RetryMode, breaker: BreakerMode) -> Self {
        Self { retry, breaker }
    }

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
            retry: RetryMode::until_cancelled(Duration::from_millis(250), Duration::from_secs(10)),
            breaker: BreakerMode::WaitForProbe,
        }
    }

    /// Bounded retry, fail fast — what `execute` does.
    pub fn bounded(policy: RetryPolicy) -> Self {
        Self {
            retry: RetryMode::Bounded(policy),
            breaker: BreakerMode::FailFast,
        }
    }

    /// Keep this policy's retry mode but wait for the breaker's probe slot
    /// instead of failing fast.
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

    /// The ceiling a server's `Retry-After` is clamped to under this mode. A
    /// server asking for two minutes must not park a call whose own policy
    /// promises to give up — or to retry sooner — well before that.
    pub(crate) fn retry_after_cap(&self) -> Option<Duration> {
        match &self.retry {
            RetryMode::None => None,
            RetryMode::Bounded(p) => Some(p.max_backoff.max(MIN_BACKOFF)),
            RetryMode::UntilCancelled { max, .. } => Some((*max).max(MIN_BACKOFF)),
        }
    }
}

/// Statuses a retry may fix. Everything else in 4xx is final.
pub(crate) fn is_retryable_status(status: u16) -> bool {
    status == 408 || status == 429 || (500..600).contains(&status)
}

/// What one attempt's answer tells the circuit breaker. Retryability and
/// health are separate questions: `429` is worth retrying but proves the API
/// is up, and a `404` is not worth retrying but proves the same thing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Health {
    /// A `2xx`: the API works. Closes the breaker and clears the failure run.
    Healthy,
    /// An answer a retry cannot fix. The API is reachable, so this closes an
    /// open breaker, but it is not evidence of health: the failure run stands.
    Reachable,
    /// `408` / `429`: the server answered, but about its own load. Neither
    /// opens nor closes the breaker.
    Inconclusive,
    /// `5xx`: counts toward opening the breaker.
    Failing,
}

pub(crate) fn status_health(status: u16) -> Health {
    match status {
        200..=299 => Health::Healthy,
        408 | 429 => Health::Inconclusive,
        500..=599 => Health::Failing,
        _ => Health::Reachable,
    }
}
