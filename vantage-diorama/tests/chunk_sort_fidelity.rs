//! High-fidelity reproduction of the launch-control launches grid:
//!   - paged/chunk-loaded, NO eager `on_start` (cache fills via viewport loads)
//!   - master serves rows in its own native order (`ordering=-last_updated`),
//!     which DIFFERS from the user's chosen sort column AND changes between
//!     refreshes (the live simulator bumps `last_updated`)
//!   - the grid re-issues `set_viewport` after each generation bump (the watcher)
//!
//! The user's client-side sort by `v` must hold through all of it.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_diorama::{Generation, Lens, SortDir, TableScenery};
use vantage_types::Record;
use vantage_vista::{Column, Vista, VistaMetadata, mocks::MockShell};

/// A backend row: a stable id, a sort value `v`, and a `last_updated` `lu` that
/// the "simulator" bumps to reorder the native (server) order.
fn rec(v: &str, lu: i64) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("v".to_string(), CborValue::Text(v.to_string()));
    r.insert("lu".to_string(), CborValue::Integer(lu.into()));
    r
}

fn value(scenery: &Arc<dyn TableScenery>, idx: usize) -> Option<String> {
    scenery.row(idx).and_then(|r| match r.record.get("v") {
        Some(CborValue::Text(s)) => Some(s.clone()),
        _ => None,
    })
}

fn order(scenery: &Arc<dyn TableScenery>) -> Vec<String> {
    (0..scenery.row_count())
        .filter_map(|i| value(scenery, i))
        .collect()
}

fn master() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("v", "String"))
        .with_column(Column::new("lu", "i64"))
        .with_id_column("id");
    Vista::new("items", Box::new(MockShell::new().with_metadata(metadata)))
}

type Backend = Arc<Mutex<Vec<(String, Record<CborValue>)>>>;

/// Paged lens whose `on_load_chunk` serves rows in **native order** = `lu`
/// descending (mirrors the launches URL `?ordering=-last_updated`), windowed.
/// This is what a non-orderable paged master does: the client sort never
/// reaches it.
fn paged_lens(cache: std::path::PathBuf, backend: Backend) -> Arc<Lens> {
    let total = backend.clone();
    let lens = Lens::new()
        .cache_at(cache)
        .total_provider(move |_dio| {
            let b = total.clone();
            async move { Ok(b.lock().unwrap().len()) }
        })
        .on_load_chunk(move |_dio, range, sink| {
            let b = backend.clone();
            async move {
                // Native order: lu DESC, like the server's baked ordering.
                let mut rows = b.lock().unwrap().clone();
                rows.sort_by(|a, b| {
                    let la = a.1.get("lu");
                    let lb = b.1.get("lu");
                    match (la, lb) {
                        (Some(CborValue::Integer(x)), Some(CborValue::Integer(y))) => {
                            i128::from(*y).cmp(&i128::from(*x))
                        }
                        _ => std::cmp::Ordering::Equal,
                    }
                });
                for idx in range {
                    if let Some((id, r)) = rows.get(idx) {
                        sink.push(idx, id.clone(), r.clone()).await?;
                    }
                }
                Ok(())
            }
        })
        .build()
        .expect("build paged lens");
    Arc::new(lens)
}

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

/// Let the viewport debounce + chunk load settle.
async fn settle() {
    tokio::time::sleep(Duration::from_millis(80)).await;
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
