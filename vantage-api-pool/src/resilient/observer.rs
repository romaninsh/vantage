//! What a client tells the outside world about its traffic. The consumer
//! (the desktop app's health registry) keys everything by the `key` given to
//! the builder — the datasource name — and the crate never learns what a
//! datasource is.

use std::time::Duration;

use super::ClientError;

#[derive(Debug, Clone)]
pub enum TransportEvent {
    /// Once per attempt: immediately before the request is sent, or, for a
    /// fail-fast rejection, immediately before its `Failed { ms: 0 }`.
    Started,
    /// One attempt got a `2xx`. `bytes` is the `Content-Length`, when sent.
    Succeeded {
        status: u16,
        ms: u64,
        bytes: Option<u64>,
    },
    /// One attempt failed. The last `Failed` of a call carries the error the
    /// call returns.
    Failed {
        error: ClientError,
        ms: u64,
    },
    /// A retry will run after `after`; `attempt` is its 1-based number.
    RetryScheduled {
        after: Duration,
        attempt: usize,
    },
    BreakerOpened {
        cooldown: Duration,
    },
    BreakerClosed,
    /// Reported by the caller once it has parsed a response.
    RowsPulled {
        n: usize,
    },
    /// Reported by the caller once a write was acknowledged.
    WritePushed,
}

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
    /// - `Started` pairs with exactly one terminal event — `Succeeded` or
    ///   `Failed` — per attempt, including for a fail-fast rejection, which
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
