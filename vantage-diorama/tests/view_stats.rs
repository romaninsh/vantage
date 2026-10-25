//! `ViewStats` — the snapshot a status bar reads, and when it moves.

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use tokio::sync::Semaphore;
use vantage_core::Result;
use vantage_diorama::{
    Cache, CacheBackend, Dio, FilterOrigin, Lens, LoadState, Loader, RedbCache, RowStatusSummary,
    TableScenery, ViewCount, ViewState, ViewStats,
};
use vantage_types::Record;
use vantage_vista::{
    Column, Condition, FilterOp, SortDirection, Vista, VistaMetadata, mocks::MockShell,
};

mod support;
use support::chunk::{master as paged_master, redb_lens};
use support::copy_master;

fn record(name: &str, price: i64) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("name".to_string(), CborValue::Text(name.to_string()));
    r.insert("price".to_string(), CborValue::Integer(price.into()));
    r
}

fn seeded_master() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String"))
        .with_column(Column::new("price", "i64"))
        .with_id_column("id");
    let shell = MockShell::new()
        .with_metadata(metadata)
        .with_record("a", record("alpha", 30))
        .with_record("b", record("beta", 10))
        .with_record("c", record("gamma", 20));
    Vista::new("items", shell)
}

/// An eager Dio over the seeded master: the start copies it into the cache.
async fn eager_dio(cache: std::path::PathBuf) -> Result<Dio> {
    redb_lens(cache, copy_master())
        .dio(seeded_master())
        .build()
        .await
}

/// A paged loader serving `rows` at their indices, stating `total` when
/// given, each load waiting for a permit of `gate` when given.
fn row_loader(
    rows: Vec<(String, Record<CborValue>)>,
    total: Option<usize>,
    gate: Option<Arc<Semaphore>>,
) -> Loader {
    Loader::paged(move |_cx, range, _query, sink| {
        let (rows, gate) = (rows.clone(), gate.clone());
        async move {
            if let Some(gate) = gate {
                gate.acquire().await.expect("gate").forget();
            }
            if let Some(total) = total {
                sink.set_total(total);
            }
            for idx in range {
                if let Some((id, r)) = rows.get(idx) {
                    sink.push(idx, id.clone(), r.clone()).await?;
                }
            }
            Ok(())
        }
    })
}

fn text_rows(n: usize) -> Vec<(String, Record<CborValue>)> {
    (0..n)
        .map(|i| {
            let mut r = Record::new();
            r.insert("v".to_string(), CborValue::Text(format!("v{i}")));
            (format!("id{i:04}"), r)
        })
        .collect()
}

/// Wait until the published snapshot satisfies `ok`; the timeout bounds a
/// hang, not a speed.
async fn wait_stats(
    what: &str,
    scenery: &Arc<dyn TableScenery>,
    ok: impl Fn(&ViewStats) -> bool,
) -> ViewStats {
    let mut rx = scenery.subscribe_view_stats();
    let found = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            rx.borrow_and_update();
            let stats = scenery.view_stats();
            if ok(&stats) {
                return stats;
            }
            rx.changed().await.expect("view stats channel closed");
        }
    })
    .await;
    found.unwrap_or_else(|_| panic!("timed out waiting for {what}: {:?}", scenery.view_stats()))
}

#[tokio::test]
async fn loaded_rows_make_an_exact_total() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = eager_dio(tmp.path().join("c.redb")).await?;
    let scenery = dio.table_scenery().open().await?;

    let stats = wait_stats("rows", &scenery, |s| s.loaded == 3).await;
    assert_eq!(stats.count(), ViewCount::Exact(3));
    assert!(!stats.has_more);
    assert_eq!(stats.state, ViewState::Ready);
    assert_eq!(stats.showing, None);
    Ok(())
}

#[tokio::test]
async fn the_shown_range_is_the_second_viewport_argument() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = eager_dio(tmp.path().join("c.redb")).await?;
    let scenery = dio.table_scenery().open().await?;
    wait_stats("rows", &scenery, |s| s.loaded == 3).await;

    let mut rx = scenery.subscribe_view_stats();
    rx.borrow_and_update();
    scenery.set_viewport(0..10, 1..3);
    assert!(rx.has_changed().unwrap(), "a shown-range move signals");
    assert_eq!(scenery.view_stats().showing, Some(1..3));

    // Clamped to the rows the view has.
    scenery.set_viewport(0..10, 1..50);
    assert_eq!(scenery.view_stats().showing, Some(1..3));
    assert_eq!(scenery.view_stats().showing_count(), 2);

    scenery.set_viewport(0..10, 0..0);
    assert_eq!(
        scenery.view_stats().showing,
        None,
        "an empty range shows none"
    );
    Ok(())
}

#[tokio::test]
async fn order_search_and_filters_are_reported() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = eager_dio(tmp.path().join("c.redb")).await?;
    let scenery = dio
        .table_scenery()
        .filter(Condition::new("price", FilterOp::Gt, 5))
        .open()
        .await?;
    wait_stats("rows", &scenery, |s| s.loaded == 3).await;
    let filters = scenery.view_stats().filters;
    assert_eq!(filters.len(), 1);
    assert_eq!(filters[0].origin, FilterOrigin::Query);
    assert_eq!(filters[0].op, FilterOp::Gt);

    scenery.set_order(Some(("price".into(), SortDirection::Descending)));
    assert_eq!(
        scenery.view_stats().order,
        Some(("price".to_string(), SortDirection::Descending))
    );

    scenery.set_search(Some("al".into()));
    assert_eq!(scenery.view_stats().search.as_deref(), Some("al"));

    scenery.set_filters(vec![Condition::new("name", FilterOp::Ne, "beta")]);
    let stats = scenery.view_stats();
    assert_eq!(stats.filters.len(), 2);
    assert_eq!(stats.filters[1].origin, FilterOrigin::View);
    assert_eq!(stats.filters[1].column, "name");

    // The narrowed set lands: only `alpha` matches the search.
    wait_stats("narrowed rows", &scenery, |s| s.loaded == 1).await;
    assert_eq!(scenery.view_stats().count(), ViewCount::Exact(1));
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn remembered_total_is_an_estimate_until_restated() -> Result<()> {
    const REAL_TOTAL: usize = 133;
    let tmp = TempDir::new().unwrap();
    let gate = Arc::new(Semaphore::new(0));
    let rows = text_rows(REAL_TOTAL);
    let backend = Arc::new(RedbCache::open(tmp.path().join("c.redb"))?);
    let lens = Lens::builder()
        .cache(Cache::backend(backend.clone()))
        .loader(row_loader(rows, Some(REAL_TOTAL), Some(gate.clone())))
        .build()?;
    let dio = lens.dio(paged_master(&[("v", "String")])).build().await?;

    // A warm cache from an earlier session that stated 500 rows.
    let table = backend.open_table("items").await?;
    let (id, rec) = &text_rows(1)[0];
    table.insert_value(id, rec).await?;
    table.set_meta_total(500).await?;

    let scenery = dio.table_scenery().open().await?;
    let stats = scenery.view_stats();
    assert_eq!(stats.count(), ViewCount::Estimated(500));
    assert_eq!(stats.count_text(), "~500");

    gate.add_permits(100);
    let stats = wait_stats("restated total", &scenery, |s| s.total_exact).await;
    assert_eq!(stats.count(), ViewCount::Exact(REAL_TOTAL));
    assert!(stats.has_more);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn horizon_is_a_lower_bound_until_the_end_is_found() -> Result<()> {
    const REAL_TOTAL: usize = 133;
    let tmp = TempDir::new().unwrap();
    let rows = text_rows(REAL_TOTAL);
    let lens = redb_lens(tmp.path().join("c.redb"), row_loader(rows, None, None));
    let dio = lens.dio(paged_master(&[("v", "String")])).build().await?;
    let scenery = dio.table_scenery().open().await?;

    // A full first page from a source that states no total: more exist, but
    // the horizon one page ahead is not a total.
    let stats = wait_stats("first page", &scenery, |s| s.loaded == 100 && s.has_more).await;
    assert_eq!(stats.count(), ViewCount::AtLeast(100));
    assert_eq!(stats.count_text(), "100+");

    // The short page past it is the end of the set.
    scenery.set_viewport(100..200, 100..200);
    let stats = wait_stats("end of set", &scenery, |s| s.total_exact).await;
    assert_eq!(stats.count(), ViewCount::Exact(REAL_TOTAL));
    assert_eq!(stats.loaded, REAL_TOTAL);
    assert!(!stats.has_more);
    assert_eq!(stats.showing, Some(100..REAL_TOTAL));
    Ok(())
}

#[tokio::test]
async fn a_limit_reports_the_cap() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = eager_dio(tmp.path().join("c.redb")).await?;
    let capped = dio.table_scenery().limit(2).open().await?;
    wait_stats("rows", &capped, |s| s.loaded == 2).await;

    capped.set_viewport(0..10, 0..10);
    let stats = wait_stats("capped viewport", &capped, |s| s.showing.is_some()).await;
    assert_eq!(stats.count(), ViewCount::Exact(2));
    assert_eq!(stats.loaded, 2);
    assert!(!stats.has_more);
    assert_eq!(stats.showing, Some(0..2));
    Ok(())
}

// ---- the snapshot's own rules ----------------------------------------------

fn stats(total: Option<usize>, total_exact: bool, loaded: usize, has_more: bool) -> ViewStats {
    let mut stats = ViewStats::default();
    stats.total = total;
    stats.total_exact = total_exact;
    stats.loaded = loaded;
    stats.has_more = has_more;
    stats
}

#[test]
fn the_count_rule_never_presents_a_partial_number_as_the_whole() {
    let exact = stats(Some(1_240), true, 100, true);
    assert_eq!(exact.count(), ViewCount::Exact(1_240));
    assert_eq!(exact.count_text(), "1,240");

    let estimated = stats(Some(1_200), false, 100, true);
    assert_eq!(estimated.count(), ViewCount::Estimated(1_200));
    assert_eq!(estimated.count_text(), "~1,200");

    let lower_bound = stats(None, false, 300, true);
    assert_eq!(lower_bound.count(), ViewCount::AtLeast(300));
    assert_eq!(lower_bound.count_text(), "300+");

    let all_there = stats(None, false, 5, false);
    assert_eq!(all_there.count(), ViewCount::Loaded(5));
    assert_eq!(all_there.count_text(), "5");
}

#[test]
fn a_count_value_drops_its_qualifier() {
    for count in [
        ViewCount::Exact(7),
        ViewCount::Estimated(7),
        ViewCount::AtLeast(7),
        ViewCount::Loaded(7),
    ] {
        assert_eq!(count.value(), 7, "{count:?}");
    }
}

#[test]
fn showing_counts_zero_while_no_grid_shows_the_view() {
    let mut idle = ViewStats::default();
    assert_eq!(idle.showing_len(), None);
    assert_eq!(idle.showing_count(), 0);
    idle.showing = Some(40..60);
    assert_eq!(idle.showing_len(), Some(20));
    assert_eq!(idle.showing_count(), 20);
}

#[test]
fn a_cap_reached_completes_the_view_at_exactly_the_cap() {
    let mut full = stats(None, false, 50, true);
    full.showing = Some(5..50);
    let capped = full.capped(10);
    assert_eq!(capped.loaded, 10);
    assert_eq!(capped.total, Some(10));
    assert!(capped.total_exact);
    assert!(!capped.has_more);
    assert_eq!(capped.showing, Some(5..10));
}

#[test]
fn a_cap_not_yet_reached_keeps_the_inner_answer_bounded_by_the_cap() {
    let mut partial = stats(Some(500), true, 3, true);
    partial.showing = Some(20..30);
    let capped = partial.capped(10);
    assert_eq!(capped.loaded, 3);
    assert_eq!(capped.total, Some(10));
    assert!(capped.has_more);
    assert_eq!(capped.showing, None, "nothing shown remains inside the cap");

    let small = stats(Some(4), true, 3, true).capped(10);
    assert_eq!(small.total, Some(4));
}

#[test]
fn the_state_marker_picks_the_most_urgent() {
    let rows = |failed, pending_write| {
        let mut rows = RowStatusSummary::default();
        rows.failed = failed;
        rows.pending_write = pending_write;
        rows
    };
    let complete = LoadState::Complete;
    assert_eq!(
        ViewState::derive(&LoadState::Loading, &rows(1, 1), true),
        ViewState::Loading
    );
    assert_eq!(
        ViewState::derive(&complete, &rows(1, 1), true),
        ViewState::Failed
    );
    assert_eq!(
        ViewState::derive(&complete, &rows(0, 1), true),
        ViewState::PendingWrites
    );
    assert_eq!(
        ViewState::derive(&complete, &rows(0, 0), true),
        ViewState::Stale
    );
    assert_eq!(
        ViewState::derive(&complete, &rows(0, 0), false),
        ViewState::Ready
    );
}
