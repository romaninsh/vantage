//! Two sceneries over one Dio: a paged view serving master windows, and a
//! sorted view served from the cache. A window load that brings a new row
//! into the shared cache announces only `RangeLoaded`, and nothing follows
//! it (a hub recheck would find the cached row equal to the master's and
//! announce nothing). The cache-served view must still show the row.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{TableScenery, ViewState};
use vantage_types::Record;
use vantage_vista::SortDirection;

mod support;
use support::chunk::{Backend, master as master_cols, order, paged_lens};

fn rec(v: &str) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("v".to_string(), CborValue::Text(v.to_string()));
    r
}

/// The bound is for a hang, not a speed assertion: the coverage job runs the
/// whole instrumented workspace at once on a small runner. It stays under the
/// 5 s test limit, so a hang names the step it stopped at.
const HANG: Duration = Duration::from_secs(4);

/// Poll `holds` until it is true, or panic naming `what`.
async fn until(what: &str, holds: impl Fn() -> bool) {
    let wait = async {
        while !holds() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    };
    if tokio::time::timeout(HANG, wait).await.is_err() {
        panic!("{what}: not within {HANG:?}");
    }
}

fn shows(view: &Arc<dyn TableScenery>, want: &[&str]) -> bool {
    order(view, "v") == want
}

#[tokio::test]
async fn a_window_load_reaches_a_cache_served_view() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let backend: Backend = Arc::new(Mutex::new(vec![("a".into(), rec("v1"))]));
    let lens = paged_lens(tmp.path().join("c.redb"), backend.clone());
    let dio = lens.dio(master_cols(&[("v", "String")])).build().await?;

    let paged = dio.table_scenery().open().await?;
    paged.set_viewport(0..1, 0..1);
    until("the paged view shows a", || shows(&paged, &["v1"])).await;

    // The master can't sort, so this view copies the set into the cache and
    // sorts it there.
    let sorted = dio
        .table_scenery()
        .order("v", SortDirection::Descending)
        .open()
        .await?;
    until("the sorted view shows a", || shows(&sorted, &["v1"])).await;
    // Opening the sorted view re-pulls the paged one; that re-pull must be
    // over before the source changes, or it may already see the new row.
    until("the paged view settles", || {
        paged.view_stats().state == ViewState::Ready
    })
    .await;

    // A row appears at the source with no change event. The paged view has
    // proved its end (one row); a view attaching to its rows re-pulls the
    // block it holds, which states the new total but brings no new row.
    backend.lock().unwrap().push(("b".into(), rec("v2")));
    let attached = dio.table_scenery().open().await?;
    attached.set_viewport(0..1, 0..1);
    until("the paged view counts b", || paged.row_count() == 2).await;
    assert!(dio.cached().get_value("b").await?.is_none());
    assert!(shows(&sorted, &["v1"]));

    // The paged view's next window brings the row into the shared cache.
    paged.set_viewport(0..2, 0..2);
    until("the paged view shows b", || shows(&paged, &["v1", "v2"])).await;
    // The window's rows reach the cache when the load flushes, which can
    // trail the paged view by a moment.
    tokio::time::timeout(HANG, async {
        while dio.cached().get_value("b").await.ok().flatten().is_none() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("b reaches the cache");

    until("the sorted view shows b", || shows(&sorted, &["v2", "v1"])).await;
    Ok(())
}
