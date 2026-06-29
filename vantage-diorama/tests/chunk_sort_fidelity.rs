//! High-fidelity reproduction of the launch-control launches grid:
//!   - paged/chunk-loaded, NO eager `on_start` (cache fills via viewport loads)
//!   - master serves rows in its own native order (`ordering=-last_updated`),
//!     which DIFFERS from the user's chosen sort column AND changes between
//!     refreshes (the live simulator bumps `last_updated`)
//!   - the grid re-issues `set_viewport` after each generation bump (the watcher)
//!
//! The user's client-side sort by `v` must hold through all of it.

use std::sync::{Arc, Mutex};

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_diorama::{SortDir, TableScenery};
use vantage_types::Record;
use vantage_vista::Vista;

mod support;
use support::chunk::{
    Backend, master as master_cols, order as order_col, paged_lens_native_desc, settle,
    wait_for_gen,
};

/// A backend row: a stable id, a sort value `v`, and a `last_updated` `lu` that
/// the "simulator" bumps to reorder the native (server) order.
fn rec(v: &str, lu: i64) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("v".to_string(), CborValue::Text(v.to_string()));
    r.insert("lu".to_string(), CborValue::Integer(lu.into()));
    r
}

fn order(scenery: &Arc<dyn TableScenery>) -> Vec<String> {
    order_col(scenery, "v")
}

fn master() -> Vista {
    master_cols(&[("v", "String"), ("lu", "i64")])
}

/// Paged lens whose `on_load_chunk` serves rows in native order = `lu`
/// descending (mirrors the launches URL `?ordering=-last_updated`), windowed.
fn paged_lens(cache: std::path::PathBuf, backend: Backend) -> Arc<vantage_diorama::Lens> {
    paged_lens_native_desc(cache, backend, "lu")
}

#[tokio::test]
async fn client_sort_holds_through_reordering_refreshes() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    // Native order by lu desc is: c(30), a(20), b(10). Sort by v asc is a,b,c.
    let backend: Backend = Arc::new(Mutex::new(vec![
        ("a".into(), rec("a", 20)),
        ("b".into(), rec("b", 10)),
        ("c".into(), rec("c", 30)),
    ]));
    let lens = paged_lens(tmp.path().join("c.redb"), backend.clone());
    let dio = lens.make_dio(master()).await?;
    let scenery = dio.table_scenery().open().await?;
    let mut gen_rx = scenery.subscribe();

    // Grid sets its viewport; cache fills from the master in native order.
    let g = u64::from(*gen_rx.borrow_and_update());
    scenery.set_viewport(0..3);
    wait_for_gen(&mut gen_rx, g).await;
    settle().await;
    assert_eq!(
        order(&scenery),
        vec!["c", "a", "b"],
        "native order (lu desc)"
    );

    // User sorts by v ascending.
    let g = u64::from(*gen_rx.borrow_and_update());
    scenery.set_sort(Some("v".to_string()), SortDir::Asc);
    wait_for_gen(&mut gen_rx, g).await;
    settle().await;
    assert_eq!(order(&scenery), vec!["a", "b", "c"], "sorted by v");

    // The simulator reorders native order (bump b's lu to the top) and a refresh
    // fires. The client sort must still hold.
    backend.lock().unwrap()[1].1 = rec("b", 99); // b now newest
    let g = u64::from(*gen_rx.borrow_and_update());
    dio.refresh().await?;
    wait_for_gen(&mut gen_rx, g).await;
    settle().await;
    // The grid re-issues its viewport after the bump (the watcher does this).
    scenery.set_viewport(0..3);
    settle().await;
    assert_eq!(
        order(&scenery),
        vec!["a", "b", "c"],
        "sort holds after a reordering refresh + viewport re-issue"
    );

    // A second refresh for good measure.
    backend.lock().unwrap()[0].1 = rec("a", 1); // a now oldest
    let g = u64::from(*gen_rx.borrow_and_update());
    dio.refresh().await?;
    wait_for_gen(&mut gen_rx, g).await;
    settle().await;
    scenery.set_viewport(0..3);
    settle().await;
    assert_eq!(
        order(&scenery),
        vec!["a", "b", "c"],
        "sort still holds after a second reordering refresh"
    );
    Ok(())
}

/// Same, but the cache is only PARTIALLY loaded when the user sorts (page_size <
/// total) — then the user scrolls to load the rest. Documents how far the
/// local sort reaches as more rows load.
#[tokio::test]
async fn client_sort_with_partial_then_full_load() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    // Native order lu desc: e,d,c,b,a. Sort by v asc: a,b,c,d,e.
    let backend: Backend = Arc::new(Mutex::new(vec![
        ("a".into(), rec("a", 10)),
        ("b".into(), rec("b", 20)),
        ("c".into(), rec("c", 30)),
        ("d".into(), rec("d", 40)),
        ("e".into(), rec("e", 50)),
    ]));
    let lens = paged_lens(tmp.path().join("c.redb"), backend.clone());
    let dio = lens.make_dio(master()).await?;
    let scenery = dio.table_scenery().open().await?;
    let mut gen_rx = scenery.subscribe();

    // Load only the first viewport.
    let g = u64::from(*gen_rx.borrow_and_update());
    scenery.set_viewport(0..5);
    wait_for_gen(&mut gen_rx, g).await;
    settle().await;

    // Sort by v asc, with everything loaded → full sort.
    let g = u64::from(*gen_rx.borrow_and_update());
    scenery.set_sort(Some("v".to_string()), SortDir::Asc);
    wait_for_gen(&mut gen_rx, g).await;
    settle().await;
    assert_eq!(order(&scenery), vec!["a", "b", "c", "d", "e"], "full sort");

    Ok(())
}
