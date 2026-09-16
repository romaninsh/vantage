//! Shared fixtures for the `resilient_*` test binaries: compressed policies,
//! the deadline wrapper every unbounded call runs under, shorthands for the
//! mock server and for driving a client, an event recorder and scripted auth
//! refreshers.
//!
//! Each test binary uses a subset of this.
#![allow(dead_code)]

use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::task::JoinHandle;
use vantage_api_pool::resilient::{
    AuthRefresher, BreakerMode, CallPolicy, ErrorKind, RetryMode, RetryPolicy, TransportEvent,
    TransportObserver,
};
use vantage_api_pool::ResilientClient;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

/// A bounded retry policy with the back-off compressed to 1–5 ms, so a test
/// that needs several retries finishes in milliseconds.
pub fn fast_retry(max_retries: usize) -> RetryPolicy {
    RetryPolicy {
        max_retries,
        base_backoff: Duration::from_millis(1),
        max_backoff: Duration::from_millis(5),
    }
}

/// `CallPolicy::essential()` with the back-off compressed from 250 ms–10 s to
/// 1–4 ms.
pub fn essential_fast() -> CallPolicy {
    CallPolicy {
        retry: RetryMode::UntilCancelled {
            base: Duration::from_millis(1),
            max: Duration::from_millis(4),
        },
        breaker: BreakerMode::WaitForProbe,
    }
}

/// Run an unbounded call under a deadline: `UntilCancelled` retries forever,
/// so a regression must fail the test rather than hang the suite.
pub async fn within<T>(fut: impl Future<Output = T>) -> T {
    within_secs(5, fut).await
}

pub async fn within_secs<T>(secs: u64, fut: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(secs), fut)
        .await
        .unwrap_or_else(|_| panic!("the call did not finish within {secs}s"))
}

/// Start an essential call in its own task, so the test can [`cancel`] it —
/// dropping the future is the only way such a call ever stops.
pub fn spawn_essential(client: &ResilientClient, url: &str) -> JoinHandle<()> {
    let client = client.clone();
    let url = url.to_string();
    tokio::spawn(async move {
        let _ = within(client.execute_with(&essential_fast(), |h| h.get(&url))).await;
    })
}

/// Drop the call a task is running, and wait until it has stopped.
pub async fn cancel(task: JoinHandle<()>) {
    task.abort();
    let _ = task.await;
}

/// The `ErrorKind` of a background call that is expected to fail.
pub async fn background_error(client: &ResilientClient, url: &str) -> ErrorKind {
    client
        .execute_with(&CallPolicy::background(), |h| h.get(url))
        .await
        .expect_err("the call was expected to fail")
        .kind
}

/// A background call that is expected to succeed; returns its status.
pub async fn background_ok(client: &ResilientClient, url: &str) -> u16 {
    client
        .execute_with(&CallPolicy::background(), |h| h.get(url))
        .await
        .expect("the call was expected to succeed")
        .status()
        .as_u16()
}

/// Make `failures` background calls against a failing server, ignoring their
/// outcome — enough of them reaches the breaker's threshold and opens it.
pub async fn open_breaker(client: &ResilientClient, url: &str, failures: usize) {
    for _ in 0..failures {
        let _ = client
            .execute_with(&CallPolicy::background(), |h| h.get(url))
            .await;
    }
}

/// Answer every `GET` with `responder`.
pub async fn mount(server: &MockServer, responder: impl Respond + 'static) {
    Mock::given(method("GET"))
        .respond_with(responder)
        .mount(server)
        .await;
}

/// Answer every request with `status`.
pub async fn mount_status(server: &MockServer, status: u16) {
    mount(server, ResponseTemplate::new(status)).await;
}

/// Answer `statuses` in order, then repeat the last one.
pub async fn mount_script(server: &MockServer, statuses: impl Into<Vec<u16>>) {
    mount(server, Script::new(statuses)).await;
}

/// Answer every request with `status`, `delay` after it arrives.
pub async fn mount_delayed(server: &MockServer, status: u16, delay: Duration) {
    mount(server, ResponseTemplate::new(status).set_delay(delay)).await;
}

/// How many requests the server has received.
pub async fn requests(server: &MockServer) -> usize {
    server.received_requests().await.unwrap().len()
}

/// Block until the server has actually received `n` requests, so a test that
/// wants to act on an in-flight request is not racing a fixed sleep.
pub async fn wait_for_requests(server: &MockServer, n: usize, limit: Duration) -> usize {
    let deadline = Instant::now() + limit;
    loop {
        let seen = requests(server).await;
        if seen >= n {
            return seen;
        }
        assert!(
            Instant::now() < deadline,
            "the server saw {seen} of {n} requests within {limit:?}"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// Poll `cond` until it holds, or fail after `limit`.
pub async fn wait_until(limit: Duration, what: &str, cond: impl Fn() -> bool) {
    let deadline = Instant::now() + limit;
    while !cond() {
        assert!(
            Instant::now() < deadline,
            "{what} did not happen in {limit:?}"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// A server that fails every other request.
#[derive(Default)]
pub struct HalfFailing(AtomicUsize);

impl Respond for HalfFailing {
    fn respond(&self, _request: &Request) -> ResponseTemplate {
        let n = self.0.fetch_add(1, Ordering::SeqCst);
        ResponseTemplate::new(if n.is_multiple_of(2) { 503 } else { 200 })
    }
}

/// A server that answers a fixed sequence of statuses, then repeats the last.
pub struct Script {
    statuses: Vec<u16>,
    next: AtomicUsize,
}

impl Script {
    pub fn new(statuses: impl Into<Vec<u16>>) -> Self {
        let statuses = statuses.into();
        assert!(!statuses.is_empty(), "a script needs at least one status");
        Self {
            statuses,
            next: AtomicUsize::new(0),
        }
    }
}

impl Respond for Script {
    fn respond(&self, _request: &Request) -> ResponseTemplate {
        let i = self
            .next
            .fetch_add(1, Ordering::SeqCst)
            .min(self.statuses.len() - 1);
        ResponseTemplate::new(self.statuses[i])
    }
}

/// How many times a scripted refresher has been asked for a token.
#[derive(Clone, Default)]
pub struct Calls(Arc<AtomicUsize>);

impl Calls {
    pub fn count(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }
}

/// A refresher whose call number `n` (0-based) answers with `answer(n)`,
/// alongside its call count.
pub fn scripted_refresher(
    answer: impl Fn(usize) -> anyhow::Result<String> + Send + Sync + 'static,
) -> (AuthRefresher, Calls) {
    let calls = Calls::default();
    let counter = calls.clone();
    let refresher: AuthRefresher = Arc::new(move || {
        let token = answer(counter.0.fetch_add(1, Ordering::SeqCst));
        Box::pin(async move { token })
    });
    (refresher, calls)
}

/// A refresher that hands out `token-0`, `token-1`, … and never fails.
pub fn counting_refresher() -> (AuthRefresher, Calls) {
    scripted_refresher(|n| Ok(format!("token-{n}")))
}

/// Records every event a client reports, in order.
#[derive(Default)]
pub struct Recorder {
    events: Mutex<Vec<(String, String)>>,
    failed_ms: Mutex<Vec<u64>>,
    succeeded_ms: Mutex<Vec<u64>>,
}

impl Recorder {
    /// One short tag per event, in order — what most assertions compare.
    pub fn tags(&self) -> Vec<String> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .map(|(_, tag)| tag.clone())
            .collect()
    }

    /// The key every event was reported under; panics if they disagree.
    pub fn key(&self) -> Option<String> {
        let events = self.events.lock().unwrap();
        let mut keys = events.iter().map(|(k, _)| k);
        let first = keys.next()?.clone();
        assert!(keys.all(|k| *k == first), "events used more than one key");
        Some(first)
    }

    /// The `ms` of each `Failed`, in event order.
    pub fn failed_ms(&self) -> Vec<u64> {
        self.failed_ms.lock().unwrap().clone()
    }

    /// The `ms` of each `Succeeded`, in event order.
    pub fn succeeded_ms(&self) -> Vec<u64> {
        self.succeeded_ms.lock().unwrap().clone()
    }
}

impl TransportObserver for Recorder {
    fn on_event(&self, key: &str, event: TransportEvent) {
        let tag = match event {
            TransportEvent::Started => "started".to_string(),
            TransportEvent::Succeeded { status, ms, .. } => {
                self.succeeded_ms.lock().unwrap().push(ms);
                format!("ok:{status}")
            }
            TransportEvent::Failed { error, ms } => {
                self.failed_ms.lock().unwrap().push(ms);
                format!("failed:{}", error.kind_name())
            }
            TransportEvent::RetryScheduled { attempt, .. } => format!("retry:{attempt}"),
            TransportEvent::BreakerOpened { .. } => "opened".to_string(),
            TransportEvent::BreakerClosed => "closed".to_string(),
            TransportEvent::RowsPulled { n } => format!("rows:{n}"),
            TransportEvent::WritePushed => "write".to_string(),
        };
        self.events.lock().unwrap().push((key.to_string(), tag));
    }
}
