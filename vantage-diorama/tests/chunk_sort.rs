//! A refresh of a single-pass, chunk-loaded (paged) scenery re-counts: a
//! newly-appeared row grows `row_count`, so the cached total is not frozen at
//! its open-time value.

use std::sync::{Arc, Mutex};

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_types::Record;

mod support;
use support::chunk::{Backend, master as master_cols, paged_lens, wait_for_gen};

fn rec(v: &str) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("v".to_string(), CborValue::Text(v.to_string()));
    r
}

fn master() -> vantage_vista::Vista {
    master_cols(&[("v", "String")])
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
    let dio = lens.dio(master()).build().await?;
    let scenery = dio.table_scenery().open().await?;
    let mut gen_rx = scenery.subscribe();

    let g0 = u64::from(*gen_rx.borrow_and_update());
    scenery.set_viewport(0..2, 0..0);
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
