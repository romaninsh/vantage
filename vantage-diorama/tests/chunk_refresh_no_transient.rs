//! A client-sorted scenery over a paged master must never expose the master's
//! native order — not even for the brief window between a refresh's in-place
//! refetch and the re-sort that follows it.
//!
//! The grid repaints on its own timer (independent of generation bumps), so any
//! moment the displayed rows sit in master order is a visible flicker. A paged
//! view whose order the master can't take is served from the whole set its
//! Lens copies through the eager start, so a refresh re-sorts the cache and no
//! window is fetched at all while the sort holds.
//!
//! This test observes the rows *from inside* the paged loader, right after the
//! pushes — exactly the window the grid can repaint in — and asserts that no
//! such load runs under the sort.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_diorama::{Cache, Lens, Loader, TableScenery};
use vantage_types::Record;
use vantage_vista::{SortDirection, Vista};

mod support;
use support::chunk::{Backend, col_at, master as master_cols, wait_for_gen};

fn rec(v: &str) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("v".to_string(), CborValue::Text(v.to_string()));
    r
}

fn value(scenery: &Arc<dyn TableScenery>, idx: usize) -> Option<String> {
    col_at(scenery, idx, "v")
}

fn master() -> Vista {
    master_cols(&[("v", "String")])
}

/// Master native order is (a=v3, b=v1, c=v2). Sorted ascending the view is
/// (v1, v2, v3). Were a refresh to refetch a window, the paged loader would
/// push in master order; the test would read `row(0)` straight after those
/// pushes. Under the sort no window load runs, and the order stays sorted.
#[tokio::test]
async fn refresh_never_exposes_master_order_midload() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let backend: Backend = Arc::new(Mutex::new(vec![
        ("a".into(), rec("v3")),
        ("b".into(), rec("v1")),
        ("c".into(), rec("v2")),
    ]));

    // Shared seams: the scenery handle (set after open) and the order observed
    // from inside the refresh's chunk load.
    let scenery_slot: Arc<Mutex<Option<Weak<dyn TableScenery>>>> = Arc::new(Mutex::new(None));
    let observed: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let load_count = Arc::new(AtomicUsize::new(0));

    let lens = {
        let backend = backend.clone();
        let whole = backend.clone();
        let scenery_slot = scenery_slot.clone();
        let observed = observed.clone();
        let load_count = load_count.clone();
        let loader = Loader::paged(move |_cx, range, _query, sink| {
            let b = backend.clone();
            let scenery_slot = scenery_slot.clone();
            let observed = observed.clone();
            let load_count = load_count.clone();
            async move {
                let rows = b.lock().unwrap().clone();
                sink.set_total(rows.len());
                for idx in range {
                    if let Some((id, r)) = rows.get(idx) {
                        sink.push(idx, id.clone(), r.clone()).await?;
                    }
                }
                // Second load == the refresh's in-place refetch. Snapshot
                // the visible order in the post-push / pre-resort window.
                if load_count.fetch_add(1, Ordering::SeqCst) == 1
                    && let Some(scenery) = scenery_slot
                        .lock()
                        .unwrap()
                        .as_ref()
                        .and_then(Weak::upgrade)
                {
                    let snap: Vec<String> = (0..3).filter_map(|i| value(&scenery, i)).collect();
                    *observed.lock().unwrap() = snap;
                }
                Ok(())
            }
        })
        .or_eager(move |cx| {
            let rows: indexmap::IndexMap<_, _> = whole.lock().unwrap().iter().cloned().collect();
            async move { cx.cache().insert_values(rows).await }
        });
        Lens::builder()
            .cache(Cache::redb(tmp.path().join("c.redb")))
            .loader(loader)
            .build()
            .expect("build paged lens")
    };

    let dio = lens.dio(master()).build().await?;
    let scenery = dio.table_scenery().open().await?;
    *scenery_slot.lock().unwrap() = Some(Arc::downgrade(&scenery));
    let mut gen_rx = scenery.subscribe();

    let g0 = u64::from(*gen_rx.borrow_and_update());
    scenery.set_viewport(0..3, 0..0);
    wait_for_gen(&mut gen_rx, g0).await;

    let loads_before_sort = load_count.load(Ordering::SeqCst);
    observed.lock().unwrap().clear();
    let g1 = u64::from(*gen_rx.borrow_and_update());
    scenery.set_order(Some(("v".to_string(), SortDirection::Ascending)));
    wait_for_gen(&mut gen_rx, g1).await;
    assert_eq!(value(&scenery, 0).as_deref(), Some("v1"), "sorted top");

    let g2 = u64::from(*gen_rx.borrow_and_update());
    dio.refresh().await?;
    wait_for_gen(&mut gen_rx, g2).await;

    // What the grid could have repainted mid-refresh: nothing, because no
    // window load ran under the sort.
    assert_eq!(
        load_count.load(Ordering::SeqCst),
        loads_before_sort,
        "a sorted view over a master that can't order fetches no window"
    );
    assert!(
        observed.lock().unwrap().is_empty(),
        "mid-refresh the visible order never passes through the master's (v3,v1,v2)"
    );

    // And the settled order is sorted too.
    assert_eq!(value(&scenery, 0).as_deref(), Some("v1"), "settled top");
    assert_eq!(value(&scenery, 2).as_deref(), Some("v3"), "settled bottom");
    Ok(())
}
