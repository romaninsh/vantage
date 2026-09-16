//! The loader, not the lens and not the transport, decides who is waiting:
//! a viewport nobody has cached is essential, a poll over cached rows is
//! background, and the priority reaches the chunk callback through the
//! task-local even though the loader runs on its own task.
//!
//! The same file covers what a load leaves behind when it does not finish:
//! a cancelled or failed load must restore the visible map to what it was,
//! not delete the rows it happened to overwrite.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::{Priority, Result};
use vantage_diorama::{ChunkSink, Lens, LensBuilder};
use vantage_types::Record;

mod support;
use support::MockView;
use support::chunk::{col_at, master};

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

/// Poll `pred` every 5ms until it holds; panic naming `label` if two seconds
/// pass first. The budget bounds a *hang* — it is not an assertion about
/// speed, which is why it is generous — and the label is what turns a timeout
/// into a diagnosis rather than a stack trace in a loop.
async fn wait_until(label: &str, mut pred: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !pred() {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out after 2s waiting for: {label}",
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// Serve the rows of `range` that `snapshot` holds — the body every chunk
/// callback in this file shares. It takes a snapshot rather than the backend
/// so a gated callback reads the same rows either side of its gate.
async fn push_rows(
    snapshot: &[(String, Record<CborValue>)],
    range: impl IntoIterator<Item = usize>,
    sink: &ChunkSink,
) -> Result<()> {
    for idx in range {
        if let Some((id, r)) = snapshot.get(idx) {
            sink.push(idx, id.clone(), r.clone()).await?;
        }
    }
    Ok(())
}

/// The `total_provider` every lens here that states a total states: the
/// backend's current length.
fn with_backend_total(lens: LensBuilder, backend: &Backend) -> LensBuilder {
    let backend = backend.clone();
    lens.total_provider(move |_dio| {
        let backend = backend.clone();
        async move { Ok(backend.lock().unwrap().len()) }
    })
}

/// A paged lens whose chunk callback records the priority it ran under.
/// `total_provider` and `debounce` are separate knobs because two rules only
/// show themselves without one or with a longer other: the horizon probe
/// needs a source that never states a total, and the coalescing rule needs a
/// debounce window wide enough to put two requests into by hand.
fn observing_lens(
    cache: std::path::PathBuf,
    backend: Backend,
    seen: Arc<Mutex<Vec<Priority>>>,
) -> Arc<Lens> {
    build_observing_lens(cache, backend, seen, true, None)
}

fn build_observing_lens(
    cache: std::path::PathBuf,
    backend: Backend,
    seen: Arc<Mutex<Vec<Priority>>>,
    with_total: bool,
    debounce: Option<Duration>,
) -> Arc<Lens> {
    let mut lens = Lens::new().cache_at(cache);
    if with_total {
        lens = with_backend_total(lens, &backend);
    }
    if let Some(debounce) = debounce {
        lens = lens.viewport_debounce(debounce);
    }
    Arc::new(
        lens.on_load_chunk(move |_dio, range, _query, sink| {
            let b = backend.clone();
            let seen = seen.clone();
            async move {
                seen.lock().unwrap().push(Priority::current());
                let rows = b.lock().unwrap().clone();
                push_rows(&rows, range, &sink).await
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
    wait_until("refresh ran", || seen.lock().unwrap().len() > before).await;
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
    wait_until("load more ran", || seen.lock().unwrap().len() > before).await;
    assert_eq!(seen.lock().unwrap().last(), Some(&Priority::Essential));
    Ok(())
}

/// A search is a `force_load` re-pull of a block already on screen — and a
/// user waiting on it. `force_load` is not what decides the priority.
#[tokio::test]
async fn a_search_refetch_is_essential() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let seen: Arc<Mutex<Vec<Priority>>> = Arc::new(Mutex::new(Vec::new()));
    let lens = observing_lens(tmp.path().join("c.redb"), rows(30), seen.clone());
    let dio = lens.make_dio(master(&[("v", "String")])).await?;
    let view = MockView::open(&dio, 10).await;
    view.settle_until("first page", |v| v.loaded_rows() >= 10)
        .await;

    let before = seen.lock().unwrap().len();
    view.scenery().set_search(Some("v2".into()));
    wait_until("the search refetch ran", || {
        seen.lock().unwrap().len() > before
    })
    .await;
    assert_eq!(seen.lock().unwrap().last(), Some(&Priority::Essential));
    Ok(())
}

/// The grand total the open blocks on is awaited by whoever is mounting the
/// page, so it is essential — a single failure on it fails the open.
#[tokio::test]
async fn the_open_blocking_total_is_essential() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let backend = rows(30);
    let counted: Arc<Mutex<Vec<Priority>>> = Arc::new(Mutex::new(Vec::new()));
    let lens = {
        let total = backend.clone();
        let observed = counted.clone();
        Arc::new(
            Lens::new()
                .cache_at(tmp.path().join("c.redb"))
                .total_provider(move |_dio| {
                    let b = total.clone();
                    let observed = observed.clone();
                    async move {
                        observed.lock().unwrap().push(Priority::current());
                        Ok(b.lock().unwrap().len())
                    }
                })
                .on_load_chunk(move |_dio, range, _query, sink| {
                    let b = backend.clone();
                    async move {
                        let rows = b.lock().unwrap().clone();
                        push_rows(&rows, range, &sink).await
                    }
                })
                .build()
                .expect("lens"),
        )
    };
    let dio = lens.make_dio(master(&[("v", "String")])).await?;
    let _view = MockView::open(&dio, 10).await;
    assert_eq!(counted.lock().unwrap().first(), Some(&Priority::Essential));
    Ok(())
}

/// The probe past the end the loader itself inferred is speculative: nobody
/// asked for those rows, so it must not hold a retry budget open.
#[tokio::test]
async fn the_horizon_probe_runs_background() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let seen: Arc<Mutex<Vec<Priority>>> = Arc::new(Mutex::new(Vec::new()));
    // No `total_provider`: nothing ever states a total, so the end of the set
    // is only ever this side's inference — which is what makes a viewport
    // reaching it a probe rather than a fetch.
    let lens = build_observing_lens(
        tmp.path().join("c.redb"),
        rows(30),
        seen.clone(),
        false,
        None,
    );
    let dio = lens.make_dio(master(&[("v", "String")])).await?;
    let view = MockView::open(&dio, 10).await;
    view.settle_until("first page", |v| v.loaded_rows() >= 10)
        .await;
    // The cold on-open fetch is a real wait; the inferred horizon is now 20.
    assert_eq!(seen.lock().unwrap().as_slice(), [Priority::Essential]);
    assert_eq!(view.total(), Some(20));

    // A viewport pressed against that inferred end asks past it on the user's
    // behalf, but the rows beyond it are not known to exist.
    let before = seen.lock().unwrap().len();
    view.viewport(10..20);
    wait_until("the horizon probe ran", || {
        seen.lock().unwrap().len() > before
    })
    .await;
    assert_eq!(seen.lock().unwrap().last(), Some(&Priority::Background));
    Ok(())
}

/// Coalescing merges the *wait*, not just the range: an essential viewport
/// absorbed together with a background refresh still has someone waiting on
/// the rows.
#[tokio::test]
async fn coalescing_a_refresh_into_a_viewport_keeps_essential() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let seen: Arc<Mutex<Vec<Priority>>> = Arc::new(Mutex::new(Vec::new()));
    // A wide debounce so both requests land in the same window by hand
    // rather than by luck.
    let lens = build_observing_lens(
        tmp.path().join("c.redb"),
        rows(30),
        seen.clone(),
        true,
        Some(Duration::from_millis(400)),
    );
    let dio = lens.make_dio(master(&[("v", "String")])).await?;
    let view = MockView::open(&dio, 10).await;
    view.settle_until("first page", |v| v.loaded_rows() >= 10)
        .await;
    let before = seen.lock().unwrap().len();

    // An essential viewport over the block already on screen — on its own it
    // is fully cached and fetches nothing — then, inside the debounce, the
    // background refresh of the same block, which supplies the `force_load`.
    // One fetch comes out of the two, and it is the one someone is waiting on.
    view.viewport(0..10);
    view.scenery().request_refresh();
    wait_until("the coalesced fetch ran", || {
        seen.lock().unwrap().len() > before
    })
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
    let _view = MockView::open(&dio, 10).await;
    wait_until("re-pulled", || !seen.lock().unwrap().is_empty()).await;
    assert_eq!(seen.lock().unwrap().first(), Some(&Priority::Background));
    Ok(())
}

/// A lens whose chunk callback blocks on a gate until the test releases it,
/// and records what it ran as, which ranges it was asked for, and whether
/// its future was dropped before finishing.
struct Gated {
    release: Arc<tokio::sync::Notify>,
    calls: Arc<Mutex<Vec<std::ops::Range<usize>>>>,
    priorities: Arc<Mutex<Vec<Priority>>>,
    cancelled: Arc<AtomicUsize>,
    /// How many rows the callback pushes before it blocks. Enough by default
    /// that a test can change one of them behind the loader's back and watch
    /// what a cancellation does to it; zero when a test needs the in-flight
    /// load to leave the visible map strictly alone (an early-pushed row
    /// moves `next_load_more_start`, and one test depends on where that
    /// lands).
    early_push: usize,
}

impl Gated {
    fn new() -> Self {
        Self {
            release: Arc::new(tokio::sync::Notify::new()),
            calls: Arc::new(Mutex::new(Vec::new())),
            priorities: Arc::new(Mutex::new(Vec::new())),
            cancelled: Arc::new(AtomicUsize::new(0)),
            early_push: 4,
        }
    }

    fn priority_of_call(&self, n: usize) -> Option<Priority> {
        self.priorities.lock().unwrap().get(n).copied()
    }
}

struct DropFlag(Arc<AtomicUsize>, bool);
impl Drop for DropFlag {
    fn drop(&mut self) {
        if !self.1 {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
}

fn gated_lens(cache: std::path::PathBuf, backend: Backend, gated: &Gated) -> Arc<Lens> {
    gated_lens_from(Lens::new().cache_at(cache), backend, gated)
}

/// [`gated_lens`] plus a write route that always fails. A failing flash ends
/// in `WriteReverted`, which the reactor turns into a `mark_row` — the one
/// id-keyed operation a test can reach through the public surface, and
/// therefore how `id_to_idx` is observed from outside.
fn gated_lens_rejecting_writes(
    cache: std::path::PathBuf,
    backend: Backend,
    gated: &Gated,
) -> Arc<Lens> {
    gated_lens_from(
        Lens::new()
            .cache_at(cache)
            .on_flash(|_dio, _flash| async move { Err(vantage_core::error!("write route fails")) }),
        backend,
        gated,
    )
}

fn gated_lens_from(lens: LensBuilder, backend: Backend, gated: &Gated) -> Arc<Lens> {
    let lens = with_backend_total(lens, &backend);
    let release = gated.release.clone();
    let calls = gated.calls.clone();
    let priorities = gated.priorities.clone();
    let cancelled = gated.cancelled.clone();
    let early_push = gated.early_push;
    Arc::new(
        lens.on_load_chunk(move |_dio, range, _query, sink| {
            let b = backend.clone();
            let release = release.clone();
            let calls = calls.clone();
            let priorities = priorities.clone();
            let cancelled = cancelled.clone();
            async move {
                calls.lock().unwrap().push(range.clone());
                priorities.lock().unwrap().push(Priority::current());
                let mut flag = DropFlag(cancelled, false);
                let rows = b.lock().unwrap().clone();
                // Push the first few rows before blocking, so a test can
                // observe them bound in the visible map while the rest of
                // the range is still pending — and, if this call gets
                // cancelled, check what happened to them.
                let mut range = range;
                push_rows(&rows, range.by_ref().take(early_push), &sink).await?;
                release.notified().await;
                push_rows(&rows, range, &sink).await?;
                flag.1 = true; // finished: not a cancellation
                Ok(())
            }
        })
        .build()
        .expect("lens"),
    )
}

fn calls_len(g: &Gated) -> usize {
    g.calls.lock().unwrap().len()
}

/// Let the on-open fetch (call 1) through and wait for a row only the
/// remainder loop writes — unlike an early-pushed row, row 5 cannot already
/// be present before the release is actually consumed, so waiting on it
/// proves call 1 finished rather than merely started.
async fn release_the_open_fetch(gated: &Gated, scenery: &Arc<dyn vantage_diorama::TableScenery>) {
    wait_until("the on-open fetch to start", || calls_len(gated) >= 1).await;
    gated.release.notify_one();
    wait_until("the on-open fetch to finish", || scenery.row(5).is_some()).await;
}

#[tokio::test]
async fn a_newer_viewport_cancels_the_in_flight_load() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let gated = Gated::new();
    let lens = gated_lens(tmp.path().join("c.redb"), rows(40), &gated);
    let dio = lens.make_dio(master(&[("v", "String")])).await?;
    let scenery = dio.table_scenery().page_size(10).open().await?;
    release_the_open_fetch(&gated, &scenery).await;

    // Scroll to 20..30: call 2 starts and blocks on the gate.
    scenery.set_viewport(20..30);
    wait_until("the 20..30 load to start", || calls_len(&gated) >= 2).await;
    assert_eq!(gated.calls.lock().unwrap()[1], 20..30);

    // Scroll on to 30..40 while call 2 is still blocked: call 2 must be
    // dropped (not finished) and call 3 must start.
    scenery.set_viewport(30..40);
    wait_until("the 30..40 load to start", || calls_len(&gated) >= 3).await;
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
    scenery.set_viewport(20..30);
    wait_until("20..30 to be re-requested", || calls_len(&gated) >= 4).await;
    assert_eq!(gated.calls.lock().unwrap()[3], 20..30);
    gated.release.notify_one();
    wait_until("row 25", || scenery.row(25).is_some()).await;
    Ok(())
}

#[tokio::test]
async fn an_identical_viewport_is_absorbed_into_the_running_load() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let gated = Gated::new();
    let lens = gated_lens(tmp.path().join("c.redb"), rows(40), &gated);
    let dio = lens.make_dio(master(&[("v", "String")])).await?;
    let scenery = dio.table_scenery().page_size(10).open().await?;
    release_the_open_fetch(&gated, &scenery).await;

    scenery.set_viewport(20..30);
    wait_until("the 20..30 load to start", || calls_len(&gated) >= 2).await;
    // The same range again while it is loading: absorbed, no new call.
    scenery.set_viewport(20..30);
    // Longer than the default 50 ms debounce, so a would-be regression that
    // routed this through the debounce absorb loop instead of the mid-load
    // select would still have fired call 3 well before this check runs.
    tokio::time::sleep(Duration::from_millis(120)).await;
    assert_eq!(calls_len(&gated), 2);
    assert_eq!(gated.cancelled.load(Ordering::SeqCst), 0);

    gated.release.notify_one();
    wait_until("row 25", || scenery.row(25).is_some()).await;
    Ok(())
}

#[tokio::test]
async fn a_same_range_refresh_runs_after_the_current_load() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let gated = Gated::new();
    let lens = gated_lens(tmp.path().join("c.redb"), rows(40), &gated);
    let dio = lens.make_dio(master(&[("v", "String")])).await?;
    let scenery = dio.table_scenery().page_size(10).open().await?;
    release_the_open_fetch(&gated, &scenery).await;

    // Scroll to 20..30: call 2 starts and blocks on the gate. By the time
    // its callback has run at all, `last_viewport` is already 20..30 (it is
    // stamped before the callback is even dispatched), so the refresh below
    // targets this same block, not the one loaded before it.
    scenery.set_viewport(20..30);
    wait_until("the 20..30 load to start", || calls_len(&gated) >= 2).await;
    assert_eq!(gated.calls.lock().unwrap()[1], 20..30);

    // A force_load refresh of the block currently loading: same range, so it
    // must park behind the running load rather than race or cancel it.
    scenery.request_refresh();
    tokio::time::sleep(Duration::from_millis(120)).await;
    assert_eq!(
        calls_len(&gated),
        2,
        "a same-range refresh must not preempt the load already in flight"
    );

    // Release call 2: its rows commit. The parked refresh then runs as call 3.
    gated.release.notify_one();
    wait_until("the parked refresh to run", || calls_len(&gated) >= 3).await;
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
    let tmp = TempDir::new().unwrap();
    let gated = Gated::new();
    let lens = gated_lens(tmp.path().join("c.redb"), rows(40), &gated);
    let dio = lens.make_dio(master(&[("v", "String")])).await?;
    let scenery = dio.table_scenery().page_size(10).open().await?;
    release_the_open_fetch(&gated, &scenery).await;

    // Call 2 for 20..30 blocks on the gate; the refresh behind it parks.
    scenery.set_viewport(20..30);
    wait_until("the 20..30 load to start", || calls_len(&gated) >= 2).await;
    scenery.request_refresh();
    tokio::time::sleep(Duration::from_millis(120)).await;
    assert_eq!(calls_len(&gated), 2, "the refresh parked behind the load");

    // Supersede both with 0..10 — fully cached by call 1. A plain viewport
    // for it would be served locally and never reach the callback; call 3
    // happening at all is the parked refresh's `force_load` arriving on it.
    scenery.set_viewport(0..10);
    wait_until("the superseding load to run", || calls_len(&gated) >= 3).await;
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
    let tmp = TempDir::new().unwrap();
    let mut gated = Gated::new();
    // No early push: an early-bound row would move `next_load_more_start`
    // past the range this test needs `request_load_more` to ask for.
    gated.early_push = 0;
    let lens = gated_lens(tmp.path().join("c.redb"), rows(40), &gated);
    let dio = lens.make_dio(master(&[("v", "String")])).await?;
    let scenery = dio.table_scenery().page_size(10).open().await?;
    release_the_open_fetch(&gated, &scenery).await;

    // Call 2 for 10..20 blocks on the gate, having bound nothing.
    scenery.set_viewport(10..20);
    wait_until("the 10..20 load to start", || calls_len(&gated) >= 2).await;
    assert_eq!(gated.calls.lock().unwrap()[1], 10..20);

    // Rows 0..9 are cached and nothing above them is, so load-more asks for
    // exactly 10..20 — the range already in flight. Essential, `force_load`:
    // it parks.
    scenery.request_load_more();
    tokio::time::sleep(Duration::from_millis(120)).await;
    assert_eq!(calls_len(&gated), 2, "load-more parked behind the load");

    // The refresh poll then supersedes both: its contiguous-block expansion
    // reaches back over the cached rows 0..9, so it asks for 0..20 — a
    // different range, and a background one.
    scenery.request_refresh();
    wait_until("the superseding load to run", || calls_len(&gated) >= 3).await;
    assert_eq!(gated.calls.lock().unwrap()[2], 0..20);
    assert_eq!(
        gated.priority_of_call(2),
        Some(Priority::Essential),
        "the parked load-more's wait rode onto the request that superseded it",
    );
    gated.release.notify_one();
    Ok(())
}

#[tokio::test]
async fn a_cancelled_refresh_over_unchanged_rows_keeps_them() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let gated = Gated::new();
    let lens = gated_lens(tmp.path().join("c.redb"), rows(40), &gated);
    let dio = lens.make_dio(master(&[("v", "String")])).await?;
    let scenery = dio.table_scenery().page_size(10).open().await?;
    release_the_open_fetch(&gated, &scenery).await;
    assert_eq!(col_at(&scenery, 0, "v").as_deref(), Some("v0"));

    // A force_load refresh of the block just loaded: call 2 re-fetches
    // 0..10. Its early push re-sends the first rows unchanged — the
    // identical-fresh-record dedup in `write_chunk_row` answers `Skipped`
    // for them (they were already bound, unchanged, by call 1) — then it
    // blocks on the gate.
    scenery.request_refresh();
    wait_until("the refresh to start", || calls_len(&gated) >= 2).await;
    assert_eq!(gated.calls.lock().unwrap()[1], 0..10);

    // Supersede it with a different range before it finishes: call 2 is
    // cancelled. Its early-pushed rows were never *bound*, so the undo must
    // not touch them.
    scenery.set_viewport(20..30);
    wait_until("the superseding load to start", || calls_len(&gated) >= 3).await;
    assert_eq!(gated.calls.lock().unwrap()[2], 20..30);

    gated.release.notify_one();
    wait_until("row 25", || scenery.row(25).is_some()).await;

    // Rows 0, 1 and 2 must still be present with their original values — a
    // cancelled load that only ever touched them through the dedup skip
    // must not have deleted them.
    assert_eq!(col_at(&scenery, 0, "v").as_deref(), Some("v0"));
    assert_eq!(col_at(&scenery, 1, "v").as_deref(), Some("v1"));
    assert_eq!(col_at(&scenery, 2, "v").as_deref(), Some("v2"));
    Ok(())
}

/// The case the undo is really about: the cancelled load had already
/// overwritten a row with a *newer* value. Deleting that slot would punch a
/// hole in the grid where a perfectly good cached row was; the slot goes back
/// to the record it held instead.
#[tokio::test]
async fn a_cancelled_refresh_over_a_changed_row_restores_the_old_one() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let gated = Gated::new();
    let backend = rows(40);
    let lens = gated_lens(tmp.path().join("c.redb"), backend.clone(), &gated);
    let dio = lens.make_dio(master(&[("v", "String")])).await?;
    let scenery = dio.table_scenery().page_size(10).open().await?;
    release_the_open_fetch(&gated, &scenery).await;
    assert_eq!(col_at(&scenery, 3, "v").as_deref(), Some("v3"));

    // The backend moves under us, then a refresh picks the new value up in
    // its early push — row 3 is genuinely re-bound, with the old record
    // handed back as what it replaced.
    backend.lock().unwrap()[3].1 = rec("v3-new");
    scenery.request_refresh();
    wait_until("the refresh to start", || calls_len(&gated) >= 2).await;
    assert_eq!(gated.calls.lock().unwrap()[1], 0..10);
    wait_until("row 3 to take the new value", || {
        col_at(&scenery, 3, "v").as_deref() == Some("v3-new")
    })
    .await;

    // Supersede the refresh before it can commit: the new value was never
    // written to the cache, so the slot must go back to the old record
    // rather than disappear.
    scenery.set_viewport(20..30);
    wait_until("the superseding load to start", || calls_len(&gated) >= 3).await;
    gated.release.notify_one();
    wait_until("row 25", || scenery.row(25).is_some()).await;

    assert_eq!(
        col_at(&scenery, 3, "v").as_deref(),
        Some("v3"),
        "the cancelled load's overwrite was rolled back to the cached record",
    );
    assert_eq!(col_at(&scenery, 0, "v").as_deref(), Some("v0"));
    assert_eq!(col_at(&scenery, 1, "v").as_deref(), Some("v1"));
    assert_eq!(col_at(&scenery, 2, "v").as_deref(), Some("v2"));
    Ok(())
}

/// Rows whose id column is an integer, as a numeric-primary-key source
/// serves them. The cache key is still the string form, which is what
/// `cbor_id_to_string` is for.
fn rows_with_int_ids(n: usize) -> Backend {
    Arc::new(Mutex::new(
        (0..n)
            .map(|i| {
                let mut r = Record::new();
                r.insert("id".to_string(), CborValue::Integer((i as i64).into()));
                r.insert("v".to_string(), CborValue::Text(format!("v{i}")));
                (i.to_string(), r)
            })
            .collect(),
    ))
}

/// Restoring a slot re-registers its id, and "its id" is whatever the record
/// carries — an integer as readily as a string. Read back through the one
/// id-keyed path a test can drive from outside: a failed flash emits
/// `WriteReverted`, which the reactor resolves through `id_to_idx` and stamps
/// onto the row. No mapping, no stamp.
#[tokio::test]
async fn restore_re_registers_numeric_ids() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let gated = Gated::new();
    let backend = rows_with_int_ids(40);
    let lens = gated_lens_rejecting_writes(tmp.path().join("c.redb"), backend.clone(), &gated);
    let dio = lens
        .make_dio(support::chunk::master_with_id_type(
            "Int",
            &[("v", "String")],
        ))
        .await?;
    let scenery = dio.table_scenery().page_size(10).open().await?;
    release_the_open_fetch(&gated, &scenery).await;

    // Row 2 changes, so the refresh's early push re-binds exactly that slot
    // — and only it: the other early rows dedup away unchanged.
    let mut changed = Record::new();
    changed.insert("id".to_string(), CborValue::Integer(2.into()));
    changed.insert("v".to_string(), CborValue::Text("v2-new".into()));
    backend.lock().unwrap()[2].1 = changed;
    scenery.request_refresh();
    wait_until("the refresh to start", || calls_len(&gated) >= 2).await;
    wait_until("row 2 to take the new value", || {
        col_at(&scenery, 2, "v").as_deref() == Some("v2-new")
    })
    .await;

    // Cancel it: row 2 is restored, and with it the mapping from its id.
    scenery.set_viewport(20..30);
    wait_until("the superseding load to start", || calls_len(&gated) >= 3).await;
    gated.release.notify_one();
    wait_until("row 25", || scenery.row(25).is_some()).await;
    assert_eq!(col_at(&scenery, 2, "v").as_deref(), Some("v2"));

    // The write route always fails, so this settles as `WriteFailed` on the
    // row the id resolves to — which requires the restored mapping.
    let mut patch = Record::new();
    patch.insert("v".to_string(), CborValue::Text("flashed".into()));
    assert!(dio.flash_patch("2", patch).await.is_err());
    wait_until("row 2 to be stamped through its id", || {
        scenery.status_summary().failed == 1
    })
    .await;
    assert!(
        matches!(
            scenery.row(2).map(|r| r.status.clone()),
            Some(vantage_diorama::RowStatus::WriteFailed { .. })
        ),
        "the failed write landed on row 2, so its numeric id still maps to it",
    );
    Ok(())
}

/// A lens whose chunk callback fails. `pushes_first` decides whether it
/// fails clean or fails having already bound rows the cache never saw.
fn failing_lens(
    cache: std::path::PathBuf,
    backend: Backend,
    fail: Arc<AtomicBool>,
    calls: Arc<AtomicUsize>,
    pushes_first: bool,
) -> Arc<Lens> {
    let lens = with_backend_total(Lens::new().cache_at(cache), &backend);
    Arc::new(
        lens.on_load_chunk(move |_dio, range, _query, sink| {
            let b = backend.clone();
            let fail = fail.clone();
            let calls = calls.clone();
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                if fail.load(Ordering::SeqCst) {
                    if pushes_first {
                        // Two rows with genuinely new values, so they bind
                        // rather than dedup away, and then the fetch dies.
                        for idx in range.clone().take(2) {
                            sink.push(idx, format!("r{idx}"), rec(&format!("v{idx}-new")))
                                .await?;
                        }
                    }
                    return Err(vantage_core::error!("API request failed", status = 503));
                }
                let rows = b.lock().unwrap().clone();
                push_rows(&rows, range, &sink).await
            }
        })
        .build()
        .expect("lens"),
    )
}

#[tokio::test]
async fn a_failed_background_refresh_keeps_the_rows() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let fail = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    let lens = failing_lens(
        tmp.path().join("c.redb"),
        rows(30),
        fail.clone(),
        calls.clone(),
        false,
    );
    let dio = lens.make_dio(master(&[("v", "String")])).await?;
    let view = MockView::open(&dio, 10).await;
    view.settle_until("first page", |v| v.loaded_rows() >= 10)
        .await;
    let before: Vec<_> = (0..10).map(|i| view.col_at(i, "v")).collect();
    let calls_before = calls.load(Ordering::SeqCst);

    fail.store(true, Ordering::SeqCst);
    view.scenery().request_refresh();
    // Wait for the failing fetch to have actually run: without this the
    // assertions below hold vacuously, over a refresh that never dispatched.
    wait_until("the failing refresh to run", || {
        calls.load(Ordering::SeqCst) > calls_before
    })
    .await;

    let after: Vec<_> = (0..10).map(|i| view.col_at(i, "v")).collect();
    assert_eq!(before, after, "rows on screen survive a failed refresh");
    assert_eq!(view.gray_rows(), 0);
    Ok(())
}

/// A failure after the callback has already pushed rows unwinds like a
/// cancellation: those rows were never committed to the cache, so leaving
/// them on screen would show values no later read can reproduce.
#[tokio::test]
async fn a_failing_refresh_that_pushed_rows_first_restores_them() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let fail = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    let lens = failing_lens(
        tmp.path().join("c.redb"),
        rows(30),
        fail.clone(),
        calls.clone(),
        true,
    );
    let dio = lens.make_dio(master(&[("v", "String")])).await?;
    let view = MockView::open(&dio, 10).await;
    view.settle_until("first page", |v| v.loaded_rows() >= 10)
        .await;
    let before: Vec<_> = (0..10).map(|i| view.col_at(i, "v")).collect();
    let calls_before = calls.load(Ordering::SeqCst);

    fail.store(true, Ordering::SeqCst);
    view.scenery().request_refresh();
    wait_until("the failing refresh to run", || {
        calls.load(Ordering::SeqCst) > calls_before
    })
    .await;
    // The restore happens in the commit half, after the callback returns.
    wait_until("rows 0 and 1 to be back", || {
        view.col_at(0, "v").as_deref() == Some("v0") && view.col_at(1, "v").as_deref() == Some("v1")
    })
    .await;

    let after: Vec<_> = (0..10).map(|i| view.col_at(i, "v")).collect();
    assert_eq!(
        before, after,
        "the rows the failed fetch had pushed went back to their cached values",
    );
    assert_eq!(view.gray_rows(), 0);
    Ok(())
}
