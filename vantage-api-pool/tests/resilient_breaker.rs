//! The circuit breaker end to end: the growing cooldown, the single half-open
//! probe, what does and does not count as a failure, and `breaker_state`.

mod support;

use std::time::Duration;

use support::{
    HalfFailing, background_error, background_ok, cancel, counting_refresher, essential_fast,
    local_client, mount, mount_delayed, mount_script, mount_status, open_breaker, requests,
    spawn_essential, within, within_secs,
};
use vantage_api_pool::resilient::{BreakerState, CallPolicy, ErrorKind};
use wiremock::MockServer;

#[tokio::test]
async fn breaker_state_reports_open_half_open_and_closed() {
    let server = MockServer::start().await;
    mount_script(&server, [503, 200]).await;

    assert_eq!(
        local_client().build().breaker_state(),
        None,
        "no breaker configured"
    );

    let client = local_client()
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
    assert_eq!(client.breaker_state(), Some(BreakerState::HalfOpen));

    background_ok(&client, &url).await;
    assert_eq!(client.breaker_state(), Some(BreakerState::Closed));
}

#[tokio::test]
async fn a_fast_failed_call_is_named_breaker_open() {
    let server = MockServer::start().await;
    mount_status(&server, 503).await;
    let client = local_client()
        .circuit_breaker(1, Duration::from_secs(60))
        .build();
    let url = server.uri();
    let _ = background_error(&client, &url).await;

    let err = client
        .execute_with(&CallPolicy::background(), |h| h.get(&url))
        .await
        .unwrap_err();

    assert_eq!(err.kind_name(), "breaker_open");
    assert_eq!(requests(&server).await, 1);
}

#[tokio::test]
async fn four_xx_does_not_count_toward_breaker() {
    let server = MockServer::start().await;
    mount_status(&server, 404).await;
    let client = local_client()
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
    let client = local_client()
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
async fn second_probe_in_one_call_still_excludes_other_callers() {
    let server = MockServer::start().await;
    mount_delayed(&server, 503, Duration::from_millis(200)).await;
    let client = local_client()
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
    let client = local_client()
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
    let client = local_client()
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
