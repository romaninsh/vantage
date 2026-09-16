//! The circuit breaker end to end: the growing cooldown, the single half-open
//! probe, what does and does not count as a failure, and `breaker_state`.

mod support;

use std::time::Duration;

use support::{
    background_error, background_ok, cancel, counting_refresher, essential_fast, mount,
    mount_delayed, mount_script, mount_status, open_breaker, requests, spawn_essential,
    wait_for_requests, within, within_secs, HalfFailing,
};
use vantage_api_pool::resilient::{BreakerState, ErrorKind};
use vantage_api_pool::ResilientClient;
use wiremock::MockServer;

#[tokio::test]
async fn cooldown_doubles_after_each_failed_probe() {
    let server = MockServer::start().await;
    mount_status(&server, 503).await;
    let client = ResilientClient::builder()
        .circuit_breaker_growing(2, Duration::from_millis(200), Duration::from_millis(800))
        .build();
    let url = server.uri();

    // Two failures open the breaker for 200 ms.
    for _ in 0..2 {
        assert_eq!(
            background_error(&client, &url).await,
            ErrorKind::Status(503)
        );
    }
    assert_eq!(
        background_error(&client, &url).await,
        ErrorKind::BreakerOpen
    );
    tokio::time::sleep(Duration::from_millis(250)).await;
    // Probe fails → open for 400 ms now.
    assert_eq!(
        background_error(&client, &url).await,
        ErrorKind::Status(503)
    );
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(
        background_error(&client, &url).await,
        ErrorKind::BreakerOpen,
        "still inside the doubled cooldown"
    );
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        background_error(&client, &url).await,
        ErrorKind::Status(503),
        "probe allowed after 400 ms"
    );
    assert_eq!(requests(&server).await, 4);
}

#[tokio::test]
async fn success_resets_the_cooldown() {
    let server = MockServer::start().await;
    // Two failures, a failed probe, a succeeding probe, then two fresh
    // failures and the probe that must be let through after the BASE cooldown.
    mount_script(&server, [503, 503, 503, 200, 503, 503, 503]).await;
    let client = ResilientClient::builder()
        .circuit_breaker_growing(2, Duration::from_millis(10), Duration::from_millis(80))
        .build();
    let url = server.uri();
    open_breaker(&client, &url, 2).await;
    tokio::time::sleep(Duration::from_millis(12)).await;
    let _ = background_error(&client, &url).await; // failed probe → 20 ms
    tokio::time::sleep(Duration::from_millis(22)).await;
    background_ok(&client, &url).await;

    // Two fresh failures must open for the base cooldown again (10 ms), not 40.
    open_breaker(&client, &url, 2).await;
    tokio::time::sleep(Duration::from_millis(12)).await;
    assert_eq!(
        background_error(&client, &url).await,
        ErrorKind::Status(503),
        "a probe is allowed after the base cooldown"
    );
}

#[tokio::test]
async fn wait_for_probe_sleeps_through_the_cooldown_and_takes_the_probe() {
    let server = MockServer::start().await;
    mount_script(&server, [503, 503, 200]).await;
    let client = ResilientClient::builder()
        .circuit_breaker_growing(2, Duration::from_millis(30), Duration::from_millis(30))
        .build();
    let url = server.uri();
    open_breaker(&client, &url, 2).await;
    let started = std::time::Instant::now();
    let resp = within(client.execute_with(&essential_fast(), |h| h.get(&url)))
        .await
        .expect("waited for the probe slot and succeeded");
    assert_eq!(resp.status().as_u16(), 200);
    assert!(
        started.elapsed() >= Duration::from_millis(25),
        "did not fail fast"
    );
    assert_eq!(requests(&server).await, 3);
}

#[tokio::test]
async fn four_xx_does_not_count_toward_breaker() {
    let server = MockServer::start().await;
    mount_status(&server, 404).await;
    let client = ResilientClient::builder()
        .circuit_breaker_growing(2, Duration::from_millis(20), Duration::from_millis(20))
        .build();
    let url = server.uri();
    for _ in 0..3 {
        assert_eq!(
            background_error(&client, &url).await,
            ErrorKind::Status(404),
            "4xx must never trip the breaker"
        );
    }
    assert_eq!(requests(&server).await, 3);
    assert_eq!(client.breaker_state(), Some(BreakerState::Closed));
}

#[tokio::test]
async fn interleaved_4xx_does_not_stop_the_breaker_opening() {
    let server = MockServer::start().await;
    mount_script(&server, [503, 404, 503, 404, 503]).await;
    let client = ResilientClient::builder()
        .circuit_breaker_growing(3, Duration::from_secs(30), Duration::from_secs(30))
        .build();
    let url = server.uri();

    // A 404 proves the server answers, so it closes an open breaker — but it
    // is not a success, so it must not clear the run of 5xx.
    for expected in [503u16, 404, 503, 404, 503] {
        assert_eq!(
            background_error(&client, &url).await,
            ErrorKind::Status(expected)
        );
    }
    assert!(
        matches!(client.breaker_state(), Some(BreakerState::Open { .. })),
        "the third 5xx reaches the threshold of 3"
    );
    assert_eq!(
        background_error(&client, &url).await,
        ErrorKind::BreakerOpen
    );
    assert_eq!(requests(&server).await, 5);
}

#[tokio::test]
async fn breaker_state_reports_open_half_open_and_closed() {
    let server = MockServer::start().await;
    mount_script(&server, [503, 200]).await;

    assert_eq!(
        ResilientClient::builder().build().breaker_state(),
        None,
        "no breaker configured"
    );

    let client = ResilientClient::builder()
        .circuit_breaker(1, Duration::from_millis(60))
        .build();
    let url = server.uri();
    assert_eq!(client.breaker_state(), Some(BreakerState::Closed));

    let _ = background_error(&client, &url).await;
    assert!(matches!(
        client.breaker_state(),
        Some(BreakerState::Open { .. })
    ));

    tokio::time::sleep(Duration::from_millis(70)).await;
    assert_eq!(
        client.breaker_state(),
        Some(BreakerState::HalfOpen),
        "the cooldown has elapsed; one caller may probe"
    );

    background_ok(&client, &url).await;
    assert_eq!(client.breaker_state(), Some(BreakerState::Closed));
}

#[tokio::test]
async fn probe_that_gets_404_closes_the_breaker() {
    let server = MockServer::start().await;
    mount_script(&server, [503, 503, 404]).await;
    let client = ResilientClient::builder()
        .circuit_breaker_growing(2, Duration::from_millis(10), Duration::from_millis(10))
        .build();
    let url = server.uri();
    open_breaker(&client, &url, 2).await;
    tokio::time::sleep(Duration::from_millis(12)).await;
    assert_eq!(
        background_error(&client, &url).await,
        ErrorKind::Status(404),
        "the probe reaches the server"
    );
    assert_eq!(
        background_error(&client, &url).await,
        ErrorKind::Status(404),
        "the 404 probe closed the breaker; this call is not fast-failed"
    );
    assert_eq!(requests(&server).await, 4);
}

#[tokio::test]
async fn cancelled_probe_frees_the_slot_for_the_next_caller() {
    let server = MockServer::start().await;
    mount_delayed(&server, 503, Duration::from_millis(200)).await;
    let client = ResilientClient::builder()
        .circuit_breaker_growing(2, Duration::from_millis(10), Duration::from_millis(10))
        .build();
    let url = server.uri();
    open_breaker(&client, &url, 2).await;
    tokio::time::sleep(Duration::from_millis(12)).await;
    let task = spawn_essential(&client, &url);
    // Cancel only once the probe request is demonstrably in flight, so the
    // assertion below cannot pass vacuously.
    wait_for_requests(&server, 3, Duration::from_secs(2)).await;
    cancel(task).await;
    assert_eq!(
        background_error(&client, &url).await,
        ErrorKind::Status(503),
        "the cancelled probe's slot was freed for this call, not left stuck open"
    );
    assert_eq!(requests(&server).await, 4);
}

#[tokio::test]
async fn second_probe_in_one_call_still_excludes_other_callers() {
    let server = MockServer::start().await;
    mount_delayed(&server, 503, Duration::from_millis(200)).await;
    let client = ResilientClient::builder()
        .circuit_breaker_growing(2, Duration::from_millis(20), Duration::from_millis(20))
        .build();
    let url = server.uri();
    open_breaker(&client, &url, 2).await;
    // The essential call waits out the 20 ms cooldown, takes the first probe
    // (200 ms in flight, fails), waits out the re-opened 20 ms, and takes a
    // second probe: roughly a 240..440 ms window, measured from this spawn,
    // in which that second probe is in flight.
    let task = spawn_essential(&client, &url);
    tokio::time::sleep(Duration::from_millis(320)).await;
    assert_eq!(
        background_error(&client, &url).await,
        ErrorKind::BreakerOpen,
        "the essential call's second probe is still in flight; only one probe at a time"
    );
    cancel(task).await;
}

#[tokio::test]
async fn probe_that_gets_401_refreshes_and_continues_without_jamming() {
    let server = MockServer::start().await;
    mount_script(&server, [503, 503, 401, 200]).await;

    let (refresher, calls) = counting_refresher();
    let client = ResilientClient::builder()
        .circuit_breaker_growing(2, Duration::from_millis(10), Duration::from_millis(10))
        .bearer_auth(refresher)
        .build();
    let url = server.uri();
    open_breaker(&client, &url, 2).await;
    tokio::time::sleep(Duration::from_millis(12)).await;
    let resp = within(client.execute_with(&essential_fast(), |h| h.get(&url)))
        .await
        .expect("the 401 probe refreshes the token and continues to a 200");
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(
        calls.count(),
        2,
        "one lazy acquire plus one refresh on the 401"
    );
    assert_eq!(requests(&server).await, 4);
}

/// Twenty callers on four worker threads against a server that fails every
/// other request, with a parallelism cap of two and a breaker that keeps
/// opening: every call must still get through, and the breaker must be closed
/// when the dust settles.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_callers_all_recover_from_a_half_failing_server() {
    let server = MockServer::start().await;
    mount(&server, HalfFailing::default()).await;
    let client = ResilientClient::builder()
        .max_parallel(2)
        .circuit_breaker_growing(3, Duration::from_millis(20), Duration::from_millis(20))
        .build();
    let url = server.uri();

    let mut handles = Vec::new();
    for _ in 0..20 {
        let client = client.clone();
        let url = url.clone();
        handles.push(tokio::spawn(async move {
            within_secs(
                10,
                client.execute_with(&essential_fast(), move |h| h.get(&url)),
            )
            .await
            .map(|r| r.status().as_u16())
        }));
    }
    for handle in handles {
        assert_eq!(
            handle
                .await
                .unwrap()
                .expect("every essential call succeeds"),
            200
        );
    }

    let peak = client.peak_in_flight();
    assert!(peak <= 2, "parallelism cap exceeded: peak {peak}");
    assert_eq!(client.in_flight(), 0);
    assert_eq!(
        client.breaker_state(),
        Some(BreakerState::Closed),
        "the last thing each call did was succeed"
    );
}
