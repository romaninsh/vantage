//! Request priority, carried as a tokio task-local: the owner of a wait sets
//! it with [`Priority::scope`], transports read it with [`Priority::current`].
//! A task spawned with `tokio::spawn` does not inherit it; the spawner must
//! wrap the spawned future in another `scope`. [`Backoff`] is how long an
//! `Essential` request waits between attempts.

use std::future::Future;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The shortest sleep a [`Backoff`] schedules. A zero base (or a zero
/// ceiling) would otherwise spin.
const MIN_BACKOFF: Duration = Duration::from_millis(1);

/// Whether someone is waiting on the result of the request about to be
/// sent. Transports choose their retry policy from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Priority {
    /// Nobody is waiting: a poll over cached rows, a refresh, hydration.
    /// One attempt; a failure only feeds health.
    #[default]
    Background,
    /// Someone is waiting: a cold page, an uncached viewport, a write.
    /// Retry until the awaiting future is dropped.
    Essential,
}

tokio::task_local! {
    static PRIORITY: Priority;
}

impl Priority {
    /// The priority of the current task, `Background` when none was set.
    pub fn current() -> Priority {
        PRIORITY.try_with(|p| *p).unwrap_or_default()
    }

    pub fn is_essential(self) -> bool {
        matches!(self, Priority::Essential)
    }

    /// Run `fut` with this priority visible to everything it awaits.
    pub async fn scope<F: Future>(self, fut: F) -> F::Output {
        PRIORITY.scope(self, fut).await
    }
}

/// Exponential back-off between attempts: `base × 2^attempt`, capped at
/// `max`. Both bounds are floored at 1 ms, so a schedule built from zeros
/// still sleeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Backoff {
    /// The sleep before the first retry; doubles for each one after.
    pub base: Duration,
    /// The ceiling the doubling stops at.
    pub max: Duration,
}

impl Backoff {
    /// The schedule of a request someone is waiting on: 250 ms doubling to
    /// 10 s, until the caller drops the future.
    pub const ESSENTIAL: Backoff =
        Backoff::new(Duration::from_millis(250), Duration::from_secs(10));

    pub const fn new(base: Duration, max: Duration) -> Self {
        Self { base, max }
    }

    /// The sleep before retry number `attempt` (0 for the first retry),
    /// without jitter.
    pub fn step(self, attempt: usize) -> Duration {
        let factor = 2u32.saturating_pow(attempt.min(31) as u32);
        self.base
            .max(MIN_BACKOFF)
            .saturating_mul(factor)
            .min(self.ceiling())
    }

    /// [`step`](Self::step) with jitter: what a retry loop sleeps.
    pub fn delay(self, attempt: usize) -> Duration {
        Self::jitter(self.step(attempt))
    }

    /// The longest step, floored like the others. A server's `Retry-After`
    /// is clamped to it.
    pub fn ceiling(self) -> Duration {
        self.max.max(MIN_BACKOFF)
    }

    /// `d` plus up to 25%, so retrying clients don't synchronize.
    pub fn jitter(d: Duration) -> Duration {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|t| t.subsec_nanos())
            .unwrap_or(0);
        let frac = (nanos % 250) as f64 / 1000.0;
        d + d.mul_f64(frac)
    }
}
