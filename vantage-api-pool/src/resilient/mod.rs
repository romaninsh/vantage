//! `ResilientClient` — an async-native HTTP transport.
//!
//! This is the structured-concurrency replacement for the worker-pool +
//! oneshot-matcher machinery ([`AwwPool`](crate::AwwPool) /
//! [`HttpClientPool`](crate::HttpClientPool) /
//! [`EventualRequest`](crate::EventualRequest)). Instead of detaching requests
//! onto a pool and matching responses back by id, a caller just `.await`s
//! [`execute`](ResilientClient::execute): concurrency is bounded by a
//! [`Semaphore`], and the genuinely-valuable policies live as inline middleware:
//!
//! - **parallelism cap** — at most `max_parallel` requests in flight per client,
//! - **retry with backoff + jitter** on `429` / `5xx` / network errors, honoring
//!   `Retry-After`, bounded by `max_retries`,
//! - **auth refresh** — a `401` triggers one token re-acquire + replay,
//! - **circuit breaker** — after N consecutive failures the client fails fast
//!   for a cooldown, then probes (half-open).
//!
//! Cancellation is structural: drop the future and the in-flight request is
//! dropped with it — no detached task outlives its caller.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use tokio::sync::{RwLock, Semaphore};

/// Retry/backoff knobs. Backoff is exponential from `base` (doubling per
/// attempt), capped at `max`, with jitter added before each sleep.
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
    fn backoff(&self, attempt: usize) -> Duration {
        let factor = 2u32.saturating_pow(attempt as u32);
        let raw = self.base_backoff.saturating_mul(factor);
        raw.min(self.max_backoff)
    }
}

/// Add up to +25% jitter so retrying clients don't synchronize (thundering
/// herd). Entropy is the wall-clock subsecond — cheap and dependency-free.
fn with_jitter(d: Duration) -> Duration {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|t| t.subsec_nanos())
        .unwrap_or(0);
    let frac = (nanos % 250) as f64 / 1000.0; // 0.0..0.25
    d + d.mul_f64(frac)
}

/// Future returned by the auth token refresher.
pub type AuthFuture = Pin<Box<dyn Future<Output = Result<String>> + Send>>;
/// Acquire (or re-acquire) an auth token — called lazily and on `401`.
pub type AuthRefresher = Arc<dyn Fn() -> AuthFuture + Send + Sync>;

struct AuthState {
    token: RwLock<Option<String>>,
    refresh: AuthRefresher,
    header: String,
    scheme: String,
}

impl AuthState {
    async fn current(&self) -> Result<String> {
        if let Some(t) = self.token.read().await.clone() {
            return Ok(t);
        }
        self.reacquire().await
    }

    async fn reacquire(&self) -> Result<String> {
        let t = (self.refresh)().await?;
        *self.token.write().await = Some(t.clone());
        Ok(t)
    }
}

#[derive(Default)]
struct BreakerInner {
    consecutive_failures: usize,
    open_until: Option<Instant>,
}

struct CircuitBreaker {
    threshold: usize,
    cooldown: Duration,
    inner: std::sync::Mutex<BreakerInner>,
}

impl CircuitBreaker {
    /// Whether a request may proceed. When the breaker is open and the cooldown
    /// has elapsed, this returns `true` once (half-open probe) and re-arms.
    fn allow(&self) -> bool {
        let mut s = self.inner.lock().unwrap();
        if let Some(until) = s.open_until {
            if Instant::now() < until {
                return false;
            }
            s.open_until = None; // half-open: let one through
        }
        true
    }

    fn record_success(&self) {
        let mut s = self.inner.lock().unwrap();
        s.consecutive_failures = 0;
        s.open_until = None;
    }

    fn record_failure(&self) {
        let mut s = self.inner.lock().unwrap();
        s.consecutive_failures += 1;
        if s.consecutive_failures >= self.threshold {
            s.open_until = Some(Instant::now() + self.cooldown);
        }
    }
}

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
}

impl ResilientClient {
    pub fn builder() -> ResilientClientBuilder {
        ResilientClientBuilder::default()
    }

    /// Execute a request with all policies applied. `build` is called once per
    /// attempt with the shared `reqwest::Client`, so retries and auth re-apply
    /// against a fresh `RequestBuilder` (a `RequestBuilder` is single-use).
    ///
    /// Returns the first successful (`2xx`) response, or an error after retries
    /// are exhausted / the breaker is open / a non-retryable status.
    pub async fn execute<F>(&self, build: F) -> Result<reqwest::Response>
    where
        F: Fn(&reqwest::Client) -> reqwest::RequestBuilder,
    {
        if let Some(b) = &self.breaker {
            if !b.allow() {
                return Err(anyhow!("circuit breaker open"));
            }
        }
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| anyhow!("client closed"))?;

        let cur = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak_in_flight.fetch_max(cur, Ordering::SeqCst);

        let result = self.attempt_loop(&build).await;

        self.in_flight.fetch_sub(1, Ordering::SeqCst);
        if let Some(b) = &self.breaker {
            match &result {
                Ok(_) => b.record_success(),
                Err(_) => b.record_failure(),
            }
        }
        result
    }

    async fn attempt_loop<F>(&self, build: &F) -> Result<reqwest::Response>
    where
        F: Fn(&reqwest::Client) -> reqwest::RequestBuilder,
    {
        let mut attempt = 0usize;
        let mut refreshed = false;
        loop {
            let mut req = build(&self.http);
            if let Some(auth) = &self.auth {
                let token = auth.current().await?;
                req = req.header(&auth.header, format!("{}{token}", auth.scheme));
            }

            match req.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        return Ok(resp);
                    }
                    // 401 → refresh the token once and replay (not a retry).
                    if status.as_u16() == 401 && !refreshed {
                        if let Some(auth) = self.auth.as_ref() {
                            refreshed = true;
                            auth.reacquire().await?;
                            continue;
                        }
                    }
                    // 429 / 5xx → backoff + retry.
                    if status.as_u16() == 429 || status.is_server_error() {
                        if attempt >= self.policy.max_retries {
                            return Err(anyhow!(
                                "giving up after {attempt} retries: HTTP {status}"
                            ));
                        }
                        let delay =
                            retry_after(&resp).unwrap_or_else(|| self.policy.backoff(attempt));
                        attempt += 1;
                        tokio::time::sleep(with_jitter(delay)).await;
                        continue;
                    }
                    // other 4xx → not retryable.
                    return Err(anyhow!("HTTP {status}"));
                }
                Err(e) => {
                    if attempt >= self.policy.max_retries {
                        return Err(anyhow!("giving up after {attempt} retries: {e}"));
                    }
                    let delay = self.policy.backoff(attempt);
                    attempt += 1;
                    tokio::time::sleep(with_jitter(delay)).await;
                }
            }
        }
    }

    /// Highest number of simultaneously in-flight requests this client has
    /// observed — proves the parallelism cap holds. Test/diagnostic hook.
    pub fn peak_in_flight(&self) -> usize {
        self.peak_in_flight.load(Ordering::SeqCst)
    }
}

fn retry_after(resp: &reqwest::Response) -> Option<Duration> {
    let secs: u64 = resp
        .headers()
        .get("retry-after")?
        .to_str()
        .ok()?
        .parse()
        .ok()?;
    (secs >= 1).then(|| Duration::from_secs(secs))
}

/// Builder for [`ResilientClient`].
pub struct ResilientClientBuilder {
    http: Option<reqwest::Client>,
    max_parallel: usize,
    policy: RetryPolicy,
    breaker: Option<(usize, Duration)>,
    auth: Option<(AuthRefresher, String, String)>,
}

impl Default for ResilientClientBuilder {
    fn default() -> Self {
        Self {
            http: None,
            max_parallel: 8,
            policy: RetryPolicy::default(),
            breaker: None,
            auth: None,
        }
    }
}

impl ResilientClientBuilder {
    /// Cap concurrent in-flight requests for this client (the per-API
    /// parallelism limit). Default 8.
    pub fn max_parallel(mut self, n: usize) -> Self {
        self.max_parallel = n.max(1);
        self
    }

    pub fn retry(mut self, policy: RetryPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Open the breaker after `threshold` consecutive failures; stay open for
    /// `cooldown`, then allow one half-open probe.
    pub fn circuit_breaker(mut self, threshold: usize, cooldown: Duration) -> Self {
        self.breaker = Some((threshold.max(1), cooldown));
        self
    }

    /// Apply a bearer-style auth token, re-acquired lazily and on `401`.
    /// `refresher` returns the token value (without the scheme prefix).
    pub fn bearer_auth(mut self, refresher: AuthRefresher) -> Self {
        self.auth = Some((
            refresher,
            "Authorization".to_string(),
            "Bearer ".to_string(),
        ));
        self
    }

    /// Apply auth with a custom header name and scheme prefix.
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

    pub fn build(self) -> ResilientClient {
        ResilientClient {
            http: self.http.unwrap_or_default(),
            semaphore: Arc::new(Semaphore::new(self.max_parallel)),
            policy: self.policy,
            breaker: self.breaker.map(|(threshold, cooldown)| {
                Arc::new(CircuitBreaker {
                    threshold,
                    cooldown,
                    inner: std::sync::Mutex::new(BreakerInner::default()),
                })
            }),
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
        }
    }
}
