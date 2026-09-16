//! Assembling a [`ResilientClient`]: every policy is optional, and a client
//! built with no options at all is a plain bounded-retry HTTP client.

use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{RwLock, Semaphore};

use super::auth::{AuthRefresher, AuthState};
use super::breaker::CircuitBreaker;
use super::rate::TokenBucket;
use super::{ResilientClient, RetryPolicy, TransportObserver};

/// Builder for [`ResilientClient`].
pub struct ResilientClientBuilder {
    http: Option<reqwest::Client>,
    max_parallel: usize,
    policy: RetryPolicy,
    breaker: Option<(usize, Duration, Duration)>,
    auth: Option<(AuthRefresher, String, String)>,
    observer: Option<(Arc<str>, Arc<dyn TransportObserver>)>,
    rate: Option<(f64, usize)>,
}

impl Default for ResilientClientBuilder {
    fn default() -> Self {
        Self {
            http: None,
            max_parallel: 8,
            policy: RetryPolicy::default(),
            breaker: None,
            auth: None,
            observer: None,
            rate: None,
        }
    }
}

impl ResilientClientBuilder {
    /// Cap concurrent in-flight requests for this client. Default 8.
    pub fn max_parallel(mut self, n: usize) -> Self {
        self.max_parallel = n.max(1);
        self
    }

    pub fn retry(mut self, policy: RetryPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Open the breaker after `threshold` consecutive failures; stay open for
    /// `cooldown` (fixed), then allow one half-open probe.
    pub fn circuit_breaker(mut self, threshold: usize, cooldown: Duration) -> Self {
        self.breaker = Some((threshold, cooldown, cooldown));
        self
    }

    /// Like `circuit_breaker`, but each failed probe doubles the cooldown up
    /// to `max_cooldown`; a success resets it to `base_cooldown`.
    pub fn circuit_breaker_growing(
        mut self,
        threshold: usize,
        base_cooldown: Duration,
        max_cooldown: Duration,
    ) -> Self {
        self.breaker = Some((threshold, base_cooldown, max_cooldown));
        self
    }

    /// The spec's default: 5 failures, 5 s doubling to 60 s.
    pub fn default_breaker(self) -> Self {
        self.circuit_breaker_growing(5, Duration::from_secs(5), Duration::from_secs(60))
    }

    /// Bearer auth, re-acquired lazily and on `401`.
    pub fn bearer_auth(self, refresher: AuthRefresher) -> Self {
        self.auth(refresher, "Authorization", "Bearer ")
    }

    /// Auth with a custom header name and scheme prefix.
    pub fn auth(
        mut self,
        refresher: AuthRefresher,
        header: impl Into<String>,
        scheme: impl Into<String>,
    ) -> Self {
        self.auth = Some((refresher, header.into(), scheme.into()));
        self
    }

    pub fn http_client(mut self, client: reqwest::Client) -> Self {
        self.http = Some(client);
        self
    }

    /// At most `per_second` attempts per second, with a burst of
    /// `per_second.ceil()`.
    pub fn rate_limit(self, per_second: f64) -> Self {
        let burst = per_second.ceil().max(1.0) as usize;
        self.rate_limit_with_burst(per_second, burst)
    }

    pub fn rate_limit_with_burst(mut self, per_second: f64, burst: usize) -> Self {
        self.rate = Some((per_second, burst));
        self
    }

    /// Report every attempt, retry and breaker transition under `key`. See
    /// [`TransportObserver::on_event`] for what the callback may do.
    pub fn observer(
        mut self,
        key: impl Into<Arc<str>>,
        observer: Arc<dyn TransportObserver>,
    ) -> Self {
        self.observer = Some((key.into(), observer));
        self
    }

    pub fn build(self) -> ResilientClient {
        ResilientClient {
            http: self.http.unwrap_or_default(),
            semaphore: Arc::new(Semaphore::new(self.max_parallel)),
            policy: self.policy,
            breaker: self
                .breaker
                .map(|(t, base, max)| Arc::new(CircuitBreaker::new(t, base, max))),
            auth: self.auth.map(|(refresh, header, scheme)| {
                Arc::new(AuthState {
                    token: RwLock::new(None),
                    refresh,
                    header,
                    scheme,
                })
            }),
            in_flight: Arc::new(AtomicUsize::new(0)),
            peak_in_flight: Arc::new(AtomicUsize::new(0)),
            observer: self.observer,
            rate: self.rate.map(|(r, b)| Arc::new(TokenBucket::new(r, b))),
        }
    }
}
