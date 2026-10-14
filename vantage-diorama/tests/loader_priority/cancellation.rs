//! A newer request cancels a running load, an identical one is absorbed into
//! it, and a same-range one parks behind it — carrying its intent and its
//! wait onto whatever supersedes it.

use std::sync::atomic::Ordering;
use std::time::Duration;

use vantage_core::{Priority, Result};

use super::fixtures::{Gated, GatedScenery, rows, wait_until};

#[tokio::test]
async fn a_newer_viewport_cancels_the_in_flight_load() -> Result<()> {
    let gated = Gated::new();
    let open = GatedScenery::open(&gated, rows(40)).await?;
    let scenery = &open.scenery;

    // Scroll to 20..30: call 2 starts and blocks on the gate.
    scenery.set_viewport(20..30, 0..0);
    wait_until("the 20..30 load to start", || gated.calls_len() >= 2).await;
    assert_eq!(gated.calls.lock().unwrap()[1], 20..30);

    // Scroll on to 30..40 while call 2 is still blocked: call 2 must be
    // dropped (not finished) and call 3 must start.
    scenery.set_viewport(30..40, 0..0);
    wait_until("the 30..40 load to start", || gated.calls_len() >= 3).await;
    assert_eq!(gated.calls.lock().unwrap()[2], 30..40);
    assert_eq!(
        gated.cancelled.load(Ordering::SeqCst),
        1,
        "call 2 was dropped"
    );

    // Release call 3: rows 30..40 arrive; rows 20..30 never do.
    gated.release.notify_one();
    wait_until("row 35", || scenery.row(35).is_some()).await;
    assert!(
        scenery.row(25).is_none(),
        "a cancelled load never writes rows"
    );
    assert!(
        scenery.row(20).is_none(),
        "the cancelled load's early-pushed rows went back to empty, not left bound with nothing in the cache"
    );
    assert!(
        scenery.row(22).is_none(),
        "the cancelled load's early-pushed rows went back to empty, not left bound with nothing in the cache"
    );

    // The in-flight marker was cleared by the cancellation: asking for
    // 20..30 again fetches it.
    scenery.set_viewport(20..30, 0..0);
    wait_until("20..30 to be re-requested", || gated.calls_len() >= 4).await;
    assert_eq!(gated.calls.lock().unwrap()[3], 20..30);
    gated.release.notify_one();
    wait_until("row 25", || scenery.row(25).is_some()).await;
    Ok(())
}

#[tokio::test]
async fn an_identical_viewport_is_absorbed_into_the_running_load() -> Result<()> {
    let gated = Gated::new();
    let open = GatedScenery::open(&gated, rows(40)).await?;
    let scenery = &open.scenery;

    scenery.set_viewport(20..30, 0..0);
    wait_until("the 20..30 load to start", || gated.calls_len() >= 2).await;
    // The same range again while it is loading: absorbed, no new call.
    scenery.set_viewport(20..30, 0..0);
    // Longer than the default 50 ms debounce, so a would-be regression that
    // routed this through the debounce absorb loop instead of the mid-load
    // select would still have fired call 3 well before this check runs.
    tokio::time::sleep(Duration::from_millis(120)).await;
    assert_eq!(gated.calls_len(), 2);
    assert_eq!(gated.cancelled.load(Ordering::SeqCst), 0);

    gated.release.notify_one();
    wait_until("row 25", || scenery.row(25).is_some()).await;
    Ok(())
}

#[tokio::test]
async fn a_same_range_refresh_runs_after_the_current_load() -> Result<()> {
    let gated = Gated::new();
    let open = GatedScenery::open(&gated, rows(40)).await?;
    let scenery = &open.scenery;

    // Scroll to 20..30: call 2 starts and blocks on the gate. By the time
    // its callback has run at all, the view's viewport is already 20..30 (it
    // is stamped before the callback is even dispatched), so the refresh below
    // targets this same block, not the one loaded before it.
    scenery.set_viewport(20..30, 0..0);
    wait_until("the 20..30 load to start", || gated.calls_len() >= 2).await;
    assert_eq!(gated.calls.lock().unwrap()[1], 20..30);

    // A force_load refresh of the block currently loading: same range, so it
    // must park behind the running load rather than race or cancel it.
    scenery.refresh_source();
    tokio::time::sleep(Duration::from_millis(120)).await;
    assert_eq!(
        gated.calls_len(),
        2,
        "a same-range refresh must not preempt the load already in flight"
    );

    // Release call 2: its rows commit. The parked refresh then runs as call 3.
    gated.release.notify_one();
    wait_until("the parked refresh to run", || gated.calls_len() >= 3).await;
    assert_eq!(
        gated.calls.lock().unwrap()[2],
        20..30,
        "the parked refresh re-asks for the block it was for"
    );
    gated.release.notify_one();
    wait_until("row 25", || scenery.row(25).is_some()).await;
    assert!(scenery.row(5).is_some());
    Ok(())
}

/// The parked refresh's *intent* — consult the master, don't trust the cache
/// — outlives the range it was for: it is OR'd onto whatever request
/// supersedes it. Proven with a superseding range the cache already holds in
/// full, which without the carried `force_load` would fetch nothing at all.
#[tokio::test]
async fn a_parked_refresh_survives_a_supersede() -> Result<()> {
    let gated = Gated::new();
    let open = GatedScenery::open(&gated, rows(40)).await?;
    let scenery = &open.scenery;

    // Call 2 for 20..30 blocks on the gate; the refresh behind it parks.
    scenery.set_viewport(20..30, 0..0);
    wait_until("the 20..30 load to start", || gated.calls_len() >= 2).await;
    scenery.refresh_source();
    tokio::time::sleep(Duration::from_millis(120)).await;
    assert_eq!(gated.calls_len(), 2, "the refresh parked behind the load");

    // Supersede both with 0..10 — fully cached by call 1. A plain viewport
    // for it would be served locally and never reach the callback; call 3
    // happening at all is the parked refresh's `force_load` arriving on it.
    scenery.set_viewport(0..10, 0..0);
    wait_until("the superseding load to run", || gated.calls_len() >= 3).await;
    assert_eq!(
        gated.calls.lock().unwrap()[2],
        0..10,
        "the superseding range was re-pulled from the master although the cache held it",
    );
    gated.release.notify_one();
    Ok(())
}

/// The other half of the same fold: the parked request's *wait* rides onto
/// the superseding one too. A `request_load_more` someone is waiting on,
/// parked behind a running load and then overtaken by the background refresh
/// poll, must not come out of the merge as a poll.
#[tokio::test]
async fn a_parked_requests_priority_survives_a_supersede() -> Result<()> {
    let mut gated = Gated::new();
    // No early push: an early-bound row would move `next_load_more_start`
    // past the range this test needs `request_load_more` to ask for.
    gated.early_push = 0;
    let open = GatedScenery::open(&gated, rows(40)).await?;
    let scenery = &open.scenery;

    // Call 2 for 10..20 blocks on the gate, having bound nothing.
    scenery.set_viewport(10..20, 0..0);
    wait_until("the 10..20 load to start", || gated.calls_len() >= 2).await;
    assert_eq!(gated.calls.lock().unwrap()[1], 10..20);

    // Rows 0..9 are cached and nothing above them is, so load-more asks for
    // exactly 10..20 — the range already in flight. Essential, `force_load`:
    // it parks.
    scenery.request_load_more();
    tokio::time::sleep(Duration::from_millis(120)).await;
    assert_eq!(gated.calls_len(), 2, "load-more parked behind the load");

    // The refresh poll then supersedes both: its contiguous-block expansion
    // reaches back over the cached rows 0..9, so it asks for 0..20 — a
    // different range, and a background one.
    scenery.refresh_source();
    wait_until("the superseding load to run", || gated.calls_len() >= 3).await;
    assert_eq!(gated.calls.lock().unwrap()[2], 0..20);
    assert_eq!(
        gated.priority_of_call(2),
        Some(Priority::Essential),
        "the parked load-more's wait rode onto the request that superseded it",
    );
    gated.release.notify_one();
    Ok(())
}
