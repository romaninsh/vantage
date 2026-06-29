//! A client-sorted, chunk-loaded scenery must never expose the master's native
//! order — not even for the brief window between a refresh's in-place refetch
//! and the re-sort that follows it.
//!
//! The grid repaints on its own timer (independent of generation bumps), so any
//! moment the displayed rows sit in master order is a visible flicker: the order
//! jumps to the server's order while the chunk lands, then snaps back once the
//! sort is re-imposed. The fix makes the displayed map a pure projection of the
//! cache via the re-sort, so `push` fills the cache without ever stamping
//! server-ordered rows into the visible map.
//!
//! This test observes the rows *from inside* `on_load_chunk`, right after the
//! pushes and before the loader's re-sort — exactly the window the grid can
//! repaint in.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_diorama::{Generation, Lens, SortDir, TableScenery};
use vantage_types::Record;
use vantage_vista::{Column, Vista, VistaMetadata, mocks::MockShell};

fn rec(v: &str) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("v".to_string(), CborValue::Text(v.to_string()));
    r
}

fn value(scenery: &Arc<dyn TableScenery>, idx: usize) -> Option<String> {
    scenery.row(idx).and_then(|r| match r.record.get("v") {
        Some(CborValue::Text(s)) => Some(s.clone()),
        _ => None,
    })
}

fn master() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("v", "String"))
        .with_id_column("id");
    Vista::new("items", Box::new(MockShell::new().with_metadata(metadata)))
}

type Backend = Arc<Mutex<Vec<(String, Record<CborValue>)>>>;

async fn wait_for_gen(rx: &mut tokio::sync::watch::Receiver<Generation>, current: u64) -> u64 {
    tokio::time::timeout(Duration::from_millis(500), async {
        loop {
            if u64::from(*rx.borrow_and_update()) > current {
                return u64::from(*rx.borrow());
            }
            rx.changed().await.expect("watch channel closed");
        }
    })
    .await
    .expect("timed out waiting for generation bump")
}

/// Master native order is (a=v3, b=v1, c=v2). Sorted ascending the view is
/// (v1, v2, v3). On a refresh, `on_load_chunk` pushes in master order; the test
/// reads `row(0)` straight after those pushes (before the loader re-sorts) and
/// asserts it is still the sorted top — never the master top.
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
        let total = backend.clone();
        let scenery_slot = scenery_slot.clone();
        let observed = observed.clone();
        let load_count = load_count.clone();
        Arc::new(
            Lens::new()
                .cache_at(tmp.path().join("c.redb"))
                .total_provider(move |_dio| {
                    let b = total.clone();
                    async move { Ok(b.lock().unwrap().len()) }
                })
                .on_load_chunk(move |_dio, range, sink| {
                    let b = backend.clone();
                    let scenery_slot = scenery_slot.clone();
                    let observed = observed.clone();
                    let load_count = load_count.clone();
                    async move {
                        let rows = b.lock().unwrap().clone();
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
                            let snap: Vec<String> =
                                (0..3).filter_map(|i| value(&scenery, i)).collect();
                            *observed.lock().unwrap() = snap;
                        }
                        Ok(())
                    }
                })
                .build()
                .expect("build paged lens"),
        )
    };

    let dio = lens.make_dio(master()).await?;
    let scenery = dio.table_scenery().open().await?;
    *scenery_slot.lock().unwrap() = Some(Arc::downgrade(&scenery));
    let mut gen_rx = scenery.subscribe();

    let g0 = u64::from(*gen_rx.borrow_and_update());
    scenery.set_viewport(0..3);
    wait_for_gen(&mut gen_rx, g0).await;

    let g1 = u64::from(*gen_rx.borrow_and_update());
    scenery.set_sort(Some("v".to_string()), SortDir::Asc);
    wait_for_gen(&mut gen_rx, g1).await;
    assert_eq!(value(&scenery, 0).as_deref(), Some("v1"), "sorted top");

    let g2 = u64::from(*gen_rx.borrow_and_update());
    dio.refresh().await?;
    wait_for_gen(&mut gen_rx, g2).await;

    // What the grid could have repainted mid-refresh.
    let observed = observed.lock().unwrap().clone();
    assert_eq!(
        observed,
        vec!["v1".to_string(), "v2".to_string(), "v3".to_string()],
        "mid-refresh the visible order must stay sorted, never the master's (v3,v1,v2)"
    );

    // And the settled order is sorted too.
    assert_eq!(value(&scenery, 0).as_deref(), Some("v1"), "settled top");
    assert_eq!(value(&scenery, 2).as_deref(), Some("v3"), "settled bottom");
    Ok(())
}
