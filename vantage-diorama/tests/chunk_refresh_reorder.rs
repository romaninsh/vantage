//! A chunk-loaded grid with NO client sort, showing the master's native order
//! (e.g. REST `?ordering=-last_updated`). When that native order SHIFTS between
//! refreshes (the simulator bumps `last_updated`) and the refresh re-fetches only
//! a partial viewport by absolute offset, a row that migrated into the refetched
//! window can be left ALSO occupying its old slot — a duplicate. This reproduces
//! that scramble.

use std::sync::{Arc, Mutex};

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_diorama::TableScenery;
use vantage_types::Record;
use vantage_vista::Vista;

mod support;
use support::chunk::{
    Backend, col_at, master as master_cols, paged_lens_native_desc, settle, wait_for_gen,
};

fn rec(name: &str, lu: i64) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("name".to_string(), CborValue::Text(name.to_string()));
    r.insert("lu".to_string(), CborValue::Integer(lu.into()));
    r
}

fn name_at(scenery: &Arc<dyn TableScenery>, idx: usize) -> Option<String> {
    col_at(scenery, idx, "name")
}

fn master() -> Vista {
    master_cols(&[("name", "String"), ("lu", "i64")])
}

fn paged_lens(cache: std::path::PathBuf, backend: Backend) -> Arc<vantage_diorama::Lens> {
    paged_lens_native_desc(cache, backend, "lu")
}

#[tokio::test]
async fn refresh_after_reorder_does_not_duplicate_rows() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    // Native order lu desc: a,b,c,d (a newest).
    let backend: Backend = Arc::new(Mutex::new(vec![
        ("a".into(), rec("a", 40)),
        ("b".into(), rec("b", 30)),
        ("c".into(), rec("c", 20)),
        ("d".into(), rec("d", 10)),
    ]));
    let lens = paged_lens(tmp.path().join("c.redb"), backend.clone());
    let dio = lens.make_dio(master()).await?;
    let scenery = dio.table_scenery().open().await?;
    let mut gen_rx = scenery.subscribe();

    // Load the whole set, then narrow the live viewport to the top rows (the
    // grid's visible window is smaller than what it has loaded).
    let g = u64::from(*gen_rx.borrow_and_update());
    scenery.set_viewport(0..4);
    wait_for_gen(&mut gen_rx, g).await;
    settle().await;
    assert_eq!(name_at(&scenery, 0).as_deref(), Some("a"));
    assert_eq!(name_at(&scenery, 3).as_deref(), Some("d"));

    scenery.set_viewport(0..2); // visible window shrinks; last_viewport = 0..2
    settle().await;

    // Simulator bumps d to the top: native order becomes d,a,b,c.
    backend.lock().unwrap()[3].1 = rec("d", 99);
    let g = u64::from(*gen_rx.borrow_and_update());
    dio.refresh().await?;
    wait_for_gen(&mut gen_rx, g).await;
    settle().await;

    // Collect the visible names across the full row_count and assert no id/name
    // appears twice — a refetch of a shifted order must not duplicate rows.
    let names: Vec<String> = (0..scenery.row_count())
        .filter_map(|i| name_at(&scenery, i))
        .collect();
    let mut sorted = names.clone();
    sorted.sort();
    let before = sorted.len();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        before,
        "rows must not be duplicated after a reordering refresh; saw {names:?}"
    );
    Ok(())
}
