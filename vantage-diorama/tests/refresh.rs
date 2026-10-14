//! Non-blanking refresh for chunk-loaded (paged/lazy) sceneries.
//!
//! A refresh re-fetches the last viewport in place: fresh rows overwrite the
//! cached slots, and a *failed* refetch leaves the existing rows untouched —
//! the grid never goes blank. Contrast with the old behaviour, where refresh
//! cleared the cache and waited for a refill (any lag or error blanked the
//! visible rows while their count survived).

mod support;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_diorama::TableScenery;
use vantage_types::Record;

use support::chunk::{Backend, col_at, master, paged_lens, paged_lens_failing, wait_for_gen};

fn rec(v: &str) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("v".to_string(), CborValue::Text(v.to_string()));
    r
}

fn value(scenery: &Arc<dyn TableScenery>, idx: usize) -> Option<String> {
    col_at(scenery, idx, "v")
}

#[tokio::test]
async fn refresh_updates_visible_rows_in_place() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let backend: Backend = Arc::new(Mutex::new(vec![
        ("a".into(), rec("v1")),
        ("b".into(), rec("v1")),
    ]));
    let lens = paged_lens(tmp.path().join("c.redb"), backend.clone());
    let dio = lens.dio(master(&[("v", "String")])).build().await?;
    let scenery = dio.table_scenery().open().await?;
    let mut gen_rx = scenery.subscribe();

    let g0 = u64::from(*gen_rx.borrow_and_update());
    scenery.set_viewport(0..2, 0..0);
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
    let lens = paged_lens_failing(
        tmp.path().join("c.redb"),
        backend.clone(),
        fail_next.clone(),
    );
    let dio = lens.dio(master(&[("v", "String")])).build().await?;
    let scenery = dio.table_scenery().open().await?;
    let mut gen_rx = scenery.subscribe();

    let g0 = u64::from(*gen_rx.borrow_and_update());
    scenery.set_viewport(0..1, 0..0);
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
