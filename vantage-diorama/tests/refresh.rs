//! Non-blanking refresh for chunk-loaded (paged/lazy) sceneries.
//!
//! A refresh re-fetches the last viewport in place: fresh rows overwrite the
//! cached slots, and a *failed* refetch leaves the existing rows untouched —
//! the grid never goes blank. Contrast with the old behaviour, where refresh
//! cleared the cache and waited for a refill (any lag or error blanked the
//! visible rows while their count survived).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_diorama::{Generation, Lens, TableScenery};
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

/// A master is required to build a Dio; this scenery loads from `backend`
/// via `on_load_chunk`, not the master, so a bare MockShell suffices.
fn master() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("v", "String"))
        .with_id_column("id");
    Vista::new("items", Box::new(MockShell::new().with_metadata(metadata)))
}

type Backend = Arc<Mutex<Vec<(String, Record<CborValue>)>>>;

/// Paged lens with NO `on_refresh` — refresh flows through the scenery's
/// in-place viewport refetch, the path under test.
fn paged_lens(
    cache: std::path::PathBuf,
    backend: Backend,
    fail_next: Arc<AtomicBool>,
) -> Arc<Lens> {
    let total = backend.clone();
    let lens = Lens::new()
        .cache_at(cache)
        .total_provider(move |_dio| {
            let b = total.clone();
            async move { Ok(b.lock().unwrap().len()) }
        })
        .on_load_chunk(move |_dio, range, sink| {
            let b = backend.clone();
            let fail = fail_next.clone();
            async move {
                if fail.swap(false, Ordering::SeqCst) {
                    return Err(vantage_core::error!("simulated chunk failure"));
                }
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

#[tokio::test]
async fn refresh_updates_visible_rows_in_place() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let backend: Backend = Arc::new(Mutex::new(vec![
        ("a".into(), rec("v1")),
        ("b".into(), rec("v1")),
    ]));
    let fail_next = Arc::new(AtomicBool::new(false));
    let lens = paged_lens(tmp.path().join("c.redb"), backend.clone(), fail_next);
    let dio = lens.make_dio(master()).await?;
    let scenery = dio.table_scenery().open().await?;
    let mut gen_rx = scenery.subscribe();

    let g0 = u64::from(*gen_rx.borrow_and_update());
    scenery.set_viewport(0..2);
    wait_for_gen(&mut gen_rx, g0).await;
    assert_eq!(value(&scenery, 0).as_deref(), Some("v1"));

    // Source changes underneath us; refresh must reflect it in place.
    backend.lock().unwrap()[0].1 = rec("v2");
    let g1 = u64::from(*gen_rx.borrow_and_update());
    dio.refresh().await?;
    wait_for_gen(&mut gen_rx, g1).await;

    assert_eq!(
        value(&scenery, 0).as_deref(),
        Some("v2"),
        "refresh updates in place"
    );
    assert_eq!(scenery.row_count(), 2);
    Ok(())
}

#[tokio::test]
async fn failed_refresh_keeps_rows_instead_of_blanking() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let backend: Backend = Arc::new(Mutex::new(vec![("a".into(), rec("v1"))]));
    let fail_next = Arc::new(AtomicBool::new(false));
    let lens = paged_lens(
        tmp.path().join("c.redb"),
        backend.clone(),
        fail_next.clone(),
    );
    let dio = lens.make_dio(master()).await?;
    let scenery = dio.table_scenery().open().await?;
    let mut gen_rx = scenery.subscribe();

    let g0 = u64::from(*gen_rx.borrow_and_update());
    scenery.set_viewport(0..1);
    wait_for_gen(&mut gen_rx, g0).await;
    assert_eq!(value(&scenery, 0).as_deref(), Some("v1"));

    // The next chunk fetch will fail (e.g. a 504). The refresh must NOT
    // clear the row — the old value has to survive.
    backend.lock().unwrap()[0].1 = rec("v2");
    fail_next.store(true, Ordering::SeqCst);
    dio.refresh().await?;
    tokio::time::sleep(Duration::from_millis(200)).await;

    assert_eq!(
        value(&scenery, 0).as_deref(),
        Some("v1"),
        "a failed refresh must keep the previous value, not blank it"
    );
    assert!(
        scenery.row(0).is_some(),
        "row must not be cleared on a failed refresh"
    );
    Ok(())
}
