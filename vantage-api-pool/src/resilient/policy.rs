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
