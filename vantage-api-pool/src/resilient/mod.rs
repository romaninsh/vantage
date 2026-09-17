//! `ResilientClient` — an async-native HTTP transport.
//!
//! One client per remote API. Concurrency is bounded by a semaphore; retry,
//! auth refresh, a rate limit and a circuit breaker are inline middleware.
//! Cancellation is structural: drop the future and the in-flight request goes
//! with it.
//!
//! The pieces live in one file each: `builder` assembles a client, `attempt`
//! runs one call's attempt loop, `breaker` is the circuit breaker, `rate` the
//! token bucket, `policy` the retry knobs, and `auth`, `error` and `observer`
//! the types those exchange.

mod attempt;
mod auth;
mod breaker;
mod builder;
mod error;
mod observer;
mod policy;
mod rate;

pub use auth::{AuthFuture, AuthRefresher};
pub use breaker::BreakerState;
pub use builder::ResilientClientBuilder;
pub use error::{ClientError, ErrorKind};
pub use observer::{TransportEvent, TransportObserver};
pub use policy::{BreakerMode, CallPolicy, RetryMode, RetryPolicy};

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use tokio::sync::Semaphore;

use attempt::AttemptLoop;
use auth::AuthState;
use breaker::CircuitBreaker;

/// A cheap-to-clone resilient HTTP client. Build via [`ResilientClient::builder`].
#[derive(Clone)]
pub struct ResilientClient {
    http: reqwest::Client,
    semaphore: Arc<Semaphore>,
    policy: RetryPolicy,
    breaker: Option<Arc<CircuitBreaker>>,
    auth: Option<Arc<AuthState>>,
    in_flight: Arc<AtomicUsize>,
    peak_in_flight: Arc<AtomicUsize>,
    observer: Option<(Arc<str>, Arc<dyn TransportObserver>)>,
    rate: Option<Arc<rate::TokenBucket>>,
}

impl ResilientClient {
    pub fn builder() -> ResilientClientBuilder {
        ResilientClientBuilder::default()
    }

    /// Execute a request with the client's default policies (bounded retry,
    /// fail fast on an open breaker). `build` is called once per attempt.
    pub async fn execute<F>(&self, build: F) -> Result<reqwest::Response, ClientError>
    where
        F: Fn(&reqwest::Client) -> reqwest::RequestBuilder,
    {
        let policy = CallPolicy::bounded(self.policy.clone());
        self.execute_with(&policy, build).await
    }

    /// Execute a request under `policy`. Returns the first `2xx` response,
    /// or the last attempt's failure. Dropping the returned future cancels
    /// the in-flight request and any pending back-off.
    pub async fn execute_with<F>(
        &self,
        policy: &CallPolicy,
        build: F,
    ) -> Result<reqwest::Response, ClientError>
    where
        F: Fn(&reqwest::Client) -> reqwest::RequestBuilder,
    {
        AttemptLoop::run(self, policy, &build).await
    }

    /// Requests in flight right now — sent, not yet answered. Waiting for a
    /// breaker cooldown, a rate-limit token, a permit or a retry back-off
    /// does not count.
    pub fn in_flight(&self) -> usize {
        self.in_flight.load(Ordering::SeqCst)
    }

    /// Highest number of simultaneously in-flight requests observed.
    pub fn peak_in_flight(&self) -> usize {
        self.peak_in_flight.load(Ordering::SeqCst)
    }

    /// What the circuit breaker is doing right now, for consumers that poll
    /// rather than follow `TransportEvent`s. `None` when this client has no
    /// breaker configured.
    pub fn breaker_state(&self) -> Option<BreakerState> {
        self.breaker.as_ref().map(|b| b.state())
    }

    /// The datasource key this client reports under, when it has an observer.
    pub fn key(&self) -> Option<&str> {
        self.observer.as_ref().map(|(k, _)| &**k)
    }

    /// Emit an event on behalf of the caller (`RowsPulled`, `WritePushed`).
    pub fn report(&self, event: TransportEvent) {
        if let Some((key, obs)) = &self.observer {
            obs.on_event(key, event);
        }
    }

    /// Count one request as in flight until the returned guard is dropped —
    /// including when the caller's future is cancelled mid-send, which is why
    /// this is a guard and not a pair of `fetch_add` / `fetch_sub` calls.
    fn enter_flight(&self) -> InFlightGuard {
        let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak_in_flight.fetch_max(now, Ordering::SeqCst);
        InFlightGuard(Arc::clone(&self.in_flight))
    }
}

impl std::fmt::Debug for ResilientClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResilientClient")
            .field("key", &self.key())
            .field("in_flight", &self.in_flight())
            .finish_non_exhaustive()
    }
}

struct InFlightGuard(Arc<AtomicUsize>);

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
