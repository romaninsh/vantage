//! Per-call policies: background calls never retry, essential calls retry
//! until the caller drops them, and 4xx is final under every mode.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use vantage_api_pool::resilient::{AuthRefresher, BreakerMode, CallPolicy, ErrorKind, RetryMode};
use vantage_api_pool::{ResilientClient, RetryPolicy};
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

fn essential_fast() -> CallPolicy {
    CallPolicy {
        retry: RetryMode::UntilCancelled {
            base: Duration::from_millis(1),
            max: Duration::from_millis(4),
        },
        breaker: BreakerMode::WaitForProbe,
    }
}

#[tokio::test]
async fn background_call_makes_exactly_one_attempt() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    let client = ResilientClient::builder().build();
    let url = server.uri();
    let err = client
        .execute_with(&CallPolicy::background(), |h| h.get(&url))
        .await
        .unwrap_err();
    assert_eq!(err.kind, ErrorKind::Status(503));
    assert_eq!(err.attempts, 1);
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn essential_call_retries_until_the_server_recovers() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(7)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let client = ResilientClient::builder().build();
    let url = server.uri();
    let resp = client
        .execute_with(&essential_fast(), |h| h.get(&url))
        .await
        .expect("recovers after 7 failures");
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(server.received_requests().await.unwrap().len(), 8);
}

#[tokio::test]
async fn essential_call_stops_when_dropped() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    let client = ResilientClient::builder().build();
    let url = server.uri();
    let task = tokio::spawn({
        let client = client.clone();
        async move {
            let _ = client.execute_with(&essential_fast(), |h| h.get(&url)).await;
        }
    });
    tokio::time::sleep(Duration::from_millis(30)).await;
    task.abort();
    let _ = task.await;
    let seen = server.received_requests().await.unwrap().len();
    assert!(seen >= 3, "was retrying while alive: {seen}");
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert_eq!(
        server.received_requests().await.unwrap().len(),
        seen,
        "no request after the caller dropped the future"
    );
}

#[tokio::test]
async fn four_xx_is_final_even_for_essential_calls() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let client = ResilientClient::builder().build();
    let url = server.uri();
    let err = client
        .execute_with(&essential_fast(), |h| h.get(&url))
        .await
        .unwrap_err();
    assert_eq!(err.kind, ErrorKind::Status(404));
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn bounded_policy_per_call_overrides_client_default() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    let client = ResilientClient::builder()
        .retry(RetryPolicy {
            max_retries: 9,
            base_backoff: Duration::from_millis(1),
            max_backoff: Duration::from_millis(1),
        })
        .build();
    let url = server.uri();
    let policy = CallPolicy::bounded(RetryPolicy {
        max_retries: 1,
        base_backoff: Duration::from_millis(1),
        max_backoff: Duration::from_millis(1),
    });
    let err = client
        .execute_with(&policy, |h| h.get(&url))
        .await
        .unwrap_err();
    assert_eq!(err.attempts, 2);
}

#[tokio::test]
async fn execute_keeps_the_client_default_policy() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    let client = ResilientClient::builder()
        .retry(RetryPolicy {
            max_retries: 2,
            base_backoff: Duration::from_millis(1),
            max_backoff: Duration::from_millis(1),
        })
        .build();
    let url = server.uri();
    let err = client.execute(|h| h.get(&url)).await.unwrap_err();
    assert_eq!(err.attempts, 3);
}

/// Shared helper for later tasks: counts requests the server saw.
#[allow(dead_code)]
fn hits(counter: &Arc<AtomicUsize>) -> usize {
    counter.load(Ordering::SeqCst)
}

#[tokio::test]
async fn cooldown_doubles_after_each_failed_probe() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    let client = ResilientClient::builder()
        .circuit_breaker_growing(2, Duration::from_millis(20), Duration::from_millis(80))
        .build();
    let url = server.uri();
    let bg = CallPolicy::background();

    // Two failures open the breaker for 20 ms.
    for _ in 0..2 {
        assert_eq!(
            client.execute_with(&bg, |h| h.get(&url)).await.unwrap_err().kind,
            ErrorKind::Status(503)
        );
    }
    assert_eq!(
        client.execute_with(&bg, |h| h.get(&url)).await.unwrap_err().kind,
        ErrorKind::BreakerOpen
    );
    tokio::time::sleep(Duration::from_millis(25)).await;
    // Probe fails → open for 40 ms now.
    assert_eq!(
        client.execute_with(&bg, |h| h.get(&url)).await.unwrap_err().kind,
        ErrorKind::Status(503)
    );
    tokio::time::sleep(Duration::from_millis(25)).await;
    assert_eq!(
        client.execute_with(&bg, |h| h.get(&url)).await.unwrap_err().kind,
        ErrorKind::BreakerOpen,
        "still inside the doubled cooldown"
    );
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(
        client.execute_with(&bg, |h| h.get(&url)).await.unwrap_err().kind,
        ErrorKind::Status(503),
        "probe allowed after 40 ms"
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 4);
}

#[tokio::test]
async fn success_resets_the_cooldown() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(3)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let client = ResilientClient::builder()
        .circuit_breaker_growing(2, Duration::from_millis(10), Duration::from_millis(80))
        .build();
    let url = server.uri();
    let bg = CallPolicy::background();
    for _ in 0..2 {
        let _ = client.execute_with(&bg, |h| h.get(&url)).await;
    }
    tokio::time::sleep(Duration::from_millis(12)).await;
    let _ = client.execute_with(&bg, |h| h.get(&url)).await; // failed probe → 20 ms
    tokio::time::sleep(Duration::from_millis(22)).await;
    client
        .execute_with(&bg, |h| h.get(&url))
        .await
        .expect("probe succeeds");
    // Two fresh failures must open for the BASE cooldown again (10 ms), not 40.
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .with_priority(1)
        .mount(&server)
        .await;
    for _ in 0..2 {
        let _ = client.execute_with(&bg, |h| h.get(&url)).await;
    }
    tokio::time::sleep(Duration::from_millis(12)).await;
    assert_eq!(
        client.execute_with(&bg, |h| h.get(&url)).await.unwrap_err().kind,
        ErrorKind::Status(503),
        "a probe is allowed after the base cooldown"
    );
}

#[tokio::test]
async fn wait_for_probe_sleeps_through_the_cooldown_and_takes_the_probe() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(2)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let client = ResilientClient::builder()
        .circuit_breaker_growing(2, Duration::from_millis(30), Duration::from_millis(30))
        .build();
    let url = server.uri();
    let bg = CallPolicy::background();
    for _ in 0..2 {
        let _ = client.execute_with(&bg, |h| h.get(&url)).await;
    }
    let started = std::time::Instant::now();
    let resp = client
        .execute_with(&essential_fast(), |h| h.get(&url))
        .await
        .expect("waited for the probe slot and succeeded");
    assert_eq!(resp.status().as_u16(), 200);
    assert!(started.elapsed() >= Duration::from_millis(25), "did not fail fast");
    assert_eq!(server.received_requests().await.unwrap().len(), 3);
}

#[tokio::test]
async fn four_xx_does_not_count_toward_breaker() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let client = ResilientClient::builder()
        .circuit_breaker_growing(2, Duration::from_millis(20), Duration::from_millis(20))
        .build();
    let url = server.uri();
    let bg = CallPolicy::background();
    for _ in 0..3 {
        assert_eq!(
            client.execute_with(&bg, |h| h.get(&url)).await.unwrap_err().kind,
            ErrorKind::Status(404),
            "4xx must never trip the breaker"
        );
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 3);
}

#[tokio::test]
async fn probe_that_gets_404_closes_the_breaker() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(2)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let client = ResilientClient::builder()
        .circuit_breaker_growing(2, Duration::from_millis(10), Duration::from_millis(10))
        .build();
    let url = server.uri();
    let bg = CallPolicy::background();
    for _ in 0..2 {
        let _ = client.execute_with(&bg, |h| h.get(&url)).await;
    }
    tokio::time::sleep(Duration::from_millis(12)).await;
    assert_eq!(
        client.execute_with(&bg, |h| h.get(&url)).await.unwrap_err().kind,
        ErrorKind::Status(404),
        "the probe reaches the server"
    );
    assert_eq!(
        client.execute_with(&bg, |h| h.get(&url)).await.unwrap_err().kind,
        ErrorKind::Status(404),
        "the 404 probe closed the breaker; this call is not fast-failed"
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 4);
}

#[tokio::test]
async fn cancelled_probe_frees_the_slot_for_the_next_caller() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503).set_delay(Duration::from_millis(50)))
        .mount(&server)
        .await;
    let client = ResilientClient::builder()
        .circuit_breaker_growing(2, Duration::from_millis(10), Duration::from_millis(10))
        .build();
    let url = server.uri();
    let bg = CallPolicy::background();
    for _ in 0..2 {
        let _ = client.execute_with(&bg, |h| h.get(&url)).await;
    }
    tokio::time::sleep(Duration::from_millis(12)).await;
    let task = tokio::spawn({
        let client = client.clone();
        let url = url.clone();
        async move {
            let _ = client.execute_with(&essential_fast(), |h| h.get(&url)).await;
        }
    });
    tokio::time::sleep(Duration::from_millis(5)).await;
    task.abort();
    let _ = task.await;
    assert_eq!(
        client.execute_with(&bg, |h| h.get(&url)).await.unwrap_err().kind,
        ErrorKind::Status(503),
        "the cancelled probe's slot was freed for this call, not left stuck open"
    );
}

#[tokio::test]
async fn second_probe_in_one_call_still_excludes_other_callers() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503).set_delay(Duration::from_millis(30)))
        .mount(&server)
        .await;
    let client = ResilientClient::builder()
        .circuit_breaker_growing(2, Duration::from_millis(10), Duration::from_millis(10))
        .build();
    let url = server.uri();
    let bg = CallPolicy::background();
    for _ in 0..2 {
        let _ = client.execute_with(&bg, |h| h.get(&url)).await;
    }
    // The essential call now waits out the ~10ms cooldown, takes the first
    // probe (30ms, fails), waits out the re-opened cooldown, and takes a
    // second probe (another 30ms in flight): roughly a 50..80ms window,
    // measured from this spawn, in which that second probe is in flight.
    let task = tokio::spawn({
        let client = client.clone();
        let url = url.clone();
        async move {
            let _ = client.execute_with(&essential_fast(), |h| h.get(&url)).await;
        }
    });
    tokio::time::sleep(Duration::from_millis(75)).await;
    assert_eq!(
        client.execute_with(&bg, |h| h.get(&url)).await.unwrap_err().kind,
        ErrorKind::BreakerOpen,
        "the essential call's second probe is still in flight; only one probe at a time"
    );
    task.abort();
    let _ = task.await;
}

#[tokio::test]
async fn probe_that_gets_401_refreshes_and_continues_without_jamming() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(2)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(401))
        .up_to_n_times(1)
        .with_priority(2)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let calls = Arc::new(AtomicUsize::new(0));
    let calls2 = calls.clone();
    let refresher: AuthRefresher = Arc::new(move || {
        let calls = calls2.clone();
        Box::pin(async move {
            let n = calls.fetch_add(1, Ordering::SeqCst);
            Ok(format!("token-{n}"))
        })
    });

    let client = ResilientClient::builder()
        .circuit_breaker_growing(2, Duration::from_millis(10), Duration::from_millis(10))
        .bearer_auth(refresher)
        .build();
    let url = server.uri();
    let bg = CallPolicy::background();
    for _ in 0..2 {
        let _ = client.execute_with(&bg, |h| h.get(&url)).await;
    }
    tokio::time::sleep(Duration::from_millis(12)).await;
    let resp = client
        .execute_with(&essential_fast(), |h| h.get(&url))
        .await
        .expect("the 401 probe refreshes the token and continues to a 200");
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "one lazy acquire plus one refresh on the 401"
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 4);
}

use std::sync::Mutex;
use vantage_api_pool::resilient::{TransportEvent, TransportObserver};

#[derive(Default)]
struct Recorder(Mutex<Vec<(String, String)>>, Mutex<Vec<u64>>);

impl TransportObserver for Recorder {
    fn on_event(&self, key: &str, event: TransportEvent) {
        let tag = match event {
            TransportEvent::Started => "started".to_string(),
            TransportEvent::Succeeded { status, .. } => format!("ok:{status}"),
            TransportEvent::Failed { error, ms } => {
                // Recorded separately, in event order, for tests that need
                // to assert on the elapsed time of a specific `Failed`.
                self.1.lock().unwrap().push(ms);
                format!("failed:{}", error.kind_name())
            }
            TransportEvent::RetryScheduled { attempt, .. } => format!("retry:{attempt}"),
            TransportEvent::BreakerOpened { .. } => "opened".to_string(),
            TransportEvent::BreakerClosed => "closed".to_string(),
            TransportEvent::RowsPulled { n } => format!("rows:{n}"),
            TransportEvent::WritePushed => "write".to_string(),
        };
        self.0.lock().unwrap().push((key.to_string(), tag));
    }
}

#[tokio::test]
async fn observer_sees_every_attempt_retry_and_breaker_transition() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(2)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("abc"))
        .mount(&server)
        .await;
    let rec = Arc::new(Recorder::default());
    let client = ResilientClient::builder()
        .observer("local", rec.clone())
        .circuit_breaker_growing(2, Duration::from_millis(1), Duration::from_millis(1))
        .build();
    let url = server.uri();
    client
        .execute_with(&essential_fast(), |h| h.get(&url))
        .await
        .expect("third attempt succeeds");
    client.report(TransportEvent::RowsPulled { n: 7 });

    let seen: Vec<String> = rec
        .0
        .lock()
        .unwrap()
        .iter()
        .map(|(k, t)| {
            assert_eq!(k, "local");
            t.clone()
        })
        .collect();
    assert_eq!(
        seen,
        [
            "started", "failed:status", "retry:1",
            "started", "failed:status", "opened", "retry:2",
            "started", "ok:200", "closed",
            "rows:7",
        ]
    );
}

#[tokio::test]
async fn observer_pairs_the_401_attempt_with_a_failed_event() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(401))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let calls = Arc::new(AtomicUsize::new(0));
    let calls2 = calls.clone();
    let refresher: AuthRefresher = Arc::new(move || {
        let calls = calls2.clone();
        Box::pin(async move {
            let n = calls.fetch_add(1, Ordering::SeqCst);
            Ok(format!("token-{n}"))
        })
    });

    let rec = Arc::new(Recorder::default());
    let client = ResilientClient::builder()
        .observer("local", rec.clone())
        .bearer_auth(refresher)
        .build();
    let url = server.uri();
    client
        .execute(|h| h.get(&url))
        .await
        .expect("replays after the refresh");

    let seen: Vec<String> = rec.0.lock().unwrap().iter().map(|(_, t)| t.clone()).collect();
    assert_eq!(seen, ["started", "failed:status", "started", "ok:200"]);
    assert_eq!(calls.load(Ordering::SeqCst), 2, "one lazy acquire plus one refresh on the 401");
}

#[tokio::test]
async fn observer_reports_fail_fast_and_probe_closing_on_404() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let rec = Arc::new(Recorder::default());
    let client = ResilientClient::builder()
        .observer("local", rec.clone())
        .circuit_breaker_growing(1, Duration::from_millis(20), Duration::from_millis(20))
        .build();
    let url = server.uri();
    let bg = CallPolicy::background();

    // Call 1: the 503 opens the breaker (threshold 1).
    assert_eq!(
        client.execute_with(&bg, |h| h.get(&url)).await.unwrap_err().kind,
        ErrorKind::Status(503)
    );
    // Call 2: fails fast on the still-open breaker.
    assert_eq!(
        client.execute_with(&bg, |h| h.get(&url)).await.unwrap_err().kind,
        ErrorKind::BreakerOpen
    );
    tokio::time::sleep(Duration::from_millis(25)).await;
    // Call 3: the probe gets a 404, which closes the breaker.
    assert_eq!(
        client.execute_with(&bg, |h| h.get(&url)).await.unwrap_err().kind,
        ErrorKind::Status(404)
    );

    let seen: Vec<String> = rec.0.lock().unwrap().iter().map(|(_, t)| t.clone()).collect();
    assert_eq!(
        seen,
        [
            "started", "failed:status", "opened",
            "started", "failed:breaker_open",
            "started", "failed:status", "closed",
        ]
    );
    let failed_ms = rec.1.lock().unwrap();
    assert_eq!(failed_ms[1], 0, "the fail-fast Failed carries ms: 0");
}
