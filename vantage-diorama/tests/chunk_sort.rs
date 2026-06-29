//! Sort + count behaviour for single-pass, chunk-loaded (paged/lazy) sceneries
//! whose master can't push order down (e.g. the generic api-client).
//!
//! Two guarantees:
//!   1. A client-side sort survives a refresh. Refresh re-fetches the viewport
//!      from the master in the master's *native* order; the scenery must
//!      re-impose the active sort over the freshly-loaded rows instead of
//!      leaving them server-ordered.
//!   2. Refresh re-counts. A newly-appeared row grows `row_count` — the cached
//!      total is not frozen at its open-time value.

use std::sync::{Arc, Mutex};
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

/// Master only supplies metadata + the (false) order capability; rows come from
/// `backend` via `on_load_chunk`.
fn master() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("v", "String"))
        .with_id_column("id");
    Vista::new("items", Box::new(MockShell::new().with_metadata(metadata)))
}

type Backend = Arc<Mutex<Vec<(String, Record<CborValue>)>>>;

/// Paged lens with NO `on_refresh` — refresh flows through the scenery's
/// in-place viewport refetch. `on_load_chunk` pushes each row at its absolute
/// `backend` index (the master's native order). `total_provider` reports the
/// live `backend` length, so a re-count picks up appended rows.
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

/// Sort by `v` ascending, then refresh. The backend stays in its native order
/// (a=v3, b=v1, c=v2); the sorted view (v1, v2, v3) must survive the refresh's
/// in-place refetch.
#[tokio::test]
async fn client_sort_survives_refresh() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let backend: Backend = Arc::new(Mutex::new(vec![
        ("a".into(), rec("v3")),
        ("b".into(), rec("v1")),
        ("c".into(), rec("v2")),
    ]));
    let lens = paged_lens(tmp.path().join("c.redb"), backend.clone());
    let dio = lens.make_dio(master()).await?;
    let scenery = dio.table_scenery().open().await?;
    let mut gen_rx = scenery.subscribe();

    let g0 = u64::from(*gen_rx.borrow_and_update());
    scenery.set_viewport(0..3);
    wait_for_gen(&mut gen_rx, g0).await;
    assert_eq!(value(&scenery, 0).as_deref(), Some("v3"), "native order");

    // User sorts by `v` ascending → v1, v2, v3.
    let g1 = u64::from(*gen_rx.borrow_and_update());
    scenery.set_sort(Some("v".to_string()), SortDir::Asc);
    wait_for_gen(&mut gen_rx, g1).await;
    assert_eq!(value(&scenery, 0).as_deref(), Some("v1"), "sorted top");
    assert_eq!(value(&scenery, 2).as_deref(), Some("v3"), "sorted bottom");

    // A refresh (e.g. the page's live poll) refetches the viewport in the
    // master's native order. The active sort must be re-imposed, not clobbered.
    let g2 = u64::from(*gen_rx.borrow_and_update());
    dio.refresh().await?;
    wait_for_gen(&mut gen_rx, g2).await;

    assert_eq!(
        value(&scenery, 0).as_deref(),
        Some("v1"),
        "sort must survive refresh (top)"
    );
    assert_eq!(
        value(&scenery, 1).as_deref(),
        Some("v2"),
        "sort must survive refresh (middle)"
    );
    assert_eq!(
        value(&scenery, 2).as_deref(),
        Some("v3"),
        "sort must survive refresh (bottom)"
    );
    Ok(())
}

/// A row appended to the backend must grow `row_count` on the next refresh —
/// the open-time total is not frozen.
#[tokio::test]
async fn refresh_recounts_appended_rows() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let backend: Backend = Arc::new(Mutex::new(vec![
        ("a".into(), rec("v1")),
        ("b".into(), rec("v2")),
    ]));
    let lens = paged_lens(tmp.path().join("c.redb"), backend.clone());
    let dio = lens.make_dio(master()).await?;
    let scenery = dio.table_scenery().open().await?;
    let mut gen_rx = scenery.subscribe();

    let g0 = u64::from(*gen_rx.borrow_and_update());
    scenery.set_viewport(0..2);
    wait_for_gen(&mut gen_rx, g0).await;
    assert_eq!(scenery.row_count(), 2);

    // A new launch appears server-side.
    backend.lock().unwrap().push(("c".into(), rec("v3")));
    let g1 = u64::from(*gen_rx.borrow_and_update());
    dio.refresh().await?;
    wait_for_gen(&mut gen_rx, g1).await;

    assert_eq!(
        scenery.row_count(),
        3,
        "refresh must re-count and reflect the appended row"
    );
    Ok(())
}
