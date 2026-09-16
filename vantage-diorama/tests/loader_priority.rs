//! The loader, not the lens and not the transport, decides who is waiting:
//! a viewport nobody has cached is essential, a refresh over cached rows is
//! background, and the priority reaches the chunk callback through the
//! task-local even though the loader runs on its own task.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::{Priority, Result};
use vantage_diorama::Lens;
use vantage_types::Record;

mod support;
use support::MockView;
use support::chunk::master;

type Backend = Arc<Mutex<Vec<(String, Record<CborValue>)>>>;

fn rec(v: &str) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("v".to_string(), CborValue::Text(v.to_string()));
    r
}

fn rows(n: usize) -> Backend {
    Arc::new(Mutex::new(
        (0..n)
            .map(|i| (format!("r{i}"), rec(&format!("v{i}"))))
            .collect(),
    ))
}

/// A paged lens whose chunk callback records the priority it ran under.
fn observing_lens(
    cache: std::path::PathBuf,
    backend: Backend,
    seen: Arc<Mutex<Vec<Priority>>>,
) -> Arc<Lens> {
    let total = backend.clone();
    Arc::new(
        Lens::new()
            .cache_at(cache)
            .total_provider(move |_dio| {
                let b = total.clone();
                async move { Ok(b.lock().unwrap().len()) }
            })
            .on_load_chunk(move |_dio, range, _query, sink| {
                let b = backend.clone();
                let seen = seen.clone();
                async move {
                    seen.lock().unwrap().push(Priority::current());
                    let rows = b.lock().unwrap().clone();
                    for idx in range {
                        if let Some((id, r)) = rows.get(idx) {
                            sink.push(idx, id.clone(), r.clone()).await?;
                        }
                    }
                    Ok(())
                }
            })
            .build()
            .expect("lens"),
    )
}

#[tokio::test]
async fn cold_open_and_uncached_viewport_are_essential_refresh_is_background() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let seen: Arc<Mutex<Vec<Priority>>> = Arc::new(Mutex::new(Vec::new()));
    let lens = observing_lens(tmp.path().join("c.redb"), rows(30), seen.clone());
    let dio = lens.make_dio(master(&[("v", "String")])).await?;

    // Cold open: the cache is empty, so the on-open fetch is essential.
    let view = MockView::open(&dio, 10).await;
    view.settle_until("first page", |v| v.loaded_rows() >= 10)
        .await;
    assert_eq!(seen.lock().unwrap().as_slice(), [Priority::Essential]);

    // Scrolling into rows nobody has cached: essential.
    view.viewport(20..30);
    view.settle_until("second page", |v| v.col_at(25, "v").is_some())
        .await;
    assert_eq!(seen.lock().unwrap().last(), Some(&Priority::Essential));

    // A refresh over rows already on screen: background.
    view.scenery().request_refresh();
    let before = seen.lock().unwrap().len();
    view.settle_until("refresh ran", |_| seen.lock().unwrap().len() > before)
        .await;
    assert_eq!(seen.lock().unwrap().last(), Some(&Priority::Background));
    Ok(())
}

#[tokio::test]
async fn load_more_is_essential() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let seen: Arc<Mutex<Vec<Priority>>> = Arc::new(Mutex::new(Vec::new()));
    let lens = observing_lens(tmp.path().join("c.redb"), rows(30), seen.clone());
    let dio = lens.make_dio(master(&[("v", "String")])).await?;
    let view = MockView::open(&dio, 10).await;
    view.settle_until("first page", |v| v.loaded_rows() >= 10)
        .await;
    let before = seen.lock().unwrap().len();
    view.scenery().request_load_more();
    view.settle_until("load more ran", |_| seen.lock().unwrap().len() > before)
        .await;
    assert_eq!(seen.lock().unwrap().last(), Some(&Priority::Essential));
    Ok(())
}

#[tokio::test]
async fn warm_open_refresh_is_background() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let cache = tmp.path().join("c.redb");
    let backend = rows(30);
    {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let lens = observing_lens(cache.clone(), backend.clone(), seen);
        let dio = lens.make_dio(master(&[("v", "String")])).await?;
        let view = MockView::open(&dio, 10).await;
        view.settle_until("seeded", |v| v.loaded_rows() >= 10).await;
    }
    // Second open on the same cache file: rows are present, so the on-open
    // re-pull is a refresh, not a wait.
    let seen: Arc<Mutex<Vec<Priority>>> = Arc::new(Mutex::new(Vec::new()));
    let lens = observing_lens(cache, backend, seen.clone());
    let dio = lens.make_dio(master(&[("v", "String")])).await?;
    let view = MockView::open(&dio, 10).await;
    view.settle_until("re-pulled", |_| !seen.lock().unwrap().is_empty())
        .await;
    assert_eq!(seen.lock().unwrap().first(), Some(&Priority::Background));
    tokio::time::sleep(Duration::from_millis(10)).await;
    Ok(())
}

/// A lens whose chunk callback blocks on a gate until the test releases it,
/// and records whether its future was dropped before finishing.
struct Gated {
    release: Arc<tokio::sync::Notify>,
    calls: Arc<Mutex<Vec<std::ops::Range<usize>>>>,
    cancelled: Arc<std::sync::atomic::AtomicUsize>,
}

struct DropFlag(Arc<std::sync::atomic::AtomicUsize>, bool);
impl Drop for DropFlag {
    fn drop(&mut self) {
        if !self.1 {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }
}

fn gated_lens(cache: std::path::PathBuf, backend: Backend, gated: &Gated) -> Arc<Lens> {
    let total = backend.clone();
    let release = gated.release.clone();
    let calls = gated.calls.clone();
    let cancelled = gated.cancelled.clone();
    Arc::new(
        Lens::new()
            .cache_at(cache)
            .total_provider(move |_dio| {
                let b = total.clone();
                async move { Ok(b.lock().unwrap().len()) }
            })
            .on_load_chunk(move |_dio, range, _query, sink| {
                let b = backend.clone();
                let release = release.clone();
                let calls = calls.clone();
                let cancelled = cancelled.clone();
                async move {
                    calls.lock().unwrap().push(range.clone());
                    let mut flag = DropFlag(cancelled, false);
                    let rows = b.lock().unwrap().clone();
                    // Push the first three rows before blocking, so a test
                    // can observe them bound in the visible map while the
                    // rest of the range is still pending — and, if this
                    // call gets cancelled, check that those early rows were
                    // unbound rather than left behind.
                    let mut range = range;
                    let early: Vec<usize> = range.by_ref().take(3).collect();
                    for idx in early {
                        if let Some((id, r)) = rows.get(idx) {
                            sink.push(idx, id.clone(), r.clone()).await?;
                        }
                    }
                    release.notified().await;
                    for idx in range {
                        if let Some((id, r)) = rows.get(idx) {
                            sink.push(idx, id.clone(), r.clone()).await?;
                        }
                    }
                    flag.1 = true; // finished: not a cancellation
                    Ok(())
                }
            })
            .build()
            .expect("lens"),
    )
}

async fn calls_len(g: &Gated) -> usize {
    g.calls.lock().unwrap().len()
}

#[tokio::test]
async fn a_newer_viewport_cancels_the_in_flight_load() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let gated = Gated {
        release: Arc::new(tokio::sync::Notify::new()),
        calls: Arc::new(Mutex::new(Vec::new())),
        cancelled: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    };
    let lens = gated_lens(tmp.path().join("c.redb"), rows(40), &gated);
    let dio = lens.make_dio(master(&[("v", "String")])).await?;
    let scenery = dio.table_scenery().page_size(10).open().await?;

    // The on-open fetch is call 1; let it through.
    for _ in 0..200 {
        if calls_len(&gated).await >= 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    gated.release.notify_one();
    // Row 5 is only written by the remainder loop, after `notified()`
    // resolves — unlike row 0 (part of the early push), it can't already be
    // present before the release is actually consumed. Waiting on it (rather
    // than row 0) proves call 1 truly finished before the test moves on.
    for _ in 0..200 {
        if scenery.row(5).is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    // Scroll to 20..30: call 2 starts and blocks on the gate.
    scenery.set_viewport(20..30);
    for _ in 0..200 {
        if calls_len(&gated).await >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(gated.calls.lock().unwrap()[1], 20..30);

    // Scroll on to 30..40 while call 2 is still blocked: call 2 must be
    // dropped (not finished) and call 3 must start.
    scenery.set_viewport(30..40);
    for _ in 0..200 {
        if calls_len(&gated).await >= 3 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(gated.calls.lock().unwrap()[2], 30..40);
    assert_eq!(
        gated.cancelled.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "call 2 was dropped"
    );

    // Release call 3: rows 30..40 arrive; rows 20..30 never do.
    gated.release.notify_one();
    for _ in 0..200 {
        if scenery.row(35).is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(scenery.row(35).is_some());
    assert!(
        scenery.row(25).is_none(),
        "a cancelled load never writes rows"
    );
    assert!(
        scenery.row(20).is_none(),
        "the cancelled load's early-pushed rows were unbound, not left bound with nothing in the cache"
    );
    assert!(
        scenery.row(22).is_none(),
        "the cancelled load's early-pushed rows were unbound, not left bound with nothing in the cache"
    );

    // The in-flight marker was cleared by the cancellation: asking for
    // 20..30 again fetches it.
    scenery.set_viewport(20..30);
    for _ in 0..200 {
        if calls_len(&gated).await >= 4 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(gated.calls.lock().unwrap()[3], 20..30);
    gated.release.notify_one();
    for _ in 0..200 {
        if scenery.row(25).is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(scenery.row(25).is_some());
    Ok(())
}

#[tokio::test]
async fn an_identical_viewport_is_absorbed_into_the_running_load() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let gated = Gated {
        release: Arc::new(tokio::sync::Notify::new()),
        calls: Arc::new(Mutex::new(Vec::new())),
        cancelled: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    };
    let lens = gated_lens(tmp.path().join("c.redb"), rows(40), &gated);
    let dio = lens.make_dio(master(&[("v", "String")])).await?;
    let scenery = dio.table_scenery().page_size(10).open().await?;
    for _ in 0..200 {
        if calls_len(&gated).await >= 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    gated.release.notify_one();
    // Row 5 is only written by the remainder loop, after `notified()`
    // resolves — unlike row 0 (part of the early push), it can't already be
    // present before the release is actually consumed. Waiting on it (rather
    // than row 0) proves call 1 truly finished before the test moves on.
    for _ in 0..200 {
        if scenery.row(5).is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    scenery.set_viewport(20..30);
    for _ in 0..200 {
        if calls_len(&gated).await >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    // The same range again while it is loading: absorbed, no new call.
    scenery.set_viewport(20..30);
    // Longer than the default 50 ms debounce, so a would-be regression that
    // routed this through the debounce absorb loop instead of the mid-load
    // select would still have fired call 3 well before this check runs.
    tokio::time::sleep(Duration::from_millis(120)).await;
    assert_eq!(calls_len(&gated).await, 2);
    assert_eq!(gated.cancelled.load(std::sync::atomic::Ordering::SeqCst), 0);

    gated.release.notify_one();
    for _ in 0..200 {
        if scenery.row(25).is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(scenery.row(25).is_some());
    Ok(())
}

#[tokio::test]
async fn a_same_range_refresh_runs_after_the_current_load() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let gated = Gated {
        release: Arc::new(tokio::sync::Notify::new()),
        calls: Arc::new(Mutex::new(Vec::new())),
        cancelled: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    };
    let lens = gated_lens(tmp.path().join("c.redb"), rows(40), &gated);
    let dio = lens.make_dio(master(&[("v", "String")])).await?;
    let scenery = dio.table_scenery().page_size(10).open().await?;

    // The on-open fetch is call 1; let it through.
    for _ in 0..200 {
        if calls_len(&gated).await >= 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    gated.release.notify_one();
    // Row 5 is only written by the remainder loop, after `notified()`
    // resolves — unlike row 0 (part of the early push), it can't already be
    // present before the release is actually consumed. Waiting on it (rather
    // than row 0) proves call 1 truly finished before the test moves on.
    for _ in 0..200 {
        if scenery.row(5).is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    // Scroll to 20..30: call 2 starts and blocks on the gate. By the time
    // its callback has run at all, `last_viewport` is already 20..30 (it is
    // stamped before the callback is even dispatched), so the refresh below
    // targets this same block, not the one loaded before it.
    scenery.set_viewport(20..30);
    for _ in 0..200 {
        if calls_len(&gated).await >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(gated.calls.lock().unwrap()[1], 20..30);

    // A force_load refresh of the block currently loading: same range, so it
    // must park behind the running load rather than race or cancel it.
    scenery.request_refresh();
    tokio::time::sleep(Duration::from_millis(120)).await;
    assert_eq!(
        calls_len(&gated).await,
        2,
        "a same-range refresh must not preempt the load already in flight"
    );

    // Release call 2: its rows commit. The parked refresh then runs as call 3.
    gated.release.notify_one();
    for _ in 0..200 {
        if calls_len(&gated).await >= 3 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(
        gated.calls.lock().unwrap()[2],
        20..30,
        "the parked refresh re-asks for the block it was for"
    );
    gated.release.notify_one();
    for _ in 0..200 {
        if scenery.row(25).is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(scenery.row(25).is_some());
    assert!(scenery.row(5).is_some());
    Ok(())
}
