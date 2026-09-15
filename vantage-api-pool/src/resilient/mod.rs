//! `ResilientClient` — an async-native HTTP transport.
//!
//! One client per remote API. Concurrency is bounded by a semaphore; retry,
//! auth refresh and a circuit breaker are inline middleware. Cancellation is
//! structural: drop the future and the in-flight request goes with it.

mod breaker;
mod error;
mod observer;
mod policy;

pub use error::{ClientError, ErrorKind};
pub use observer::{TransportEvent, TransportObserver};
pub use policy::{BreakerMode, CallPolicy, RetryMode};
pub use policy::RetryPolicy;
use policy::is_retryable_status;

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tokio::sync::{RwLock, Semaphore};

use breaker::CircuitBreaker;
use policy::with_jitter;

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
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| ClientError::new(ErrorKind::Closed, 0))?;

        let cur = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak_in_flight.fetch_max(cur, Ordering::SeqCst);
        let result = self.attempt_loop(policy, &build).await;
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
        result
    }

    // `probe` is a Drop-only guard: the compiler cannot see that clearing it
    // (or letting it fall out of scope) is what releases the half-open
    // probe slot, so every assignment to it looks dead without this.
    #[allow(unused_assignments, unused_variables)]
    async fn attempt_loop<F>(
        &self,
        policy: &CallPolicy,
        build: &F,
    ) -> Result<reqwest::Response, ClientError>
    where
        F: Fn(&reqwest::Client) -> reqwest::RequestBuilder,
    {
        let mut attempt = 0usize;
        let mut refreshed = false;
        // Holds the half-open probe slot for whichever attempt was granted
        // it, and ONLY for that attempt's in-flight window: it is reset to
        // `None` right after the outcome is recorded (success, failure, or
        // 401), before any retry sleep or `continue`, so a later probe
        // grant in the same call never drops a stale guard and clears a
        // flag `gate()` just set for the new grant. For the paths that
        // don't record anything — an early `?` on auth failure, or the
        // enclosing future being cancelled — the guard is still alive and
        // its `Drop` releases the slot.
        let mut probe: Option<breaker::ProbeGuard> = None;
        loop {
            if let Some(b) = &self.breaker {
                loop {
                    match b.gate() {
                        breaker::Gate::Allow { probe: is_probe } => {
                            if is_probe {
                                probe = Some(breaker::ProbeGuard::new(Arc::clone(b)));
                            }
                            break;
                        }
                        breaker::Gate::OpenFor(wait) => match policy.breaker {
                            BreakerMode::FailFast => {
                                let error = ClientError::new(ErrorKind::BreakerOpen, attempt);
                                self.report(TransportEvent::Failed { error: error.clone(), ms: 0 });
                                return Err(error);
                            }
                            BreakerMode::WaitForProbe => tokio::time::sleep(wait).await,
                        },
                    }
                }
            }

            self.report(TransportEvent::Started);
            let started = std::time::Instant::now();

            let mut req = build(&self.http);
            if let Some(auth) = &self.auth {
                let token = auth
                    .current()
                    .await
                    .map_err(|e| ClientError::new(ErrorKind::Auth(e.to_string()), attempt + 1))?;
                req = req.header(&auth.header, format!("{}{token}", auth.scheme));
            }

            let outcome: Result<reqwest::Response, (ErrorKind, Option<Duration>)> =
                match req.send().await {
                    Ok(resp) if resp.status().is_success() => Ok(resp),
                    Ok(resp) => {
                        let status = resp.status().as_u16();
                        if status == 401 && !refreshed {
                            if let Some(auth) = self.auth.as_ref() {
                                refreshed = true;
                                // A 401 still proves the server answers.
                                if let Some(b) = &self.breaker {
                                    if b.record_success() {
                                        self.report(TransportEvent::BreakerClosed);
                                    }
                                }
                                probe = None;
                                auth.reacquire().await.map_err(|e| {
                                    ClientError::new(ErrorKind::Auth(e.to_string()), attempt + 1)
                                })?;
                                continue;
                            }
                        }
                        let hint = retry_after(&resp);
                        Err((ErrorKind::Status(status), hint))
                    }
                    Err(e) => Err((ErrorKind::Transport(e.to_string()), None)),
                };

            match outcome {
                Ok(resp) => {
                    let bytes = resp.content_length();
                    self.report(TransportEvent::Succeeded {
                        status: resp.status().as_u16(),
                        ms: started.elapsed().as_millis() as u64,
                        bytes,
                    });
                    if let Some(b) = &self.breaker {
                        if b.record_success() {
                            self.report(TransportEvent::BreakerClosed);
                        }
                    }
                    probe = None;
                    return Ok(resp);
                }
                Err((kind, hint)) => {
                    let ms = started.elapsed().as_millis() as u64;
                    let retryable = match &kind {
                        ErrorKind::Status(s) => is_retryable_status(*s),
                        ErrorKind::Transport(_) => true,
                        _ => false,
                    };
                    let error = ClientError::new(kind, attempt + 1);
                    self.report(TransportEvent::Failed { error: error.clone(), ms });
                    if retryable {
                        if let Some(b) = &self.breaker {
                            if let Some(cooldown) = b.record_failure() {
                                self.report(TransportEvent::BreakerOpened { cooldown });
                            }
                        }
                    }
                    if !retryable {
                        // A non-retryable status still proves the server is
                        // reachable: the breaker tracks reachability, not
                        // correctness, so this closes it (and, if this was
                        // the probe, releases the slot).
                        if let Some(b) = &self.breaker {
                            if b.record_success() {
                                self.report(TransportEvent::BreakerClosed);
                            }
                        }
                        probe = None;
                        return Err(error);
                    }
                    // The outcome is recorded; drop the guard now so the
                    // retry sleep below never holds the probe slot, and so
                    // a later probe grant in this same call doesn't drop a
                    // stale guard and clear the flag it just set.
                    probe = None;
                    let Some(delay) = policy.next_backoff(attempt) else {
                        return Err(error);
                    };
                    attempt += 1;
                    let after = with_jitter(hint.unwrap_or(delay));
                    self.report(TransportEvent::RetryScheduled { after, attempt });
                    tokio::time::sleep(after).await;
                }
            }
        }
    }

    /// Highest number of simultaneously in-flight requests observed.
    pub fn peak_in_flight(&self) -> usize {
        self.peak_in_flight.load(Ordering::SeqCst)
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
}

pub(crate) fn retry_after(resp: &reqwest::Response) -> Option<Duration> {
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
    breaker: Option<(usize, Duration, Duration)>,
    auth: Option<(AuthRefresher, String, String)>,
    observer: Option<(Arc<str>, Arc<dyn TransportObserver>)>,
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
    pub fn bearer_auth(mut self, refresher: AuthRefresher) -> Self {
        self.auth = Some((refresher, "Authorization".to_string(), "Bearer ".to_string()));
        self
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

    /// Report every attempt, retry and breaker transition under `key`.
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
        }
    }
}
