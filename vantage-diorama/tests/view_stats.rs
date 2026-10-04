//! `ViewStats` — the snapshot a status bar reads, and when it moves.

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use tokio::sync::Semaphore;
use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{
    CappedScenery, FilterOrigin, Lens, OpCondition, SortDir, TableScenery, ViewCount, ViewState,
    ViewStats,
};
use vantage_types::Record;
use vantage_vista::{Column, FilterOp, Vista, VistaMetadata, mocks::MockShell};

mod support;
use support::chunk::master as paged_master;

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
    Vista::new("items", Box::new(shell))
}

/// Eager lens: `on_start` copies the master into the cache.
fn eager_lens(cache: std::path::PathBuf) -> Arc<Lens> {
    let lens = Lens::new()
        .cache_at(cache)
        .on_start(|dio| {
            let dio = dio.clone();
            async move {
                let rows = dio.master().list_values().await?;
                dio.cache().insert_values(rows).await
            }
        })
        .build()
        .expect("build lens");
    Arc::new(lens)
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
    let dio = eager_lens(tmp.path().join("c.redb"))
        .make_dio(seeded_master())
        .await?;
    let scenery = dio.table_scenery().open().await?;

    let stats = wait_stats("rows", &scenery, |s| s.loaded == 3).await;
    assert_eq!(stats.count(), ViewCount::Exact(3));
    assert!(!stats.has_more);
    assert_eq!(stats.state, ViewState::Ready);
    assert_eq!(stats.showing, None);
    Ok(())
}

#[tokio::test]
async fn showing_follows_the_shown_range() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = eager_lens(tmp.path().join("c.redb"))
        .make_dio(seeded_master())
        .await?;
    let scenery = dio.table_scenery().open().await?;
    wait_stats("rows", &scenery, |s| s.loaded == 3).await;

    let mut rx = scenery.subscribe_view_stats();
    rx.borrow_and_update();
    scenery.set_shown_range(Some(0..2));
    assert!(rx.has_changed().unwrap(), "a shown-range move signals");
    assert_eq!(scenery.view_stats().showing, Some(0..2));

    // Clamped to the rows the view has.
    scenery.set_shown_range(Some(1..50));
    assert_eq!(scenery.view_stats().showing, Some(1..3));
    assert_eq!(scenery.view_stats().showing_count(), 2);

    scenery.set_shown_range(None);
    assert_eq!(scenery.view_stats().showing, None);
    Ok(())
}

#[tokio::test]
async fn a_hydration_viewport_is_not_showing() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = eager_lens(tmp.path().join("c.redb"))
        .make_dio(seeded_master())
        .await?;
    // A dashboard counter and a grid share one scenery.
    let scenery = dio.table_scenery().open().await?;
    wait_stats("rows", &scenery, |s| s.loaded == 3).await;

    scenery.set_viewport(0..200);
    assert_eq!(scenery.view_stats().showing, None, "no grid shows the view");

    scenery.set_shown_range(Some(0..2));
    scenery.set_viewport(1..3);
    assert_eq!(
        scenery.view_stats().showing,
        Some(0..2),
        "another consumer's viewport leaves the grid's range alone",
    );
    Ok(())
}

#[tokio::test]
async fn order_search_and_filters_are_reported() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = eager_lens(tmp.path().join("c.redb"))
        .make_dio(seeded_master())
        .await?;
    let scenery = dio
        .table_scenery()
        .where_op("price", FilterOp::Gt, 5)
        .open()
        .await?;
    wait_stats("rows", &scenery, |s| s.loaded == 3).await;
    let filters = scenery.view_stats().filters;
    assert_eq!(filters.len(), 1);
    assert_eq!(filters[0].origin, FilterOrigin::Query);
    assert_eq!(filters[0].op, FilterOp::Gt);

    scenery.set_sort(Some("price".into()), SortDir::Desc);
    assert_eq!(
        scenery.view_stats().order,
        Some(("price".to_string(), SortDir::Desc))
    );

    scenery.set_search(Some("al".into()));
    assert_eq!(scenery.view_stats().search.as_deref(), Some("al"));

    scenery.set_filter_terms(vec![OpCondition::new("name", FilterOp::Ne, "beta")]);
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
    let chunk_gate = gate.clone();
    let lens = Lens::new()
        .cache_at(tmp.path().join("c.redb"))
        .on_load_chunk(move |_dio, range, _query, sink| {
            let rows = rows.clone();
            let gate = chunk_gate.clone();
            async move {
                gate.acquire().await.expect("gate").forget();
                sink.set_total(REAL_TOTAL);
                for idx in range {
                    if let Some((id, r)) = rows.get(idx) {
                        sink.push(idx, id.clone(), r.clone()).await?;
                    }
                }
                Ok(())
            }
        })
        .build()
        .map(Arc::new)
        .expect("build lens");
    let dio = lens.make_dio(paged_master(&[("v", "String")])).await?;

    // A warm cache from an earlier session that stated 500 rows.
    let (id, rec) = &text_rows(1)[0];
    dio.cache().insert_value(id, rec).await?;
    dio.cache().set_meta_total(500).await?;

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
    let lens = Lens::new()
        .cache_at(tmp.path().join("c.redb"))
        .on_load_chunk(move |_dio, range, _query, sink| {
            let rows = rows.clone();
            async move {
                for idx in range {
                    if let Some((id, r)) = rows.get(idx) {
                        sink.push(idx, id.clone(), r.clone()).await?;
                    }
                }
                Ok(())
            }
        })
        .build()
        .map(Arc::new)
        .expect("build lens");
    let dio = lens.make_dio(paged_master(&[("v", "String")])).await?;
    let scenery = dio.table_scenery().open().await?;

    // A full first page from a source that states no total: more exist, but
    // the horizon one page ahead is not a total.
    let stats = wait_stats("first page", &scenery, |s| s.loaded == 100 && s.has_more).await;
    assert_eq!(stats.count(), ViewCount::AtLeast(100));
    assert_eq!(stats.count_text(), "100+");

    // The short page past it is the end of the set.
    scenery.set_viewport(100..200);
    scenery.set_shown_range(Some(100..200));
    let stats = wait_stats("end of set", &scenery, |s| s.total_exact).await;
    assert_eq!(stats.count(), ViewCount::Exact(REAL_TOTAL));
    assert_eq!(stats.loaded, REAL_TOTAL);
    assert!(!stats.has_more);
    assert_eq!(stats.showing, Some(100..REAL_TOTAL));
    Ok(())
}

#[tokio::test]
async fn capped_scenery_reports_the_cap() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = eager_lens(tmp.path().join("c.redb"))
        .make_dio(seeded_master())
        .await?;
    let inner = dio.table_scenery().open().await?;
    wait_stats("rows", &inner, |s| s.loaded == 3).await;

    let capped: Arc<dyn TableScenery> = CappedScenery::wrap(inner, 2);
    capped.set_viewport(0..10);
    capped.set_shown_range(Some(0..10));
    let stats = wait_stats("capped viewport", &capped, |s| s.showing.is_some()).await;
    assert_eq!(stats.count(), ViewCount::Exact(2));
    assert_eq!(stats.loaded, 2);
    assert!(!stats.has_more);
    assert_eq!(stats.showing, Some(0..2));
    Ok(())
}
