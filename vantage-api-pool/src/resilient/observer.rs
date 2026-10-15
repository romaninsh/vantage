//! What a client tells the outside world about its traffic, keyed by the `key`
//! given to the builder.

use std::time::Duration;

use super::ClientError;

/// One thing a client reports to its [`TransportObserver`]. New events may be
/// added; match with a wildcard arm.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum TransportEvent {
    /// Once per attempt: immediately before the request is sent, or, for a
    /// fail-fast rejection, immediately before its `Failed { ms: 0 }`.
    Started,
    /// One attempt got a `2xx`. `bytes` is the `Content-Length`, when sent.
    #[non_exhaustive]
    Succeeded {
        /// The HTTP status.
        status: u16,
        /// Milliseconds from sending to the response head.
        ms: u64,
        /// The response's `Content-Length`, when the server sent one.
        bytes: Option<u64>,
    },
    /// One attempt failed. The last `Failed` of a call carries the error the
    /// call returns.
    #[non_exhaustive]
    Failed {
        /// What went wrong in this attempt.
        error: ClientError,
        /// Milliseconds from sending to the failure; `0` when nothing was sent.
        ms: u64,
    },
    /// The call's future was dropped after `Started` and before the attempt
    /// landed: the request is abandoned, and nothing is known about how the
    /// server would have answered.
    Cancelled,
    /// A retry will run after `after`; `attempt` is its 1-based number.
    #[non_exhaustive]
    RetryScheduled {
        /// The back-off before the retry.
        after: Duration,
        /// The retry's 1-based number.
        attempt: usize,
    },
    /// The circuit breaker opened, or a half-open probe failed and it
    /// re-opened.
    #[non_exhaustive]
    BreakerOpened {
        /// How long it stays open before the next probe.
        cooldown: Duration,
    },
    /// The circuit breaker closed again.
    BreakerClosed,
    /// Reported by the caller once it has parsed a response.
    #[non_exhaustive]
    RowsPulled {
        /// Rows parsed from the response.
        n: usize,
    },
    /// Reported by the caller once a write was acknowledged.
    WritePushed,
}

impl TransportEvent {
    /// A [`Succeeded`](Self::Succeeded) event.
    pub fn succeeded(status: u16, ms: u64, bytes: Option<u64>) -> Self {
        Self::Succeeded { status, ms, bytes }
    }

    /// A [`Failed`](Self::Failed) event.
    pub fn failed(error: ClientError, ms: u64) -> Self {
        Self::Failed { error, ms }
    }

    /// A [`RetryScheduled`](Self::RetryScheduled) event.
    pub fn retry_scheduled(after: Duration, attempt: usize) -> Self {
        Self::RetryScheduled { after, attempt }
    }

    /// A [`RowsPulled`](Self::RowsPulled) event.
    pub fn rows_pulled(n: usize) -> Self {
        Self::RowsPulled { n }
    }

    /// A [`BreakerOpened`](Self::BreakerOpened) event.
    pub fn breaker_opened(cooldown: Duration) -> Self {
        Self::BreakerOpened { cooldown }
    }
}

/// Receives a client's [`TransportEvent`]s.
pub trait TransportObserver: Send + Sync {
    /// Called synchronously on the request path, from the task that owns the
    /// call, and — for `Started` — while that attempt's semaphore permit is
    /// held. The contract:
    ///
    /// - **Do not block, await or sleep.** Copy what you need into your own
    ///   state and return. Time spent in `Started` is time the client's
    ///   parallelism budget is not doing work; time spent in any other event
    ///   delays the call that reported it.
    /// - **Do not call back into the client** (`execute`, `execute_with`,
    ///   `breaker_state`). The call path is holding client state.
    /// - `Started` pairs with exactly one terminal event — `Succeeded`,
    ///   `Failed` or `Cancelled` — per attempt, including for a fail-fast rejection, which
    ///   emits a synthetic `Started` followed by `Failed { ms: 0 }` whose
    ///   error has `kind_name() == "breaker_open"`. Those two never touched
    ///   the network; filter them out of request-rate figures.
    /// - `BreakerOpened` repeats on every failed probe, with a longer
    ///   `cooldown` each time and no intervening `BreakerClosed`. Treat it as
    ///   "open until now + cooldown" — set the deadline, do not count the
    ///   events.
    /// - `ms` covers building, sending and reading the response head. It
    ///   excludes waiting for a breaker cooldown, a rate-limit token, a
    ///   semaphore permit, a retry back-off, and the auth round trip.
    fn on_event(&self, key: &str, event: TransportEvent);
}
